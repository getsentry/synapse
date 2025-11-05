use cadence::{Counted, MetricError, StatsdClient, Timed, UdpMetricSink};
use std::net::UdpSocket;
use std::sync::Arc;

enum MetricsBackend {
    Statsd(StatsdClient),
    Noop,
}

#[derive(Clone)]
pub struct Metrics {
    backend: Arc<MetricsBackend>,
}

impl Metrics {
    /// Create a new Metrics client that sends to StatsD
    pub fn new(statsd_host: String, statsd_port: u16, prefix: &str) -> Result<Self, MetricError> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_nonblocking(true)?;

        let addr = format!("{}:{}", statsd_host, statsd_port);
        let sink = UdpMetricSink::from(addr, socket)?;
        let client = StatsdClient::from_sink(prefix, sink);

        Ok(Metrics {
            backend: Arc::new(MetricsBackend::Statsd(client)),
        })
    }

    /// Create a no-op Metrics client that discards all metrics
    pub fn new_noop() -> Self {
        Metrics {
            backend: Arc::new(MetricsBackend::Noop),
        }
    }

    /// Increment a counter metric by 1
    /// metrics.incr("http.requests", Some(&[("endpoint", "api"), ("status", "200")]));
    pub fn incr(&self, metric: &str, tags: Option<&[(&str, &str)]>) {
        let client = match self.backend.as_ref() {
            MetricsBackend::Statsd(client) => client,
            MetricsBackend::Noop => return,
        };

        let result = if let Some(tag_list) = tags {
            if !tag_list.is_empty() {
                let mut counter = client.count_with_tags(metric, 1);
                for (key, value) in tag_list {
                    counter = counter.with_tag(key, value);
                }
                counter.try_send()
            } else {
                client.count(metric, 1)
            }
        } else {
            client.count(metric, 1)
        };

        if let Err(e) = result {
            eprintln!("Failed to send metric: {}", e);
        }
    }

    /// Record a timing metric in milliseconds
    /// metrics.timing("http.response_time", 42, Some(&[("endpoint", "api"), ("method", "GET")]));
    pub fn timing(&self, metric: &str, value_ms: u64, tags: Option<&[(&str, &str)]>) {
        let client = match self.backend.as_ref() {
            MetricsBackend::Statsd(client) => client,
            MetricsBackend::Noop => return,
        };

        let result = if let Some(tag_list) = tags {
            if !tag_list.is_empty() {
                let mut timer = client.time_with_tags(metric, value_ms);
                for (key, value) in tag_list {
                    timer = timer.with_tag(key, value);
                }
                timer.try_send()
            } else {
                client.time(metric, value_ms)
            }
        } else {
            client.time(metric, value_ms)
        };

        if let Err(e) = result {
            eprintln!("Failed to send metric: {}", e);
        }
    }
}
