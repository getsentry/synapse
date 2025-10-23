use crate::types::{Cell, RouteData};
use std::sync::Arc;

use crate::backup_routes::{BackupError, BackupRouteProvider};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{AcquireError, mpsc, oneshot};
use tokio::sync::{Semaphore, SemaphorePermit};

struct LocatorInner {
    org_to_cell_map: Arc<OrgToCell>,
    #[allow(dead_code)]
    handle: tokio::task::JoinHandle<()>,
    #[allow(dead_code)]
    tx: mpsc::Sender<Command>,
}

#[derive(Clone)]
pub struct Locator {
    inner: Arc<LocatorInner>,
}

impl Locator {
    pub fn new(
        control_plane_url: String,
        backup_provider: Arc<dyn BackupRouteProvider + 'static>,
    ) -> Self {
        // Channel to send commands to the worker thread.
        let (tx, rx) = mpsc::channel::<Command>(64);

        let org_to_cell_map = Arc::new(OrgToCell::new(control_plane_url, backup_provider));

        // Spawn the loader thread. All loading should happen from this thread.
        let org_to_cell_map_clone = org_to_cell_map.clone();
        let handle = tokio::spawn(async move {
            org_to_cell_map_clone.start(rx).await;
        });

        Locator {
            inner: Arc::new(LocatorInner {
                org_to_cell_map,
                handle,
                tx,
            }),
        }
    }

    pub fn lookup(&self, org_id: &str, locality: Option<&str>) -> Result<Arc<Cell>, LocatorError> {
        self.inner.org_to_cell_map.lookup(org_id, locality)
    }

    pub fn shutdown(&self) {
        unimplemented!();
    }

    pub fn refresh(&self) {
        unimplemented!();
    }

    pub fn is_ready(&self) -> bool {
        self.inner.org_to_cell_map.ready.load(Ordering::Relaxed)
    }
}

#[derive(thiserror::Error, Debug, PartialEq)]
pub enum LocatorError {
    #[error("no cell found for organization")]
    NoCell,

    #[error("requested locality does not match the cell's locality")]
    LocalityMismatch { requested: String, actual: String },

    #[error("the locator is not ready yet")]
    NotReady,

    #[error("internal error")]
    InternalError,
}

