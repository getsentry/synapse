# Metrics

This document describes all metrics emitted by Synapse. Metrics are exported via StatsD when configured.

## Configuration

Metrics are configured in the YAML config file:

```yaml
metrics:
  statsd_host: "127.0.0.1"
  statsd_port: 8125
```

---

## Locator Metrics

<!-- LOCATOR_METRICS:START -->
| Metric | Type | Description |
|--------|------|-------------|
| `negative_cache.hit` | Counter | Number of lookups that hit the negative cache |
| `negative_cache.miss` | Counter | Number of lookups that missed the negative cache |
<!-- LOCATOR_METRICS:END -->


## TODO: Add metrics for other modules

