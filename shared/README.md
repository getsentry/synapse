# Shared
Shared module for common utility functions

### Metrics
Metrics client for sending counters and timings to StatsD. Supports tags and includes a no-op mode for testing or non-prod deployments where we do not collect metrics. The Metrics type is cheap to clone and can be safely
shared across threads.
