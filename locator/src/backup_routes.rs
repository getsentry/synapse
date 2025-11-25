use crate::config;
/// The fallback route provider enables org to cell mappings to be loaded from
/// a previously stored copy, even when the control plane is unavailable.
use crate::cursor::Cursor;
use crate::types::RouteData;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static METADATA_KEY: &str = "last_cursor";

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

    #[error("invalid cursor: {0}")]
    Cursor(#[from] crate::cursor::CursorError),

    #[error("metadata error: {0}")]
    MetadataError(String),

    #[error("reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
}

#[async_trait::async_trait]
pub trait BackupRouteProvider: Send + Sync {
    async fn load(&self) -> Result<RouteData, BackupError>;
    async fn store(&self, route_data: &RouteData) -> Result<(), BackupError>;
}

#[derive(Clone)]
enum Compression {
    None,
    Gzip,
    // zstd with compression level
    Zstd(i32),
}

impl From<config::Compression> for Compression {
    fn from(value: config::Compression) -> Self {
        match value {
            config::Compression::None => Compression::None,
            config::Compression::Gip => Compression::Gzip,
            config::Compression::Zstd1 => Compression::Zstd(1),
            config::Compression::Zstd3 => Compression::Zstd(3),
        }
    }
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
            Compression::Gzip => {
                let mut encoder =
                    flate2::write::GzEncoder::new(writer, flate2::Compression::default());
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
            Compression::Gzip => {
                let mut decoder = flate2::read::GzDecoder::new(reader);
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
    pub fn new(base_dir: &str, filename: &str, compression: config::Compression) -> Self {
        FilesystemRouteProvider {
            path: Path::new(base_dir).join(filename),
            codec: Codec::new(compression.into()),
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

// The google-cloud-storage crate does not expose a way to view the object metadata via the Storage client.
// The StorageControl client does have this functionality but uses the grpc API which doesn't work with our
// emulator. This client just queries the API directly.
struct MetadataClient {
    client: reqwest::Client,
    bucket_name: String,
    object_key: String,
    base_url: String,
}

impl MetadataClient {
    pub fn new(bucket_name: &str, object_key: &str) -> Self {
        let base_url = "https://storage.googleapis.com".to_string();

        MetadataClient {
            client: reqwest::Client::new(),
            bucket_name: bucket_name.to_string(),
            object_key: object_key.to_string(),
            base_url,
        }
    }

    // Returns the cursor if found in metadata or none if it doesn't exist.
    // BackupError is returned for all other errors.
    pub async fn get_cursor(&self) -> Result<Option<Cursor>, BackupError> {
        let full_url = format!(
            "{}/storage/v1/b/{}/o/{}",
            self.base_url, self.bucket_name, &self.object_key
        );

        let resp = self.client.get(&full_url).send().await?;

        // Object does not exist yet. This happens the first time Synapse is run.
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let json_value: serde_json::Value = resp.json().await?;

        if let Some(val) = json_value["metadata"][METADATA_KEY].as_str() {
            let cursor: Cursor = val.parse()?;
            Ok(Some(cursor))
        } else {
            Err(BackupError::MetadataError(
                "Metadata key not found".to_string(),
            ))
        }
    }
}

// Route provider alternative that uses Google Cloud storage instead of local filesystem.
// This code does not handle object versioning and TTLs -- this should be configured at
//the bucket level.
// This backend assumes Google's Application Default Credentials are being used.
pub struct GcsRouteProvider {
    bucket_name: String,
    codec: Codec,
    object_key: String,
    // GCS Storage client used for reading/writing objects
    client: google_cloud_storage::client::Storage,
    // Client used for metadata requests
    metadata_client: MetadataClient,
    // The latest known cursor in the backup store. Used to avoid redundant uploads.
    last_cursor: Mutex<Option<Cursor>>,
}

impl GcsRouteProvider {
    pub async fn new(
        bucket: String,
        compression: config::Compression,
    ) -> Result<Self, BackupError> {
        let object_key = "backup-routes.bin".to_string();

        let client = google_cloud_storage::client::Storage::builder()
            .build()
            .await
            .map_err(|e| BackupError::GcsInit(e.to_string()))?;

        let metadata_client = MetadataClient::new(&bucket, &object_key);

        // In GCS, bucket names are globally unique, project is not specified
        let bucket_name = format!("projects/_/buckets/{}", &bucket);

        Ok(GcsRouteProvider {
            bucket_name,
            codec: Codec::new(compression.into()),
            object_key: object_key.clone(),
            client,
            metadata_client,
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

        let last_cursor = data.last_cursor.parse()?;

        // This shouldn't happen: prefer to unwrap/panic than risk continuing with corrupted state
        let mut guard = self.last_cursor.lock().unwrap();
        *guard = Some(last_cursor);

        Ok(data)
    }

    async fn store(&self, route_data: &RouteData) -> Result<(), BackupError> {
        let new_last_cursor = route_data.last_cursor.parse()?;

        // Check the cursor stored in GCS metadata first. Only proceed with
        // write if the new cursor is later than the one already stored.
        // If there is no cursor, the file may not exist proceed with the write.
        let metadata_cursor = self.metadata_client.get_cursor().await?;

        if let Some(mc) = metadata_cursor
            && mc >= new_last_cursor
        {
            tracing::info!(
                metadata_cursor = ?mc,
                new_last_cursor = ?new_last_cursor,
                "Skipping route store: GCS version is already up to date"
            );

            return Ok(());
        }

        // Encode the data using the codec
        let mut buffer: Vec<u8> = Vec::new();
        let size = self.codec.write(&mut buffer, route_data)?;

        let bytes_data = bytes::Bytes::from(buffer);
        let _ = self
            .client
            .write_object(&self.bucket_name, &self.object_key, bytes_data)
            .set_metadata([(METADATA_KEY, &route_data.last_cursor)])
            .send_buffered()
            .await?;

        // Update last cursor if the write was successful
        let last_cursor = route_data.last_cursor.parse()?;
        let mut guard = self.last_cursor.lock().unwrap();
        *guard = Some(last_cursor);

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
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn get_route_data() -> RouteData {
        let cursor_json_str = serde_json::json!({
            "updated_at": 1757030409,
            "id": null
        })
        .to_string();
        let last_cursor = STANDARD.encode(cursor_json_str.as_bytes());

        RouteData {
            id_to_cell: HashMap::from([("org1".into(), "cell1".into())]),
            last_cursor,
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
            Compression::Gzip,
        ] {
            let codec = Codec::new(compression.clone());
            let data = get_route_data();
            let mut buffer: Vec<u8> = Vec::new();
            let size = codec.write(&mut buffer, &data).unwrap();
            assert_eq!(size, 77);
            let mut reader: &[u8] = &buffer;
            let decoded = codec.read(&mut reader).unwrap();
            assert_eq!(data, decoded);
        }
    }

    #[tokio::test]
    async fn test_filesystem() {
        let dir = tempfile::tempdir().unwrap();

        let provider = FilesystemRouteProvider::new(dir.path().to_str().unwrap(), "backup.bin", config::Compression::Zstd1);
        let data = get_route_data();

        provider.store(&data).await.unwrap();
        let loaded = provider.load().await.unwrap();
        assert_eq!(data, loaded);
    }

    #[tokio::test]
    async fn test_gcs() {
        let endpoint = "http://localhost:4443";
        let bucket = "test-bucket";

        let mut provider = GcsRouteProvider::new(bucket.into(), config::Compression::Zstd1).await.unwrap();

        // Override the clients so we can use the local emulator
        provider.client = google_cloud_storage::client::Storage::builder()
            .with_endpoint(endpoint.to_string())
            .with_credentials(google_cloud_auth::credentials::anonymous::Builder::new().build())
            .build()
            .await
            .unwrap();

        provider.metadata_client.base_url = endpoint.to_string();

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
