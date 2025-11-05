// Lightweight negative cache which temporarily stores not found results in order to
// prevent repeated lookups for missing keys.
use moka::sync::Cache;
use shared::metrics::Metrics;
use std::time::Duration;

const SIZE: u64 = 1000;
const TTL_SECS: u64 = 5;

pub struct NegativeCache {
    cache: Cache<String, ()>,
    metrics: Metrics,
}

impl NegativeCache {
    pub fn new(metrics: Metrics) -> Self {
        let cache = Cache::builder()
            .max_capacity(SIZE)
            .time_to_live(Duration::from_secs(TTL_SECS))
            .build();

        NegativeCache { cache, metrics }
    }
    pub fn insert(&self, key: &str) {
        self.cache.insert(key.to_string(), ());
    }

    pub fn contains(&self, key: &str) -> bool {
        let cache_hit = self.cache.contains_key(key);
        let metric_name_ = if cache_hit {
            "negative_cache.hit"
        } else {
            "negative_cache.miss"
        };
        self.metrics.incr(metric_name_, None);
        cache_hit
    }
}
