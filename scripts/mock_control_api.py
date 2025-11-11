# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///

from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import time
import base64
from typing import Optional
from urllib.parse import urlparse, parse_qs
import argparse


# This API mocks the control plane snapshot request, split across pages
# to simulate pagination

DEFAULT_PAGE_SIZE = 10
TOTAL_RESULTS = 15
START_TIME = int(time.time())

Results = list[dict[str, str]]
Cursor = Optional[str]
HasMore = bool


ALL_RESULTS = [
    # org_id, slug, cell, updated_at
    (str(i), f"sentry{i}", f"us{i % 2 + 1}", START_TIME + i)
    for i in range(TOTAL_RESULTS)
]


def get_results(cursor: Optional[str]) -> tuple[Results, Cursor, HasMore]:
    from_idx: Optional[int] = None

    if cursor is None:
        from_idx = 0
    else:
        decoded = json.loads(base64.b64decode(cursor).decode("utf-8"))
        updated_at = decoded["updated_at"]
        org_id = decoded["org_id"]

        for idx, result in enumerate(ALL_RESULTS):
            # cursor matches exactly, start from next result
            if result[3] == updated_at and result[0] == org_id:
                from_idx = idx
                break
            # the cursor doesn't have an org_id
            elif result[3] == updated_at and org_id is None:
                from_idx = idx
                break
            # we passed the cursor and there was no exact match
            elif result[3] > updated_at:
                from_idx = idx
                break

        # We never found a valid from_idx, return no results
        if from_idx is None:
            return [], None, False
        assert from_idx is not None

    to_idx = min(from_idx + DEFAULT_PAGE_SIZE - 1, TOTAL_RESULTS - 1)

    has_more = to_idx < TOTAL_RESULTS - 1

    if has_more:
        next_result = ALL_RESULTS[to_idx + 1]
        next_cursor = make_cursor(next_result[3], next_result[0])
    else:
        next_cursor = make_cursor(to_idx, None)

    results = []
    for i in range(from_idx, to_idx + 1):
        results.append({
            "id": str(i),
            "slug": f"sentry{i}",
            "cell": f"us{i % 2 + 1}",
        })
    return results, next_cursor, has_more


class MockControlApi(BaseHTTPRequestHandler):
    def do_GET(self):
        parsed = urlparse(self.path)
        base_path = parsed.path
        query_params = parse_qs(parsed.query)

        if base_path == "/org-cell-mappings/":
            cursor = query_params.get("cursor", [None])[0]
            (data, next_cursor, has_more) = get_results(cursor)

            response = {
                "data": data,
                "metadata": {
                    "cursor": next_cursor,
                    "has_more": has_more,
                    "cell_to_locality": {
                        "us1": "us",
                        "us2": "us",
                    }
                }
            }

            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(response).encode("utf-8"))
        else:
            # If endpoint doesn't match, return 404
            self.send_response(404)
            self.end_headers()
            self.wfile.write(b"Not Found")


def make_cursor(updated_at: int, org_id: Optional[str]) -> str:
    return base64.b64encode(json.dumps({
        "org_id": org_id,
        "updated_at": updated_at,
    }).encode("utf-8")).decode("utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="mock control plane")
    parser.add_argument("--host")
    parser.add_argument("--port", type=int)
    args = parser.parse_args()

    host = args.host or "127.0.0.1"
    port = args.port or 9000

    server = HTTPServer((host, port), MockControlApi)
    print(f"Mock control plane listening on http://{host}:{port}")
    server.serve_forever()
