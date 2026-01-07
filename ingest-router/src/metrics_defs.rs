use shared::metrics_defs::{MetricDef, MetricType};

pub const REQUEST_DURATION: MetricDef = MetricDef {
    name: "request.duration",
    metric_type: MetricType::Histogram,
    description: "Request duration in seconds. Tagged with status, handler.",
};

pub const REQUESTS_INFLIGHT: MetricDef = MetricDef {
    name: "requests.inflight",
    metric_type: MetricType::Gauge,
    description: "Number of requests currently being processed",
};

pub const ALL_METRICS: &[MetricDef] = &[REQUEST_DURATION, REQUESTS_INFLIGHT];