#[derive(thiserror::Error, Debug)]
pub enum LoadError {
    #[error("Error loading backup")]
    BackupError(#[from] BackupError),
    #[error("Another load operation is in progress")]
    ConcurrentLoad(#[from] AcquireError),
}

#[derive(Debug)]
pub enum Command {
    // Trigger incremental mapping refresh outside of the normal interval.
    // The worker sends Ok(()) when the refresh attempt finishes.
    Refresh(oneshot::Sender<Result<(), LoadError>>),
    // Trigger the loader to shudown gracefully
    Shutdown,
}

/// Synchronizes the org to cell mappings from the control plane and backup route provider.
/// This struct is used internally by the Locator.
struct OrgToCell {
    control_plane_url: String,
    data: RwLock<RouteData>,
    update_lock: Semaphore,
    // Used by the readiness probe. Initially false and set to true once any snapshot
    // has been loaded and mappings are available.
    ready: AtomicBool,
    // last_update: Arc<RwLock<Option<SystemTime>>>,
    backup_routes: Arc<dyn BackupRouteProvider + Send + Sync>,
}

impl OrgToCell {
    pub fn new(
        control_plane_url: String,
        backup_routes: Arc<dyn BackupRouteProvider + Send + Sync>,
    ) -> Self {
        OrgToCell {
            control_plane_url,
            data: RwLock::new(RouteData {
                org_to_cell: HashMap::new(),
                locality_to_default_cell: HashMap::new(),
                last_cursor: "".into(),
                cells: HashMap::new(),
            }),
            update_lock: Semaphore::new(1),
            ready: AtomicBool::new(false),
            backup_routes,
        }
    }

    pub fn lookup(&self, org_id: &str, locality: Option<&str>) -> Result<Arc<Cell>, LocatorError> {
        // Looks up the cell for a given organization ID and locality.
        // Returns an `Option<Cell>` if found, or `None` if not found.
        // Returns an error if locality is passed and the org_id/locality pair is not valid.
        // Or if a locality is passed but no defualt cell is found for that locality
        if !self.ready.load(Ordering::Relaxed) {
            return Err(LocatorError::NotReady);
        }

        let read_guard = self.data.read();
        let cell_id = read_guard
            .org_to_cell
            .get(org_id)
            .or_else(|| {
                if let Some(loc) = locality {
                    read_guard.locality_to_default_cell.get(loc)
                } else {
                    None
                }
            })
            .ok_or(LocatorError::NoCell)?;

        let cell = read_guard
            .cells
            .get(cell_id)
            .cloned()
            .ok_or(LocatorError::InternalError)?;

        if let Some(requested_locality) = locality
            && cell.locality != requested_locality
        {
            return Err(LocatorError::LocalityMismatch {
                requested: requested_locality.to_string(),
                actual: cell.locality.clone(),
            });
        }

        Ok(cell)
    }

    /// Performs an initial full load, then periodically reloads
    /// mappings at the configured interval or on demand when the Refresh
    /// command is received. The loop runs indefinitely until the Shutdown
    /// command is received.
    pub async fn start(&self, mut rx: mpsc::Receiver<Command>) {
        if let Ok(()) = self.load_snapshot().await {
            self.ready.store(true, Ordering::Relaxed);

            // Once a snapshot is loaded, the worker periodically requests incremental results
            // until the Shutdown command is received.
            // If the Refresh command is received, the incremental load can be triggered ahead
            // of schedule.
            loop {
                if let Some(cmd) = rx.recv().await {
                    println!("Received command {:?}", cmd);
                }
            }
        }
    }

    /// Loads the entire mapping in pages from the control plane.
    /// If the control plane is unreachable, fall back to stored local copy.
    /// This function should attempt to fetch data from the control plane.
    /// Once the configured retries have been exhausted, it will attempt to
    /// load from the backup route provider.
    async fn load_snapshot(&self) -> Result<(), LoadError> {
        // Hold permit for the duration of this function
        let _permit = self.get_permit().await?;

        // TODO: Do snapshot loading

        // Testing - load from the backup route provider
        let route_data: RouteData = self.backup_routes.load()?;

        let mut write_guard: parking_lot::lock_api::RwLockWriteGuard<
            '_,
            parking_lot::RawRwLock,
            RouteData,
        > = self.data.write();

        write_guard.org_to_cell = route_data.org_to_cell;
        write_guard.last_cursor = route_data.last_cursor;
        write_guard.locality_to_default_cell = route_data.locality_to_default_cell;
        write_guard.cells = route_data.cells;

        Ok(())
    }

    /// Load incremental updates from the control plane.
    #[allow(dead_code)]
    async fn load_incremental(&self) -> Result<(), LoadError> {
        // Hold permit for the duration of this function
        let _permit = self.get_permit().await?;

        // TODO: Do incremental loading

        Ok(())
    }

    /// Guard that ensures only one load operation is in progress at a time.
    async fn get_permit(&self) -> Result<SemaphorePermit<'_>, AcquireError> {
        self.update_lock.acquire().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct TestingRouteProvider {}

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
            // Do nothing
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_locator() {
        let locator = Locator::new(
            "localhost:9000".to_string(),
            Arc::new(TestingRouteProvider {}),
        );

        assert_eq!(locator.lookup("org_0", None), Err(LocatorError::NotReady));

        // Sleep because snapshot is loaded asynchronously
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(
            locator.lookup("org_0", Some("us")),
            Ok(Arc::new(Cell {
                id: "us1".into(),
                locality: "us".into()
            }))
        );
        assert_eq!(
            locator.lookup("invalid_org", Some("us")),
            Err(LocatorError::NoCell)
        );
        assert_eq!(
            locator.lookup("org_1", Some("de")),
            Err(LocatorError::LocalityMismatch {
                requested: "de".to_string(),
                actual: "us".to_string()
            })
        );
        assert_eq!(
            locator.lookup("org_2", None),
            Ok(Arc::new(Cell {
                id: "de".into(),
                locality: "de".into()
            }))
        );
    }
}
