//! Metrics definitions for the locator.

use shared::metrics_defs::{MetricDef, MetricType};

pub const NEGATIVE_CACHE_HIT: MetricDef = MetricDef {
    name: "negative_cache.hit",
    metric_type: MetricType::Counter,
    description: "Number of lookups that hit the negative cache",
};

pub const NEGATIVE_CACHE_MISS: MetricDef = MetricDef {
    name: "negative_cache.miss",
    metric_type: MetricType::Counter,
    description: "Number of lookups that missed the negative cache",
};

pub const ALL_METRICS: &[MetricDef] = &[NEGATIVE_CACHE_HIT, NEGATIVE_CACHE_MISS];
