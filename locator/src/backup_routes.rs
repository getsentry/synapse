/// The fallback route provider enables org to cell mappings to be loaded from
/// a previously stored copy, even when the control plane is unavailable.
use crate::types::RouteData;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(thiserror::Error, Debug)]
pub enum BackupError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("encode error: {0}")]
    Encode(#[from] bincode::error::EncodeError),

    #[error("decode error: {0}")]
    Decode(#[from] bincode::error::DecodeError),

    #[error("gcs error: {0}")]
    Gcs(#[from] google_cloud_storage::Error),

    #[error("gcs client initialization error: {0}")]
    GcsInit(String),
}

#[async_trait::async_trait]
pub trait BackupRouteProvider: Send + Sync {
    async fn load(&self) -> Result<RouteData, BackupError>;
    async fn store(&self, route_data: &RouteData) -> Result<(), BackupError>;
}

#[derive(Clone)]
enum Compression {
    #[allow(dead_code)]
    None,
    // zstd with compression level
    Zstd(i32),
}

struct Codec {
    compression: Compression,
    config: bincode::config::Configuration,
}

impl Codec {
    fn new(compression: Compression) -> Self {
        Codec {
            compression,
            // standard defaults to little-endian + varint
            config: bincode::config::standard(),
        }
    }

    fn write<W: Write>(&self, writer: &mut W, data: &RouteData) -> Result<usize, BackupError> {
        match self.compression {
            Compression::None => {
                let size = bincode::encode_into_std_write(data, writer, self.config)?;
                writer.flush()?;
                Ok(size)
            }
            Compression::Zstd(level) => {
                let mut encoder = zstd::stream::write::Encoder::new(writer, level)?;
                let size = bincode::encode_into_std_write(data, &mut encoder, self.config)?;
                encoder.finish()?;
                Ok(size)
            }
        }
    }

    fn read<R: Read>(&self, mut reader: R) -> Result<RouteData, BackupError> {
        match self.compression {
            Compression::None => {
                let value: RouteData = bincode::decode_from_std_read(&mut reader, self.config)?;
                Ok(value)
            }
            Compression::Zstd(_) => {
                let mut decoder = zstd::stream::read::Decoder::new(reader)?;
                let decoded: RouteData = bincode::decode_from_std_read(&mut decoder, self.config)?;
                Ok(decoded)
            }
        }
    }
}

pub struct FilesystemRouteProvider {
    path: PathBuf,
    codec: Codec,
}

impl FilesystemRouteProvider {
    pub fn new(base_dir: &str, filename: &str) -> Self {
        FilesystemRouteProvider {
            path: Path::new(base_dir).join(filename),
            codec: Codec::new(Compression::Zstd(1)),
        }
    }
}

#[async_trait::async_trait]
impl BackupRouteProvider for FilesystemRouteProvider {
    async fn load(&self) -> Result<RouteData, BackupError> {
        let file = File::open(&self.path)?;
        let reader = io::BufReader::new(file);
        self.codec.read(reader)
    }

    async fn store(&self, route_data: &RouteData) -> Result<(), BackupError> {
        // Create or overwrite file
        let file = File::create(&self.path)?;

        let mut writer = io::BufWriter::new(file);

        let size = self.codec.write(&mut writer, route_data);

        tracing::info!(
            "Stored backup routes to {:?}, bytes: {:?}",
            &self.path,
            size
        );

        Ok(())
    }
}

pub struct GcsRouteProvider {
    bucket_name: String,
    codec: Codec,
    object_key: String,
    client: google_cloud_storage::client::Storage,
    // If the last cursor does not change, skip upload
    // Though this means this class is aware of the payload shape now
    last_cursor: Mutex<Option<String>>,
}

impl GcsRouteProvider {
    pub async fn new(bucket: String) -> Result<Self, BackupError> {
        let client = google_cloud_storage::client::Storage::builder()
            .build()
            .await
            .map_err(|e| BackupError::GcsInit(e.to_string()))?;

        let bucket_name = format!("projects/_/buckets/{}", &bucket);

        Ok(GcsRouteProvider {
            bucket_name,
            codec: Codec::new(Compression::Zstd(1)),
            object_key: "backup-routes.bin".to_string(),
            client,
            last_cursor: Mutex::new(None),
        })
    }
}

#[async_trait::async_trait]
impl BackupRouteProvider for GcsRouteProvider {
    async fn load(&self) -> Result<RouteData, BackupError> {
        // Download the object from GCS using the new API
        let mut response = self
            .client
            .read_object(&self.bucket_name, &self.object_key)
            .send()
            .await?;

        // Collect all chunks into a buffer
        let mut data = Vec::new();
        while let Some(chunk) = response.next().await {
            data.extend_from_slice(&chunk?);
        }

        // Decode the data using the codec
        let reader = io::Cursor::new(data);
        let data = self.codec.read(reader)?;

        // Better to unwrap than return error here as it should never happen, we
        // prefer to panic than risk continuing with corrupted state
        let mut guard = self.last_cursor.lock().unwrap();
        *guard = Some(data.last_cursor.clone());

        Ok(data)
    }

    async fn store(&self, route_data: &RouteData) -> Result<(), BackupError> {
        // Return early if the last cursor is unchanged
        if self.last_cursor.lock().unwrap().as_ref() == Some(&route_data.last_cursor) {
            tracing::info!(
                "Skipping upload to GCS bucket {}, object {} as last cursor is unchanged: {}",
                &self.bucket_name,
                &self.object_key,
                &route_data.last_cursor
            );
            return Ok(());
        }

        // Encode the data using the codec
        let mut buffer: Vec<u8> = Vec::new();
        let size = self.codec.write(&mut buffer, route_data)?;

        // Upload the object to GCS using the new API
        let bytes_data = bytes::Bytes::from(buffer);
        let _ = self
            .client
            .write_object(&self.bucket_name, &self.object_key, bytes_data)
            .send_buffered()
            .await?;

        tracing::info!(
            "Stored backup routes to GCS bucket {}, object {}, bytes: {}",
            &self.bucket_name,
            &self.object_key,
            size
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Cell;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn get_route_data() -> RouteData {
        RouteData {
            id_to_cell: HashMap::from([("org1".into(), "cell1".into())]),
            last_cursor: "cursor1".into(),
            cells: HashMap::from([(
                "cell1".into(),
                Arc::new(Cell {
                    id: "cell1".into(),
                    locality: "us".into(),
                }),
            )]),
        }
    }

    #[test]
    fn test_codec() {
        for compression in [
            Compression::None,
            Compression::Zstd(1),
            Compression::Zstd(3),
        ] {
            let codec = Codec::new(compression.clone());
            let data = get_route_data();
            let mut buffer: Vec<u8> = Vec::new();
            let size = codec.write(&mut buffer, &data).unwrap();
            assert_eq!(size, 36);
            let mut reader: &[u8] = &buffer;
            let decoded = codec.read(&mut reader).unwrap();
            assert_eq!(data, decoded);
        }
    }

    #[tokio::test]
    async fn test_filesystem() {
        let dir = tempfile::tempdir().unwrap();

        let provider = FilesystemRouteProvider::new(dir.path().to_str().unwrap(), "backup.bin");
        let data = get_route_data();

        provider.store(&data).await.unwrap();
        let loaded = provider.load().await.unwrap();
        assert_eq!(data, loaded);
    }

    #[tokio::test]
    async fn test_gcs() {
        let endpoint = "http://localhost:4443";
        let bucket = "test-bucket";

        let mut provider = GcsRouteProvider::new(bucket.into()).await.unwrap();

        // Override the client so we can use the local emulator
        provider.client = google_cloud_storage::client::Storage::builder()
            .with_endpoint(endpoint.to_string())
            .with_credentials(google_cloud_auth::credentials::anonymous::Builder::new().build())
            .build()
            .await
            .unwrap();

        let data = get_route_data();

        provider.store(&data).await.unwrap();
        let loaded = provider.load().await.unwrap();
        assert_eq!(data, loaded);

        // The stored data didn't change since the upload is skipped
        let mut data_modified = data.clone();
        data_modified.cells = HashMap::new();
        provider.store(&data_modified).await.unwrap();
        let loaded = provider.load().await.unwrap();
        assert_eq!(data, loaded);
    }
}
