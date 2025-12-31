// Lightweight negative cache which temporarily stores not found results in order to
// prevent repeated lookups for missing keys.
use moka::sync::Cache;
use std::time::Duration;
use crate::metrics_defs::{NEGATIVE_CACHE_HIT, NEGATIVE_CACHE_MISS};
use shared::counter;

const SIZE: u64 = 1000;
const TTL_SECS: u64 = 5;

pub struct NegativeCache {
    cache: Cache<String, ()>,
}

impl NegativeCache {
    pub fn new() -> Self {
        let cache = Cache::builder()
            .max_capacity(SIZE)
            .time_to_live(Duration::from_secs(TTL_SECS))
            .build();

        NegativeCache { cache }
    }
    pub fn insert(&self, key: &str) {
        self.cache.insert(key.to_string(), ());
    }

    pub fn contains(&self, key: &str) -> bool {
        let cache_hit = self.cache.contains_key(key);
        let metric_def = if cache_hit {
            NEGATIVE_CACHE_HIT
        } else {
            NEGATIVE_CACHE_MISS
        };
        counter!(metric_def).increment(1);
        cache_hit
    }
}
