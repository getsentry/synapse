//! Common types for metrics definitions.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
}

impl MetricType {
    pub const fn as_str(&self) -> &'static str {
        match self {
            MetricType::Counter => "Counter",
            MetricType::Gauge => "Gauge",
            MetricType::Histogram => "Histogram",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MetricDef {
    pub name: &'static str,
    pub metric_type: MetricType,
    pub description: &'static str,
}

#[macro_export]
macro_rules! counter {
    ($def:expr) => {
        metrics::counter!($def.name)
    };
}

#[macro_export]
macro_rules! gauge {
    ($def:expr) => {
        metrics::gauge!($def.name)
    };
}

#[macro_export]
macro_rules! histogram {
    ($def:expr) => {
        metrics::histogram!($def.name)
    };
}
