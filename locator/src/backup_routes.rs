/// The fallback route provider enables org to cell mappings to be loaded from
/// a previously stored copy, even when the control plane is unavailable.
use crate::types::RouteData;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[derive(thiserror::Error, Debug)]
pub enum BackupError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("encode error: {0}")]
    Encode(#[from] bincode::error::EncodeError),

    #[error("decode error: {0}")]
    Decode(#[from] bincode::error::DecodeError),
}

pub trait BackupRouteProvider: Send + Sync {
    fn load(&self) -> Result<RouteData, BackupError>;
    fn store(&self, route_data: &RouteData) -> Result<(), BackupError>;
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

// No-op backup route provider for testing
pub struct NoopRouteProvider {}

impl BackupRouteProvider for NoopRouteProvider {
    fn load(&self) -> Result<RouteData, BackupError> {
        eprintln!(
            "Warning: loading backup routes from the no-op provider. This is unsafe for production use."
        );

        Ok(RouteData {
            org_to_cell: HashMap::new(),
            locality_to_default_cell: HashMap::new(),
            last_cursor: "test".into(),
            cells: HashMap::new(),
        })
    }

    fn store(&self, _route_data: &RouteData) -> Result<(), BackupError> {
        // Do nothing
        Ok(())
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

impl BackupRouteProvider for FilesystemRouteProvider {
    fn load(&self) -> Result<RouteData, BackupError> {
        let file = File::open(&self.path)?;
        let reader = io::BufReader::new(file);
        self.codec.read(reader)
    }

    fn store(&self, route_data: &RouteData) -> Result<(), BackupError> {
        // Create or overwrite file
        let file = File::create(&self.path)?;

        let mut writer = io::BufWriter::new(file);

        let size = self.codec.write(&mut writer, route_data);

        println!(
            "Stored backup routes to {:?}, bytes: {:?}",
            &self.path, size
        );

        Ok(())
    }
}

pub struct GcsRouteProvider {}

impl GcsRouteProvider {
    pub fn new(_bucket: &str) -> Self {
        GcsRouteProvider {}
    }
}

impl BackupRouteProvider for GcsRouteProvider {
    fn load(&self) -> Result<RouteData, BackupError> {
        unimplemented!();
    }

    fn store(&self, _route_data: &RouteData) -> Result<(), BackupError> {
        unimplemented!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Cell;
    use std::sync::Arc;

    fn get_route_data() -> RouteData {
        RouteData {
            org_to_cell: HashMap::from([("org1".into(), "cell1".into())]),
            locality_to_default_cell: HashMap::from([("us".into(), "cell1".into())]),
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
            assert_eq!(size, 46);
            let mut reader: &[u8] = &buffer;
            let decoded = codec.read(&mut reader).unwrap();
            assert_eq!(data, decoded);
        }
    }

    #[test]
    fn test_filesystem() {
        let dir = tempfile::tempdir().unwrap();

        let provider = FilesystemRouteProvider::new(dir.path().to_str().unwrap(), "backup.bin");
        let data = get_route_data();

        provider.store(&data).unwrap();
        let loaded = provider.load().unwrap();
        assert_eq!(data, loaded);
    }
}
