use parking_lot::RwLock;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::sync::{mpsc, oneshot};

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

#[derive(Clone)]
pub struct Cell {
    pub id: Arc<String>,
    pub locality: Arc<String>,
}

impl Cell {
    pub fn new<I, L>(id: I, locality: L) -> Self
    where
        I: Into<String>,
        L: Into<String>,
    {
        Cell {
            id: Arc::new(id.into()),
            locality: Arc::new(locality.into()),
        }
    }
}

#[derive(Clone)]
struct Cursor {
    // seconds since 1970-01-01 00:00:00 UTC
    last_updated: u64,
    // None org_id means no more results
    org_id: Option<i64>,
}

enum Command {
    // Trigger incremental mapping refresh outside of the normal internal
    Refresh(oneshot::Sender<Result<()>>),
    // Trigger the loader to shudown gracefully
    Shutdown,
}

#[derive(Clone)]
struct OrgToCellInner {
    mapping: HashMap<String, Cell>,
    locality_to_default_cell: HashMap<String, Cell>,
    last_cursor: Option<Cursor>,
}

#[derive(Clone)]
pub struct OrgToCell {
    inner: Arc<RwLock<OrgToCellInner>>,
    update_lock: Arc<Semaphore>,
    // Used by the healthcheck. Initially false and set to true once any snapshot
    // has been loaded and mappings are available.
    ready: Arc<AtomicBool>,
    // last_update: Arc<RwLock<Option<SystemTime>>>,
}

impl OrgToCell {
    pub fn new() -> Self {
        OrgToCell {
            inner: Arc::new(RwLock::new(OrgToCellInner {
                mapping: HashMap::new(),
                locality_to_default_cell: HashMap::new(),
                last_cursor: None,
            })),
            update_lock: Arc::new(Semaphore::new(1)),
            ready: Arc::new(AtomicBool::new(false)),
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
                if let Some(loc) = locality {
                    if cell.locality.as_str() != loc {
                        return Err(LookupError::new("locality mismatch"));
                    }
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

        // Ok(guard.mapping.get(org_id).cloned().or_else(|| {
        //     guard.locality_to_default_cell.get(locality).cloned()
        // })
    }

    /// Performs an initial full load, then periodically reloads
    /// mappings at the configured interval or on demand when the Refresh
    /// command is received. The loop runs indefinitely until the Shutdown
    /// is received.
    pub async fn run_loader(&self, mut rx: mpsc::Receiver<Command>) {}

    async fn load_placeholder_data(&self) {
        std::thread::sleep(std::time::Duration::from_secs(10)); // fake sleep

        let cells = [
            Cell::new("us1", "us"),
            Cell::new("us2", "us"),
            Cell::new("de", "de"),
        ];

        let mut dummy_data = HashMap::new();
        for i in 0..10 {
            dummy_data.insert(format!("org_{i}"), cells[i % cells.len()].clone());
        }

        let mut write_guard = self.inner.write();
        write_guard.mapping = dummy_data;
        write_guard.last_cursor = Some(Cursor {
            last_updated: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            org_id: None,
        });
    }

    /// Loads the entire mapping in pages from the control plane.
    /// If the control plane is unreachable, fall back to stored local copy.
    async fn load_snapshot(&self) -> Result<(), LoadError> {
        // Hold permit for the duration of this function
        let _permit = self.get_permit()?;

        // Do loading

        Ok(())
    }

    /// Load incremental updates from the control plane.
    pub async fn load_incremental(&self) -> Result<(), LoadError> {
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
