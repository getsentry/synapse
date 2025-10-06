#![allow(unused_variables)]

use crate::backup_routes::{BackupError, BackupRouteProvider, RouteData};
use crate::types::Cell;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::sync::{mpsc, oneshot};

#[derive(thiserror::Error, Debug)]
pub enum LookupError {
    #[error("requested locality does not match the cell's locality")]
    LocalityMismatch { requested: String, actual: String },
    #[allow(dead_code)]
    #[error("the locator is not ready yet")]
    NotReady(String),
}

#[derive(thiserror::Error, Debug)]
pub enum LoadError {
    #[error("Error loading backup")]
    BackupError(#[from] BackupError),
    #[error("Another load operation is in progress")]
    ConcurrentLoad,
}

#[allow(dead_code)]
pub enum Command {
    // Trigger incremental mapping refresh outside of the normal interval.
    // The worker sends Ok(()) when the refresh attempt finishes.
    Refresh(oneshot::Sender<Result<(), LoadError>>),
    // Trigger the loader to shudown gracefully
    Shutdown,
}

#[derive(Clone)]
struct OrgToCellInner {
    mapping: HashMap<String, Cell>,
    locality_to_default_cell: HashMap<String, Cell>,
    last_cursor: Option<String>,
}

#[derive(Clone)]
pub struct OrgToCell {
    inner: Arc<RwLock<OrgToCellInner>>,
    update_lock: Arc<Semaphore>,
    // Used by the healthcheck. Initially false and set to true once any snapshot
    // has been loaded and mappings are available.
    ready: Arc<AtomicBool>,
    // last_update: Arc<RwLock<Option<SystemTime>>>,
    backup_routes: Arc<dyn BackupRouteProvider + Send + Sync>,
}

impl OrgToCell {
    pub fn new(backup_routes: Arc<dyn BackupRouteProvider + Send + Sync>) -> Self {
        OrgToCell {
            inner: Arc::new(RwLock::new(OrgToCellInner {
                mapping: HashMap::new(),
                locality_to_default_cell: HashMap::new(),
                last_cursor: None,
            })),
            update_lock: Arc::new(Semaphore::new(1)),
            ready: Arc::new(AtomicBool::new(false)),
            backup_routes,
        }
    }

    pub fn lookup(
        &self,
        org_id: &str,
        locality: Option<&str>,
    ) -> Result<Option<Cell>, LookupError> {
        // Looks up the cell for a given organization ID and locality.
        // Returns an `Option<Cell>` if found, or `None` if not found.
        // Returns an error if locality is passed and the org_id/locality pair is not valid.
        // Or if a locality is passed but no defualt cell is found for that locality
        let read_guard = self.inner.read();

        let cell = read_guard.mapping.get(org_id);

        match cell {
            Some(cell) => {
                if let Some(loc) = locality
                    && cell.locality.as_str() != loc
                {
                    return Err(LookupError::LocalityMismatch {
                        requested: loc.to_string(),
                        actual: cell.locality.to_string(),
                    });
                }
                Ok(Some(cell.clone()))
            }
            None => {
                // Use default cell if one is defined for the locality
                if let Some(loc) = locality
                    && let Some(default_cell) = read_guard.locality_to_default_cell.get(loc)
                {
                    return Ok(Some(default_cell.clone()));
                }

                // No default cell found
                Ok(None)
            }
        }
    }

    /// Performs an initial full load, then periodically reloads
    /// mappings at the configured interval or on demand when the Refresh
    /// command is received. The loop runs indefinitely until the Shutdown
    /// command is received.
    pub async fn run_loader_worker(&self, rx: mpsc::Receiver<Command>) {
        if let Ok(()) = self.load_snapshot().await {
            self.ready.store(true, Ordering::Relaxed);

            // Once a snapshot is loaded, the worker periodically requests incremental results
            // until the Shutdown command is received.
            // If the Refresh command is received, the incremental load can be triggered ahead
            // of schedule.
            // loop {

            // }
        }
    }

    /// Loads the entire mapping in pages from the control plane.
    /// If the control plane is unreachable, fall back to stored local copy.
    /// This function should attempt to fetch data from the control plane.
    /// Once the configured retries have been exhausted, it will attempt to
    /// load from the backup route provider.
    async fn load_snapshot(&self) -> Result<(), LoadError> {
        // Hold permit for the duration of this function
        let _permit = self.get_permit()?;

        // Testing - load from the backup route provider
        let route_data: RouteData = self.backup_routes.load()?;

        let mut write_guard: parking_lot::lock_api::RwLockWriteGuard<
            '_,
            parking_lot::RawRwLock,
            OrgToCellInner,
        > = self.inner.write();

        write_guard.mapping = route_data.routes;
        write_guard.last_cursor = Some(route_data.last_cursor);

        Ok(())
    }

    /// Load incremental updates from the control plane.
    #[allow(dead_code)]
    async fn load_incremental(&self) -> Result<(), LoadError> {
        // Hold permit for the duration of this function
        let _permit = self.get_permit()?;

        // Do loading

        Ok(())
    }

    /// Guard that ensures only one load operation is in progress at a time.
    fn get_permit(&self) -> Result<OwnedSemaphorePermit, LoadError> {
        if let Ok(permit) = self.update_lock.clone().try_acquire_owned() {
            Ok(permit)
        } else {
            Err(LoadError::ConcurrentLoad)
        }
    }
}
