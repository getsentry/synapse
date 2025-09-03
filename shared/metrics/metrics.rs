struct Metrics {
    statsd_host: String,
    statsd_port: u16,
}

impl Metrics {
    fn new(statsd_host: String, statsd_port: u16) -> Self {
        Metrics {
            statsd_host,
            statsd_port,
        }
    }

    fn incr(&self, metric: &str) {
        unimplemented!("Incrementing metric: {}", metric);
    }
}
