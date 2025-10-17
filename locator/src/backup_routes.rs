#![allow(dead_code, unused_variables)]

/// The fallback route provider enables org to cell mappings to be loaded from
/// a previously stored copy, even when the control plane is unavailable.
use crate::types::Cell;
use std::collections::HashMap;
use std::io;

type RouteMap = HashMap<String, Cell>;

pub struct RouteData {
    pub mapping: HashMap<String, Cell>,
    pub locality_to_default_cell: HashMap<String, Cell>,
    pub last_cursor: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum BackupError {
    #[error("Error loading routes: {source}")]
    LoadRoutes {
        #[source]
        source: io::Error,
    },
    #[error("Error storing routes: {source}")]
    StoreRoutes {
        #[source]
        source: io::Error,
    },
}

pub trait BackupRouteProvider: Send + Sync {
    fn load(&self) -> Result<RouteData, BackupError>;
    fn store(&self, route_data: &RouteData) -> Result<(), BackupError>;
}

// No-op backup route provider for testing
pub struct NoopRouteProvider {}

impl BackupRouteProvider for NoopRouteProvider {
    fn load(&self) -> Result<RouteData, BackupError> {
        eprintln!(
            "Warning: loading backup routes from the no-op provider. This is unsafe for production use."
        );

        Ok(RouteData {
            mapping: HashMap::new(),
            locality_to_default_cell: HashMap::new(),
            last_cursor: Some("test".into()),
        })
    }

    fn store(&self, route_data: &RouteData) -> Result<(), BackupError> {
        // Do nothing
        Ok(())
    }
}

pub struct FilesystemRouteProvider {
    pub path: String,
}

impl FilesystemRouteProvider {
    pub fn new(path: &str) -> Self {
        FilesystemRouteProvider { path: path.into() }
    }
}

impl BackupRouteProvider for FilesystemRouteProvider {
    fn load(&self) -> Result<RouteData, BackupError> {
        unimplemented!();
    }

    fn store(&self, route_data: &RouteData) -> Result<(), BackupError> {
        unimplemented!();
    }
}

pub struct GcsRouteProvider {}

impl GcsRouteProvider {
    pub fn new(bucket: &str) -> Self {
        GcsRouteProvider {}
    }
}

impl BackupRouteProvider for GcsRouteProvider {
    fn load(&self) -> Result<RouteData, BackupError> {
        unimplemented!();
    }

    fn store(&self, route_data: &RouteData) -> Result<(), BackupError> {
        unimplemented!();
    }
}
