# Ingest Router

This is a simple ingest router that can be used to route requests to the appropriate downstream API destination for ingest APIs. *It does not handle ingest traffic. Only the API requests are routed.*

## Requirements

There are different types of requests that are routed. Each type of request has different requirements and constraints.

### Fetching project configs and global configs

  This is a POST request to the `/api/0/relays/projectconfigs/` endpoint. It has low latency and high throughput requirements. The request body might contain global config requests as well as project config requests.

  These requests need to be fan out to all the cells to fetch the project configs of public keys which may be in different cells. The response from each cell needs to be aggregated (the `configs` field) and the upstream cell information needs to be added to the response. The sequence diagram below shows the flow for this type of request.

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

### Endpoints needing clarification

```
POST /api/0/relays/register/challenge/
POST /api/0/relays/register/response/
POST /api/0/relays/publickeys/
GET /api/0/relays/ - This seems to be called from frontend. Need not be handled by the ingest router.
POST /api/0/relays/projectconfigs/ - This is fetching project ids from public keys. Might be similar to the project configs endpoint.
```
