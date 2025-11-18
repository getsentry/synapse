use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use std::str::FromStr;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Cursor {
    // seconds since 1970-01-01 00:00:00 UTC
    pub updated_at: u64,
    // None org_id means no more results. The last cursor is always none.
    pub id: Option<String>,
}

impl FromStr for Cursor {
    type Err = CursorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let decoded = STANDARD.decode(s.as_bytes())?;
        let cursor: Cursor = serde_json::from_slice(&decoded)?;
        Ok(cursor)
    }
}

impl PartialOrd for Cursor {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Cursor {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Compare by updated_at first, then by id
        match self.updated_at.cmp(&other.updated_at) {
            std::cmp::Ordering::Equal => self.id.cmp(&other.id),
            ordering => ordering,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CursorError {
    #[error("Invalid base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("Invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_parse() {
        // Create a cursor object and encode it
        let cursor_data = serde_json::json!({
            "updated_at": 1757030409,
            "id": null
        });
        let json_str = cursor_data.to_string();
        let encoded = STANDARD.encode(json_str.as_bytes());

        // Parse it back and verify
        let cursor: Cursor = encoded.parse().unwrap();
        assert_eq!(cursor.updated_at, 1757030409);
        assert_eq!(cursor.id, None);
    }

    #[test]
    fn test_invalid_base64() {
        let result: Result<Cursor, _> = "invalid".parse();
        assert!(matches!(result, Err(CursorError::Base64(_))));
    }

    #[test]
    fn test_cursor_comparison() {
        let cursor1 = Cursor {
            updated_at: 1000,
            id: Some("b".to_string()),
        };
        let cursor2 = Cursor {
            updated_at: 2000,
            id: Some("a".to_string()),
        };

        // Ordering is by updated at first and id second
        assert!(cursor1 < cursor2);

        // None is less than some
        let cursor1 = Cursor {
            updated_at: 1000,
            id: None,
        };
        let cursor2 = Cursor {
            updated_at: 1000,
            id: Some("a".to_string()),
        };

        assert!(cursor1 < cursor2);
    }
}
