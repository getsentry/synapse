# Synapse: Locator

### Overview
The locator is a lightweight, read-only service that returns the cell for a given identifier: either organization (ID or slug), or project key. It periodically syncs with the authoritative route mappings from the control plane. The service is optimized for low-latency reads, and keeps a copy of all cell routes in memory at all times. Additionally, it stores the last copy of all routes in a durable storage, and is designed to function normally and be safe to restart in the event of control plane unavailability.

### Organization mode vs project key mode
The locator runs in one of two modes: `organization` or `project_key`. This is specified via config for standalone locator, or is passed into the constructor when the locator is initialized for an in-process locator used as a library. The use case for the `organization` mode is the proxy / Sentry API, and the use case for project_key mode is Relay / ingest routing. The locator requests data from one of the two control plane APIs based on which data type is selected. The rest of the locator functionality is identical in both cases.

### Usage examples
There are two ways in which the locator can be used:

1. Independent service

    The locator can be deployed as an independent service serving requests over HTTP:

    ```
    $ curl http://synapse.local/locator?id=1

    {
      "1": "us1"
    }
    ```

2. Bundled with proxy

    The locator can also be bundled together with the proxy module, allowing dynamic proxying decisions to be made in-process in a single container without the need for the HTTP call.

    ```rust
    let locator = synapse::Locator::new("http://control-plane-url", filesystem_route_provider, locality_to_default_cell);
    let cell: Result<Cell, LocatorError> = locator.lookup("1", "us1").await?;
    ```

### Syncing to the control plane

On initial bootstrap, the locator service requests a snapshot of all mappings from the control plane in pages. 

```
$ curl sentry-control.internal/org-cell-mappings?cursor=abcdef&limit=100000

{
   "data": [{"id": 1, "slug": "sentry", "cell": "us1"}, ....],
   "metadata": {
    "cursor": "ghijkl",
    "has_more": true
   }
}
```

The cursor represents a base64-encoded composite sort key comprised of the last updated timestamp the id of the row.
The ID is either the org ID for organization mode, or the project key itself for project key mode.

```python
cursor = {
  "updated_at": 1757030409,
  "id": "999"
}

>>> base64.b64encode(json.dumps(cursor).encode('utf-8'))

b'eyJ1cGRhdGVkX2F0IjogMTc1NzAzMDQwOSwgImlkIjogIjk5OSJ9'
```

When there are no more pages to return, the cursor contains a `null` value in the ID field, so incremental polling can pick up from this point.

```python
cursor = {
  "updated_at": 1757030409,
  "id": None
}
```

Getsentry requirement:
- This requires the control plane database to have a new `date_updated` column. The column must be indexed.
- The organization ID is already the primary key in the `organizationmapping` table.

The locator service also periodically requests incremental mapping updates from the control plane, by requesting updates that occured after a specific timestamp.
The incremental API is called periodically, as well as on demand if a cache miss occurs.
```
$ curl sentry-control.sentry.internal/org-cell-mappings?after=1757030409
```

### Backup route store
The locator is designed to continue to serve routes in the event of control plane unavailability. It achieves this by periodically flushing a copy of the id -> cell mappings to an alternate storage. If the control plane is unavailable, this fallback copy is loaded instead.

Configuring backup route storage is mandatory and there are currently two variants included:

1. filesystem: the minimal set up option. it can be run locally and in many other environments.

2. google cloud storage: provided to simplify scaling and deployment by removing the need for persistent local disk/statefulsets. designed for gcp deployments.
