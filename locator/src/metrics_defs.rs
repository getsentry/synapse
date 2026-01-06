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

pub const CONTROL_PLANE_SYNC_DURATION: MetricDef = MetricDef {
    name: "control_plane.sync.duration",
    metric_type: MetricType::Histogram,
    description: "Time to complete a control plane sync in seconds",
};

pub const CONTROL_PLANE_SYNC_ROWS: MetricDef = MetricDef {
    name: "control_plane.sync.rows",
    metric_type: MetricType::Histogram,
    description: "Number of mappings returned from control plane sync",
};

// TODO: all metrics must be added here for now, this can be done dynamically with a macro in the future.
pub const ALL_METRICS: &[MetricDef] = &[
    NEGATIVE_CACHE_HIT,
    NEGATIVE_CACHE_MISS,
    CONTROL_PLANE_SYNC_DURATION,
    CONTROL_PLANE_SYNC_ROWS,
];
