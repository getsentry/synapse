#![allow(dead_code, unused_variables)]

use crate::types::{Cell, RouteData};
/// The fallback route provider enables org to cell mappings to be loaded from
/// a previously stored copy, even when the control plane is unavailable.
use std::collections::HashMap;
use std::io;
use std::sync::Arc;

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
            org_to_cell: HashMap::new(),
            locality_to_default_cell: HashMap::new(),
            last_cursor: "test".into(),
            cells: HashMap::new(),
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

// Temporary - only used for testing. Replace test cases with the filesystem provider
// once that is implemented to avoid keeping this dummy code around.
pub struct TestingRouteProvider;

impl BackupRouteProvider for TestingRouteProvider {
    fn load(&self) -> Result<RouteData, BackupError> {
        let cells = Vec::from([
            Cell::new("us1", "us"),
            Cell::new("us2", "us"),
            Cell::new("de", "de"),
        ]);

        let mut dummy_data = HashMap::new();
        for i in 0..10 {
            dummy_data.insert(format!("org_{i}"), cells[i % cells.len()].id.clone());
        }

        Ok(RouteData {
            org_to_cell: dummy_data,
            last_cursor: "test".into(),
            locality_to_default_cell: HashMap::from([("de".into(), "de".into())]),
            cells: HashMap::from_iter(cells.into_iter().map(|c| (c.id.clone(), Arc::new(c)))),
        })
    }

    fn store(&self, _route_data: &RouteData) -> Result<(), BackupError> {
        Ok(())
    }
}
