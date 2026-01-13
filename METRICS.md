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
| `control_plane.sync.duration` | Histogram | Time to complete a control plane sync in seconds |
| `control_plane.sync.rows` | Histogram | Number of mappings returned from control plane sync |
<!-- LOCATOR_METRICS:END -->


## Proxy Metrics

<!-- PROXY_METRICS:START -->
| Metric | Type | Description |
|--------|------|-------------|
| `request.duration` | Histogram | Proxy request duration in seconds. Tagged with status, upstream. Sampled at 1%. |
| `requests.inflight` | Gauge | Number of requests currently being processed. |
<!-- PROXY_METRICS:END -->

## Ingest Router Metrics

<!-- INGEST_ROUTER_METRICS:START -->
| Metric | Type | Description |
|--------|------|-------------|
| `request.duration` | Histogram | Request duration in seconds. Tagged with status, handler. |
| `requests.inflight` | Gauge | Number of requests currently being processed |
| `upstream.request.duration` | Histogram | Per-cell upstream request duration in seconds. Tagged with cell_id, status (the status-code if successful, 'timeout', or 'error'). |
<!-- INGEST_ROUTER_METRICS:END -->
