# Synapse: Proxy

### Overview
Synapse proxy is a cell-aware reverse proxy that routes incoming HTTP requests to a destination service inside a cell based on the host, URL path and organization.

### Features

- supports traditional HTTP request/response traffic and long-lived streaming connections via server sent events
- HTTP/1.1 and HTTP/2 for incoming and outgoing connections
- Uses the [`locator`](locator/README.md) internally to resolve organizations to cells

### Organization to cell resolution

The proxy supports two locator modes - `in_process` or `url`.
- In `url` mode, the locator is deployed separately and is called into by the proxy
- In `in_process` mode, the locator is bundled together with the proxy, and can make in-process routing decisions without the overhead of an additional HTTP call


### Deterministic route matching

Routes are matched top down in the order they are defined. For each incoming request, the proxy checks the host, path and method against the route’s match block, and executes the first action that matches all three.

**Route matching examples:**

1. Exact path match
    ```yaml
    - match:
        host: "*"       # host can be anything
        path: /health/  # only the exact path /health/ will be matched
        methods: [GET]  #  only get requests will be matched
    ```

2. Match any path
    ```yaml
    - match:
        host: de.sentry.io  # match de.sentry.io only
        path: "*"           # all paths
        methods: [ALL]      # all methods
    ```

3. Path prefix match with dynamic segment
    ```yaml
    - match:
        host: us.sentry.io                                 # matches us.sentry.io only
        path: /organizations/{organization_id_or_slug}/*   # with {organization_id_or_slug} dynamic segment and trailing wildcard
        methods: [ALL]                                     # all methods
    ```

### Route actions
Each route is associated with an action -- either `handler` or `proxy`.

**Handler actions:**

A handler action returns a built-in response directly from the proxy, without forwarding the request upstream. This can be used to define infrastructure endpoints such as healthchecks and readiness probes.

**Proxy actions:**

A proxy action will proxy the request to one of the upstreams. Upstream selection can be driven by a `resolver` function.

For example, setting `resolver: resolve_cell_from_organization` together with the dynamic path `/organizations/{organization_id_or_slug}/*`, will allow the proxy to extract the organization segment, resolve the cell and route the request to the correct upstream. An optional `default` cell can be configured, which will be used if the request cannot be resolved.
