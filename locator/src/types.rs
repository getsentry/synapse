use std::collections::HashMap;
use std::sync::Arc;

pub type CellId = String;

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
pub struct RouteData {
    pub org_to_cell: HashMap<String, CellId>,
    pub locality_to_default_cell: HashMap<String, CellId>,
    pub last_cursor: String,
    pub cells: HashMap<CellId, Arc<Cell>>,
}
