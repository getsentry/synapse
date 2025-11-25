# Synapse: Proxy

### Overview
Synapse proxy is a cell-aware reverse proxy that routes incoming HTTP requests to a destination service inside a cell based on the host, URL path and organization.

### Features

- Supports traditional HTTP request/response traffic and long-lived streaming connections via server sent events
- HTTP/1.1 and HTTP/2 for incoming and outgoing connections
- Uses the [`locator`](locator/README.md) internally to resolve organizations to cells

### Organization to cell resolution

The proxy supports two locator modes - `in_process` or `url`.
- In `url` mode, the locator is deployed separately and is called into by the proxy
- In `in_process` mode, the locator is bundled together with the proxy, and can make in-process routing decisions without the overhead of an additional HTTP call


### Deterministic route matching

Routes are matched top down in the order they are defined. For each incoming request, the proxy checks the host and path against the route’s match block, and executes the first action that matches.

**Route matching examples:**

1. Exact path match
    ```yaml
    - match:
        host: null       # host can be anything
        path: /health/   # only the exact path /health/ will be matched
    ```

2. Match any path
    ```yaml
    - match:
        host: de.sentry.io  # match de.sentry.io only
        path: null          # all paths
   ```

3. Path prefix match with dynamic segment
    ```yaml
    - match:
        host: us.sentry.io                                 # matches us.sentry.io only
        path: /organizations/{organization_id_or_slug}/*   # with {organization_id_or_slug} dynamic segment and trailing wildcard
    ```

### Route actions

Each route is associated with an action, which can be a static or dynamic routing rule.


**Route actions examples:**

1. Static proxy to an upstream
    ```yaml
    action:
        to: getsentry-de1-upstream
    ```

2. Call a function to dynamically select an upstream
    ```yaml
    action:
        resolver: cell_from_organization  # resolver function defines how a request is mapped to a cell
        default: us1                              # an optional default cell can be provider
        cell_to_upstream:                         # maps the organization's cell to a specific upstream service (e.g. getsentry, conduit, etc)
            us1: getsentry-us1-upstream
            us2: getsentry-us2-upstream
    ```


### Infrastructure endpoints

Infrastructure endpoints are exposed on a dedicated host/port in order to avoid exposure of admin endpoints to end users, and to prevent collisions with endpoints on proxied services. The host/port can be configured via the `admin_listener` block in the config file.

These include:
- `/health`
- `/ready`
