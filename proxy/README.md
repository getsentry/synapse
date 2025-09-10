# Synapse Proxy Service

A high-performance HTTP proxy service that routes requests to the correct getsentry cell based on organization
context. Part of the greater overall Synapse service. This proxy is specifically for API traffic.

## Overview

The Synapse proxy service is one of two core components in the Synapse system (alongside the locator service). It serves as the routing layer that:

- Routes external API requests from locality-specific endpoints (e.g. `us.sentry.io`) to the correct getsentry cell
- Integrates with the locator service to resolve organizations to cells dynamically

**Note**: This service is NOT involved in routing requests between services inside a cell, or for communication between control silo and cell.

## Architecture

### Core Components

- **RulesEngine** (`src/rules_engine.rs`): Main routing logic that matches incoming requests against configured routes
- **Config** (`src/config.rs`): Configuration structure and YAML parsing
- **proxy** (`src/proxy.rs`): resolves to a route

### Routing Logic

The proxy evaluates routes in order and returns the first matching destination:

1. **Host Matching**: Exact match against the request host
2. **Path Pattern Matching**: Prefix matching against configured patterns (optional)
3. **Route Resolution**: 
   - Static routing to predetermined cells
   - Dynamic resolution via resolver functions that call the locator service
   - Fallback to default destinations when resolution fails

## Configuration

### Example Configuration

```yaml
routes:
  - match:
      host: us.sentry.io
      path_prefix_pattern: /organizations/{organization_id_or_slug}/
    route:
      dynamic_to: resolve_cell_from_organization
      default: us
  - match:
      host: us.sentry.io
      path_prefix_pattern: /cell/{cell_id}/
    route:
      dynamic_to: resolve_cell_from_id
  - match:
      host: de.sentry.io
    route:
      to: de
```

## Connection Pooling and Queue Management

Future improvements can include some form of connection pooler? 
```rust
pub struct ConnectionPool {
    // Current connections by destination
    active_connections: Arc<Mutex<HashMap<String, Vec<Connection>>>>,
    // Connection limits per destination
    per_destination_limits: HashMap<String, usize>,
    // Global connection limit
    global_limit: usize,
    // Connection health checker
    health_checker: HealthChecker,
    // Connection recycling configuration
    max_idle_time: Duration,
    keep_alive_timeout: Duration,
}
```

