# Synapse: Locator

### Overview
The locator is a lightweight, read-only service that returns the cell for a given organization by either ID or slug. It periodically syncs with the authoritative route mappings from the control plane. The service is optimized for low-latency reads, and keeps a copy of all cell routes in memory at all times. Additionally, it stores the last copy of all routes in a durable storage, and is designed to function normally and be safe to restart in the event of control plane unavailability.

### Usage examples
There are two ways in which the locator can be used:

1. Independent service

    The locator can be deployed as an independent service serving requests over HTTP:

    ```
    $ curl http://synapse.local/locator?org=1

    {
      "1": "us1"
    }
    ```

2. Bundled with proxy

    The locator can also be bundled together with the proxy module, allowing dynamic proxying decisions to be made in-process in a single container without the need for the HTTP call.

    ```rust
    let cell: Option<Cell> = synapse::locator::get("1").await?;
    ```

### Syncing to the control plane

On initial bootstrap, the locator service requests a snapshot of all mappings from the control plane in pages. 

```
$ curl sentry-control.internal/org-cell-mappings?cursor=abcdef&limit=100000

# headers
# Link: <https://internal-proxy.local/org-cell-mappings?cursor=abcdef&limit=100000; rel="next",    
# body
{
   "mappings": [{"id": 1, "slug": "sentry", "cell": "us1"}, ....],
   "metadata": {
    "last_timestamp": 1757030409
   }
}
```

The cursor here represents a base64-encoded composite sort key comprised of the update date and the organization ID of the row.

```python
cursor = {
  "updated_at": 1757030409,
  "id": "999"
}

>>> base64.b64encode(json.dumps(cursor).encode('utf-8'))

b'eyJ1cGRhdGVkX2F0IjogMTc1NzAzMDQwOSwgImlkIjogIjk5OSJ9'
```

When there are no more pages to return, the cursor contains a sentinel value in the ID field, so incremental polling can pick up from this point.

```python
cursor = {
  "updated_at": 1757030409,
  "id": SENTINEL_VALUE
}
```




The locator service also periodically requests incremental mapping updates from the control plane, by requesting updates that occured after a specific timestamp.
The incremental API is called periodically, as well as on demand if a cache miss occurs.
```
$ curl sentry-control.sentry.internal/org-cell-mappings?after=1757030409
```

The locator service also periodically flushes a copy of the mapping to a local storage. If the control plane is unavailable, this fallback copy is loaded instead.

