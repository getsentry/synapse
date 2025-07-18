use std::{collections::HashMap, sync::RwLock};
use std::sync::Arc;
use std::time::SystemTime;
use std::fmt;


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
struct OrgToCellInner {
    mapping: HashMap<String, Cell>,
    locality_to_default_cell: HashMap<String, Cell>,
    last_updated: Option<SystemTime>,
}


#[derive(Clone)]
pub struct OrgToCell {
    inner: Arc<RwLock<OrgToCellInner>>,
}

impl OrgToCell {
    pub fn new() -> Self {
        OrgToCell {
            inner: Arc::new(RwLock::new(OrgToCellInner {
                mapping: HashMap::new(),
                locality_to_default_cell: HashMap::new(),
                last_updated: None,
            })),
        }
    }

    pub fn lookup(&self, org_id: &str, locality: Option<&str>) -> Result<Option<Cell>, LookupError> {
        // Looks up the cell for a given organization ID and locality.
        // Returns an `Option<Cell>` if found, or `None` if not found.
        // Returns an error if locality is passed and the org_id/locality pair is not valid.
        // Or if a locality is passed but no defualt cell is found for that locality
        let guard = self.inner.read().unwrap();

        let cell = guard.mapping.get(org_id);

        match cell {
            Some(cell) => {
                if let Some(loc) = locality {
                    if cell.locality.as_str() != loc {
                        return Err(LookupError::new(&format!("locality mismatch")));
                    }
                }
                return Ok(Some(cell.clone()));

            }
            None => {
                if let Some(locality) = locality {
                    if let Some(default_cell) = guard.locality_to_default_cell.get(locality) {
                        return Ok(Some(default_cell.clone()));
                    } else {
                        return Err(LookupError::new(&format!("No cell found for org_id '{}' and locality '{}'", org_id, locality)));
                    }
                } else {
                    return Ok(None);
                }
            }
        }

        // Ok(guard.mapping.get(org_id).cloned().or_else(|| {
        //     guard.locality_to_default_cell.get(locality).cloned()
        // })
    }

    pub fn load_placeholder_data(&self) {
        std::thread::sleep(std::time::Duration::from_secs(10)); // fake sleep

        let cells = vec![
            Cell::new("us1", "us"),
            Cell::new("us2", "us"),
            Cell::new("de", "de"),
        ];

        let mut dummy_data = HashMap::new();
        for i in 0..10 {
            dummy_data.insert(format!("org_{}", i), cells[i % cells.len()].clone());
        }


        let mut guard = self.inner.write().unwrap();
        guard.mapping = dummy_data;
        guard.last_updated = Some(SystemTime::now());
    }


}
