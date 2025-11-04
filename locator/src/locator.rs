use crate::control_plane::ControlPlane;
use crate::types::RouteData;
use std::sync::Arc;
use std::time::Instant;

use crate::backup_routes::{BackupError, BackupRouteProvider};
use crate::negative_cache::NegativeCache;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{AcquireError, mpsc, oneshot};
use tokio::sync::{Semaphore, SemaphorePermit};

struct LocatorInner {
    org_to_cell_map: Arc<OrgToCell>,
    #[allow(dead_code)]
    handle: tokio::task::JoinHandle<()>,
}

#[derive(Clone)]
pub struct Locator {
    inner: Arc<LocatorInner>,
}

impl Locator {
    pub fn new(
        control_plane_url: String,
        backup_provider: Arc<dyn BackupRouteProvider + 'static>,
        locality_to_default_cell: Option<HashMap<String, String>>,
    ) -> Self {
        // Channel to send commands to the worker thread.
        let (tx, rx) = mpsc::channel::<Command>(64);

        let org_to_cell_map = Arc::new(OrgToCell::new(
            control_plane_url,
            backup_provider,
            locality_to_default_cell,
            tx.clone(),
        ));

        // Spawn the loader thread. All loading should happen from this thread.
        let org_to_cell_map_clone = org_to_cell_map.clone();
        let handle = tokio::spawn(async move {
            if let Err(err) = org_to_cell_map_clone.start(rx).await {
                eprintln!("Failed to start locator: {err:?}. Exiting process.");
                std::process::exit(1);
            }
        });

        Locator {
            inner: Arc::new(LocatorInner {
                org_to_cell_map,
                handle,
            }),
        }
    }

    pub async fn lookup(
        &self,
        org_id: &str,
        locality: Option<&str>,
    ) -> Result<String, LocatorError> {
        self.inner.org_to_cell_map.lookup(org_id, locality).await
    }

