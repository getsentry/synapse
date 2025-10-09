use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
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
