# Ingest Router

This is a simple ingest router that can be used to route requests to the appropriate downstream API destination for ingest APIs. *It does not handle ingest traffic. Only the API requests are routed.*

## Requirements

There are two types of requests that are routed:

### Project configs

  This is a POST request to the `/projectconfigs` endpoint. It has low latency and high throughput requirements.

  These requests need to be fan out to all the cells. The response from each cell needs to be aggregated
  and the upstream cell information needs to be added to the response. The sequence diagram below shows the flow for this type of request.

```mermaid
sequenceDiagram
  participant Relay-Pop
  participant Ingest Router
  box Cell 1
  participant Cell-1 Sentry
  participant Cell-1 Processing Relay
  end
  box Cell 2
  participant Cell-2 Sentry
  participant Cell-2 Processing Relay
  end
  Note right of Cell-1 Sentry: Org 1 and 3
  Note right of Cell-2 Sentry: Org 2 and 4
  Relay-Pop->>+Ingest Router: GET /projectconfigs?DSN=[1,2,3,4]
  Ingest Router->>+Cell-1 Sentry: GET /projectconfigs?DSN=[1,2,3,4]
  Cell-1 Sentry->>Cell-1 Sentry: DSN lookup. Only finds DSN 1 and 3
  Cell-1 Sentry->>-Ingest Router: {DSN 1: {project: 10, org_id: 10},<br/>DSN 3: {project: 30, org_id: 30}}
  Ingest Router->>+Cell-2 Sentry: GET /projectconfigs?DSN=[1,2,3,4]
  Cell-2 Sentry->>Cell-2 Sentry: DSN lookup. Only finds DSN 2 and 4
  Cell-2 Sentry->>-Ingest Router: {DSN 2: {project: 20, org_id: 20},<br/>DSN 4: {project: 40, org_id: 40}}
  Ingest Router->>Ingest Router: The response is an indicator of which DSN belongs to which cell
  Ingest Router->>Ingest Router: Aggregate data and add upstream relay information
  Ingest Router->>-Relay-Pop: {DSN 1: {project: 10, org_id: 10, upstream: cell-1 relay},<br/>DSN 3: {project: 30, org_id: 30, upstream: cell-1 relay},<br/>,DSN 2: {project: 20, org_id: 20, upstream: cell-2 relay},<br/>DSN 4: {project: 40, org_id: 40, upstream: cell-2 relay}}
  Relay-Pop->>Cell-1 Processing Relay: POST /envelope {error1, error3}
  Relay-Pop->>Cell-2 Processing Relay: POST /envelope {error2, error4}
```

### Other requests

  These can be either GET or POST requests. They typically do not have high latency and high throughput requirements. For these requests, a configurable authoratative cell needs to be specified. The response from this cell needs to be returned to the client. The sequence diagram below shows the flow for this type of request.

```mermaid
sequenceDiagram
  participant Relay-Pop
  participant Ingest Router
  box Cell 1
  participant Cell-1 Sentry
  participant Cell-1 Processing Relay
  end
  box Cell 2
  participant Cell-2 Sentry
  participant Cell-2 Processing Relay
  end
  Note right of Cell-1 Sentry: Org 1 and 3
  Note right of Cell-2 Sentry: Org 2 and 4
  Relay-Pop->>+Ingest Router: GET /other_request
  Ingest Router->>+Cell-1 Sentry: GET /other_request
  Cell-1 Sentry->>-Ingest Router: {Response}
  Ingest Router->>-Relay-Pop: {Response}
```

## Design

The ingest router is primarily made up of 2 components:

1. A routing engine that matches requests to routes
2. A handler factory that creates handlers from route configurations

The routing engine is responsible for matching requests to routes. It uses a routing table to match requests to routes. The routing table is loaded from a YAML file. The YAML file is parsed and converted into a routing engine routes. An example of a routing engine route is shown below:

```yaml
- name: "single_cell_api"
  match:
    path_prefix: "/api/v1/"
    method: "POST"
  handler: "forward_to_cell"
  config:
    target: "http://us1.sentry.io/api/v1/"
- name: "multi_cell_api"
  match:
    path_prefix: "/api/v1/projectconfigs"
    method: "POST"
  handler: "fan_out_with_merge"
  config:
    targets:
      - "http://us1.sentry.io/api/v1/"
      - "http://us2.sentry.io/api/v1/"
```

The handler factory is responsible for creating handlers from route configurations. It uses the routing engine to match requests to routes and then creates the appropriate handler.
