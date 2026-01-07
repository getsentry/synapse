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

pub const UPSTREAM_REQUEST_DURATION: MetricDef = MetricDef {
    name: "upstream.request.duration",
    metric_type: MetricType::Histogram,
    description: "Per-cell upstream request duration in seconds. Tagged with cell_id, status (the status-code if sucessful, 'timeout', or 'error').",
};

pub const ALL_METRICS: &[MetricDef] = &[
    REQUEST_DURATION,
    REQUESTS_INFLIGHT,
    UPSTREAM_REQUEST_DURATION,
];
