use parking_lot::RwLock;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::sync::{mpsc, oneshot};
use crate::backup_routes::{BackupRouteProvider, RouteData};
use crate::types::Cell;

#[derive(Debug)]
pub struct LookupError {
    message: String,
}

impl LookupError {
    fn new(msg: &str) -> Self {
        LookupError {
            message: msg.to_string(),
        }
    }
}

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[derive(Debug)]
pub struct LoadError {
    message: String,
}


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
    pub fn new(backup_routes: impl BackupRouteProvider + 'static) -> Self {
        OrgToCell {
            inner: Arc::new(RwLock::new(OrgToCellInner {
                mapping: HashMap::new(),
                locality_to_default_cell: HashMap::new(),
                last_cursor: None,
            })),
            update_lock: Arc::new(Semaphore::new(1)),
            ready: Arc::new(AtomicBool::new(false)),
            backup_routes: Arc::new(backup_routes)
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
                    return Err(LookupError::new("locality mismatch"));
                }
                Ok(Some(cell.clone()))
            }
            None => {
                if let Some(locality) = locality {
                    if let Some(default_cell) = read_guard.locality_to_default_cell.get(locality) {
                        Ok(Some(default_cell.clone()))
                    } else {
                        Err(LookupError::new(&format!(
                            "No cell found for org_id '{org_id}' and locality '{locality}'"
                        )))
                    }
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// Performs an initial full load, then periodically reloads
    /// mappings at the configured interval or on demand when the Refresh
    /// command is received. The loop runs indefinitely until the Shutdown
    /// is received.
    pub async fn run_loader_worker(&self, rx: mpsc::Receiver<Command>) {
        if let Ok(()) = self.load_snapshot().await {
            self.ready.store(true, Ordering::Relaxed);
        }
    }

    /// Loads the entire mapping in pages from the control plane.
    /// If the control plane is unreachable, fall back to stored local copy.
    async fn load_snapshot(&self) -> Result<(), LoadError> {
        // Hold permit for the duration of this function
        let _permit = self.get_permit()?;

        // TODO: Attempt to fetch fresh routes from the control plane
        let retries = 3;
        let retry_delay = Duration::from_secs(5);

        // Testing - load from the backup route provider
        let route_data: RouteData = self.backup_routes.load().unwrap();


        let mut write_guard: parking_lot::lock_api::RwLockWriteGuard<'_, parking_lot::RawRwLock, OrgToCellInner> = self.inner.write();

        write_guard.mapping = route_data.routes;
        write_guard.last_cursor = Some(route_data.last_cursor);


        Ok(())
    }

    /// Load incremental updates from the control plane.
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
            return Err(LoadError {
                message: "Another load operation is in progress".into(),
            });
        }
    }
}
