use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub type CellId = String;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, bincode::Encode, bincode::Decode)]
pub struct Cell {
    pub id: String,
    pub locality: String,
}

impl Cell {
    pub fn new<I, L>(id: I, locality: L) -> Self
    where
        I: Into<String>,
        L: Into<String>,
    {
        Cell {
            id: id.into(),
            locality: locality.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, bincode::Encode, bincode::Decode)]
pub struct RouteData {
    pub id_to_cell: HashMap<String, CellId>,
    pub last_cursor: String,
    pub cells: HashMap<CellId, Arc<Cell>>,
}

impl RouteData {
    pub fn from(
        id_to_cell: HashMap<String, CellId>,
        last_cursor: String,
        cell_to_locality: HashMap<CellId, String>,
    ) -> Self {
        // Construct RouteData from control plane response data
        let cells = cell_to_locality
            .iter()
            .map(|(cell_id, locality)| {
                (
                    cell_id.clone(),
                    Arc::new(Cell {
                        id: cell_id.clone(),
                        locality: locality.clone(),
                    }),
                )
            })
            .collect::<HashMap<CellId, Arc<Cell>>>();

        RouteData {
            id_to_cell,
            last_cursor,
            cells,
        }
    }
}

struct Cursor {
    id: Option<String>,
    last_updated: u64,
}
