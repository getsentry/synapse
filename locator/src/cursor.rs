#[derive(Clone)]
pub struct Cursor {
    // seconds since 1970-01-01 00:00:00 UTC
    last_updated: u64,
    // None org_id means no more results
    org_id: Option<i64>,
}

impl TryFrom<String> for Cursor {
    type Error = Box<dyn std::error::Error>;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        unimplemented!();
    }
}