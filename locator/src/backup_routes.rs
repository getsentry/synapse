#![allow(dead_code, unused_variables)]

/// The fallback route provider enables org to cell mappings to be loaded from
/// a previously stored copy, even when the control plane is unavailable.
use crate::types::Cell;
use std::collections::HashMap;

type RouteMap = HashMap<String, Cell>;

pub struct RouteData {
    pub routes: RouteMap,
    pub last_cursor: String,
}

#[derive(Debug)]
pub struct BackupError {
    message: String,
}

pub trait BackupRouteProvider: Send + Sync {
    fn load(&self) -> Result<RouteData, BackupError>;
    fn store(&self, route_data: &RouteData) -> Result<(), BackupError>;
}

// No-op backup route provider for testing
pub struct NoopRouteProvider {}

impl BackupRouteProvider for NoopRouteProvider {
    fn load(&self) -> Result<RouteData, BackupError> {
        println!(
            "Warning: loading backup routes from the no-op provider. This is unsafe for production use."
        );

        Ok(RouteData {
            routes: HashMap::new(),
            last_cursor: "test".into(),
        })
    }

    fn store(&self, route_data: &RouteData) -> Result<(), BackupError> {
        // Do nothing
        Ok(())
    }
}

// Temporary. Generates placeholder data for testing.
pub struct PlaceholderRouteProvider {}

impl BackupRouteProvider for PlaceholderRouteProvider {
    fn load(&self) -> Result<RouteData, BackupError> {
        let cells = [
            Cell::new("us1", "us"),
            Cell::new("us2", "us"),
            Cell::new("de", "de"),
        ];

        let mut dummy_data = HashMap::new();
        for i in 0..10 {
            dummy_data.insert(format!("org_{i}"), cells[i % cells.len()].clone());
        }

        Ok(RouteData {
            routes: dummy_data,
            last_cursor: "test".into(),
        })
    }

    fn store(&self, route_data: &RouteData) -> Result<(), BackupError> {
        // Do nothing
        Ok(())
    }
}

pub struct FilesystemRouteProvider {}

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
    fn new(bucket: &str) -> Self {
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
