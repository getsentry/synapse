# Synapse: Locator

### Overview
The locator is a lightweight, read-only service that returns the cell for a given organization (by ID or slug).
It periodically syncs with the authoritative route mappings from the control plane.

The service is optimized for low-latency reads, and keeps a copy of all cell routes in memory at all times.
Additionally, it stores the last durable local copy of all routes, and is designed to function normally in the event of control plane unavailability.

### Usage examples
The locator can run in two modes:


It can be deployed as an independent service and requests made over HTTP:

```
$ curl http://synapse.local/locator?org=1

{
  "1": "us1"
}
```

It can also be bundled together with the proxy module, allowing dynamic proxying decisions to be made in-process in a single container without the need for a HTTP call.


### Syncing to the control plane

On initial bootstrap, the locator service requests a snapshot of all mappings from the control plane in pages. 

```
$ curl sentry-control.internal/org-cell-mappings?cursor=org12345&limit=100000

# headers
# Link: <https://internal-proxy.local/org-cell-mappings?cursor=org12345&limit=100000; rel="next",    
# body
{
   "mappings": [{"id": 1, "slug": "sentry", "cell": "us1"}, ....]
}
```

The locator service also periodically requests incremental mapping updates from the control plane.
The incremental API is called periodically, as well as on demand if a cache miss occurs.

```
$ curl sentry-control.sentry.internal/org-cell-mappings?after=1752621331
```

The locator service also periodically flushes a copy of the mapping to a local storage. If the control plane is unavailable, this fallback copy is loaded instead.

