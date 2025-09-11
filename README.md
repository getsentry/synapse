# Synapse

Synapse is a set of services that supports routing for the cell-based architecture at Sentry. Its primary responsibility is to ensure incoming internet traffic is correctly routed to the right cell based on the organization routing key.



### Components

Synapse consists of 3 main components:


- **Locator**:  A read-only service that maintains the mapping of organizations to cells and synchronizes data with the control plane. The locator can operate either as a standalone service that exposes a HTTP API, or as an embedded component inside the proxy layer to make in-process decisions directly. See the [locator docs](locator/README.md).

- **Proxy**: A transparent, high-performance L7 proxy that routes incoming requests to the destination service inside a cell. It uses the locator internally to dynamically resolve the cell based on the organization. The proxy supports both traditional HTTP request/response traffic and long-lived streaming connections via server sent events. See the [proxy docs](proxy/README.md)

**Ingest Router**: TODO: write summary. See the [ingest-router docs](ingest-router/README.md)