    pub fn shutdown(&self) {
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
    #[error("Control plane error: {0}")]
    ControlPlaneError(#[from] crate::control_plane::ControlPlaneError),
}

#[derive(Debug)]
pub enum Command {
    // Trigger incremental mapping refresh outside of the normal interval.
    // The request timestamp ensures data is not older than the timestamp provided - min_refresh_interval.
    // The worker sends Ok(()) when the refresh attempt finishes.
    Refresh(Instant, oneshot::Sender<Result<(), LoadError>>),
    // Trigger the loader to shudown gracefully
    Shutdown,
}

struct RouteDataWithTimestamp {
    data: RouteData,
    last_updated: Option<Instant>,
}

/// Synchronizes the org to cell mappings from the control plane and backup route provider.
/// This struct is used internally by the Locator.
struct OrgToCell {
    control_plane: ControlPlane,
    locality_to_default_cell: HashMap<String, String>,
    data: RwLock<RouteDataWithTimestamp>,
    // Keeps track of recently failed lookups to avoid repeated queries against
    // non-existent or recently deleted organizations from adding load to the system.
    negative_cache: NegativeCache,
    update_lock: Semaphore,
    // Used by the readiness probe. Initially false and set to true once any snapshot
    // has been loaded and mappings are available.
    ready: AtomicBool,
    backup_routes: Arc<dyn BackupRouteProvider + Send + Sync>,
    // Standard interval between refresh attempts.
    refresh_interval: std::time::Duration,
    // Minimum duration between refresh attempts.
    min_refresh_interval: std::time::Duration,
    // Channel to send commands to the loader task.
    tx: mpsc::Sender<Command>,
}

impl OrgToCell {
    pub fn new(
        control_plane_url: String,
        backup_routes: Arc<dyn BackupRouteProvider + Send + Sync>,
        locality_to_default_cell: Option<HashMap<String, String>>,
        tx: mpsc::Sender<Command>,
    ) -> Self {
        let data = RouteDataWithTimestamp {
            data: RouteData {
                org_to_cell: HashMap::new(),
                last_cursor: "".into(),
                cells: HashMap::new(),
            },
            last_updated: None,
        };

        OrgToCell {
            control_plane: ControlPlane::new(control_plane_url),
            locality_to_default_cell: locality_to_default_cell.unwrap_or_default(),
            data: RwLock::new(data),
            negative_cache: NegativeCache::new(),
            update_lock: Semaphore::new(1),
            ready: AtomicBool::new(false),
            backup_routes,
            refresh_interval: std::time::Duration::from_secs(60),
            min_refresh_interval: std::time::Duration::from_secs(1),
            tx,
        }
    }

    pub async fn lookup(
        &self,
        org_id: &str,
        locality: Option<&str>,
    ) -> Result<String, LocatorError> {
        // Looks up the cell for a given organization ID and locality.
        // Returns `Ok(Cell)` if found, or a default applies.
        // Returns an error if locality is passed and the org_id/locality pair is not valid.
        // Or if a locality is passed but no defualt cell is found for that locality
        if !self.ready.load(Ordering::Relaxed) {
            return Err(LocatorError::NotReady);
        }

        let start_lookup = Instant::now();

        // Fetch cell and immediately release read lock
        let cell = {
            let read_guard = self.data.read();
            read_guard
                .data
                .org_to_cell
                .get(org_id)
                .and_then(|cell_id| read_guard.data.cells.get(cell_id).cloned())
        };

        // Check the negative cache and possibly refresh data from control plane
        let cell = if cell.is_none() {
            if self.negative_cache.contains(org_id) {
                None
            } else {
                let (ack_tx, ack_rx) = oneshot::channel::<Result<(), LoadError>>();

                match self.tx.try_send(Command::Refresh(start_lookup, ack_tx)) {
                    Ok(()) => {
                        if let Err(err) = ack_rx.await {
                            eprintln!("recv error: {:?}", err);
                        }

                        // Re-acquire the read lock
                        let read_guard = self.data.read();
                        let res = read_guard
                            .data
                            .org_to_cell
                            .get(org_id)
                            .and_then(|cell_id| read_guard.data.cells.get(cell_id).cloned());

                        // Record still not found after refresh, add to negative cache
                        if res.is_none() {
                            self.negative_cache.insert(org_id);
                        }

                        res
                    }
                    Err(e) => {
                        // channel is closed or full
                        eprintln!("channel error: {:?}", e);
                        None
                    }
                }
            }
        } else {
            cell
        }
        // If no cell is found, apply locality default
        .or_else(|| {
            if let Some(loc) = locality {
                self.locality_to_default_cell
                    .get(loc)
                    .and_then(|default_cell_id| {
                        let read_guard = self.data.read();
                        read_guard.data.cells.get(default_cell_id).cloned()
                    })
            } else {
                None
            }
        })
        .ok_or(LocatorError::NoCell)?;

        if let Some(requested_locality) = locality
            && cell.locality != requested_locality
        {
            return Err(LocatorError::LocalityMismatch {
                requested: requested_locality.to_string(),
                actual: cell.locality.clone(),
            });
        }

        Ok(cell.id.clone())
    }

    /// Performs an initial full load, then periodically reloads
    /// mappings at the configured interval or on demand when the Refresh
    /// command is received. The loop runs indefinitely until the Shutdown
    /// command is received.
    pub async fn start(&self, mut rx: mpsc::Receiver<Command>) -> Result<(), LoadError> {
        self.load_snapshot().await?;
        self.ready.store(true, Ordering::Relaxed);

        // Once a snapshot is loaded, the worker periodically requests incremental results
        // until the Shutdown command is received.
        // If the Refresh command is received, the incremental load can be triggered ahead
        // of schedule.
        loop {
            tokio::select! {
                _ = tokio::time::sleep(self.refresh_interval) => {
                    let _ = self.load_incremental().await;
                }
                Some(cmd) = rx.recv() => {
                    match cmd {
                        Command::Refresh(requested_at, tx) => {
                            // Immediately send response if data is up to date, otherwise load incremental updates
                            if let Some(updated) = self.data.read().last_updated && updated + self.min_refresh_interval >= requested_at {
                                let _ = tx.send(Ok(()));
                            } else {
                                let _ = tx.send(self.load_incremental().await);
                            }
                        }
                        Command::Shutdown => {
                            // Mark the locator as no longer ready and exit the loop
                            self.ready.store(false, Ordering::Relaxed);
                            break;
                        },
                    }
                }
            }
        }

        Ok(())
    }

    /// Loads the entire mapping in pages from the control plane.
    /// If the control plane is unreachable, fall back to stored local copy.
    /// This function should attempt to fetch data from the control plane.
    /// Once the configured retries have been exhausted, it will attempt to
    /// load from the backup route provider.
    async fn load_snapshot(&self) -> Result<(), LoadError> {
        let mut snapshot_requested_time: Option<Instant> = Some(Instant::now());

        // Hold permit for the duration of this function
        let _permit = self.get_permit().await?;

        // Fetch data from the control plane. If unavailable fallback to the backup route provider.
        let route_data = self
            .control_plane
            .load_mappings(None)
            .await
            .or_else(|err| {
                eprintln!(
                    "Error loading from control plane: {err:?}, falling back to backup route provider"
                );

                snapshot_requested_time = None;

                // Load from the backup route provider
                self.backup_routes.load()
            })?;

        let mut write_guard: parking_lot::lock_api::RwLockWriteGuard<
            '_,
            parking_lot::RawRwLock,
            RouteDataWithTimestamp,
        > = self.data.write();

        write_guard.data.org_to_cell = route_data.org_to_cell;
        write_guard.data.last_cursor = route_data.last_cursor;
        write_guard.data.cells = route_data.cells;
        write_guard.last_updated = snapshot_requested_time;

        Ok(())
    }

    /// Load incremental updates from the control plane.
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
    use crate::backup_routes::FilesystemRouteProvider;
    use crate::testutils::TestControlPlaneServer;
    use std::time::Duration;

    fn get_mock_provider() -> (tempfile::TempDir, FilesystemRouteProvider) {
        let route_data = RouteData::from(
            HashMap::from([
                ("org_0".into(), "us1".into()),
                ("org_1".into(), "us1".into()),
                ("org_2".into(), "de".into()),
            ]),
            "cursor1".into(),
            HashMap::from([("us1".into(), "us".into()), ("de".into(), "de".into())]),
        );

        let dir = tempfile::tempdir().unwrap();
        let provider = FilesystemRouteProvider::new(dir.path().to_str().unwrap(), "backup.bin");
        provider.store(&route_data).unwrap();
        (dir, provider)
    }

    #[tokio::test]
    async fn test_locator_control_plane_available() {
        // Control plane available, use results from control plane
        let host = "127.0.0.1";
        let port = 9001;

        // Run the control plane server
        let _server = TestControlPlaneServer::spawn(host, port).unwrap();
        std::thread::sleep(Duration::from_millis(300));

        let (_dir, provider) = get_mock_provider();

        let locator = Locator::new(
            format!("http://{host}:{port}").to_string(),
            Arc::new(provider),
            Some(HashMap::from([("de".into(), "de".into())])),
        );

        assert_eq!(
            locator.lookup("org_0", None).await,
            Err(LocatorError::NotReady)
        );

        // Wait for control plane
        tokio::time::sleep(Duration::from_millis(100)).await;

        // org "0" is in the control plane
        assert_eq!(locator.lookup("0", Some("us")).await, Ok("us1".into()));

        // org_0 errors because it's not in the control plane data, only in the backup provider
        assert_eq!(
            locator.lookup("org_0", Some("us")).await,
            Err(LocatorError::NoCell)
        );
    }

    #[tokio::test]
    async fn test_locator_control_plane_unavailable() {
        // Control plane unavailable, load from backup provider

        let (_dir, provider) = get_mock_provider();

        let locator = Locator::new(
            "http://invalid-control-plane:9000".to_string(),
            Arc::new(provider),
            Some(HashMap::from([("de".into(), "de".into())])),
        );

        assert_eq!(
            locator.lookup("org_0", None).await,
            Err(LocatorError::NotReady)
        );

        // Sleep because of retries
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(
            locator.lookup("0", Some("us")).await,
            Err(LocatorError::NoCell)
        );

        // Valid org and locality
        assert_eq!(locator.lookup("org_0", Some("us")).await, Ok("us1".into()));

        // Invalid org, no default
        assert_eq!(
            locator.lookup("invalid_org", Some("us")).await,
            Err(LocatorError::NoCell)
        );

        // Wrong locality requested
        assert_eq!(
            locator.lookup("org_1", Some("de")).await,
            Err(LocatorError::LocalityMismatch {
                requested: "de".to_string(),
                actual: "us".to_string()
            })
        );

        // Valid org, no locality
        assert_eq!(locator.lookup("org_2", None).await, Ok("de".into()));

        // Default cell is used when org_id is not found
        assert_eq!(
            locator.lookup("invalid_org", Some("de")).await,
            Ok("de".into())
        );

        // No default cell for locality returns error
        assert_eq!(
            locator.lookup("invalid_org", Some("us")).await,
            Err(LocatorError::NoCell)
        );
    }
}
