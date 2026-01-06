use shared::metrics_defs::{MetricDef, MetricType};

pub const REQUEST_DURATION: MetricDef = MetricDef {
    name: "request.duration",
    metric_type: MetricType::Histogram,
    description: "Proxy request duration in seconds. Tagged with status, upstream. Sampled at 1%.",
};

pub const REQUESTS_INFLIGHT: MetricDef = MetricDef {
    name: "requests.inflight",
    metric_type: MetricType::Gauge,
    description: "Number of requests currently being processed",
};

// TODO: all metrics must be added here for now, this can be done dynamically with a macro in the future.
pub const ALL_METRICS: &[MetricDef] = &[REQUEST_DURATION, REQUESTS_INFLIGHT];
