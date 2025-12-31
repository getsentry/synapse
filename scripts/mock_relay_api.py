# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///
"""
Mock Relay Endpoint for Integration Testing
"""

import json
import uuid
from datetime import datetime, timezone
from typing import Dict, Any, List, Optional
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import urlparse, parse_qs

# Default mock data - easily customizable
DEFAULT_MOCK_DATA = {
    "valid_keys": {
        "abc123def456": {"project_id": 100, "project_slug": "test-project", "org_id": 1},
        "a" * 32: {"project_id": 100, "project_slug": "test-project", "org_id": 1},
        "xyz789uvw012": {"project_id": 200, "project_slug": "another-project", "org_id": 2},
    },
    "inactive_keys": {
        "disabled123": {"project_id": 100, "project_slug": "test-project", "org_id": 1},
    },
    "disabled_projects": {
        "proj_disabled": {"project_id": 300, "project_slug": "disabled-project", "org_id": 1},
    }
}

class MockRelayData:
    """Simple data store for mock relay configurations"""

    def __init__(self, data: Optional[Dict] = None):
        self.data = data or DEFAULT_MOCK_DATA.copy()

    def add_valid_key(self, public_key: str, project_id: int, project_slug: str, org_id: int = 1):
        """Add a valid project key"""
        self.data["valid_keys"][public_key] = {
            "project_id": project_id,
            "project_slug": project_slug,
            "org_id": org_id
        }

    def add_inactive_key(self, public_key: str, project_id: int, project_slug: str, org_id: int = 1):
        """Add an inactive project key"""
        self.data["inactive_keys"][public_key] = {
            "project_id": project_id,
            "project_slug": project_slug,
            "org_id": org_id
        }

    def is_valid_key(self, public_key: str) -> bool:
        """Check if a key is valid and active"""
        return public_key in self.data["valid_keys"]

    def get_key_info(self, public_key: str) -> Optional[Dict]:
        """Get key information if it exists"""
        if public_key in self.data["valid_keys"]:
            return self.data["valid_keys"][public_key]
        elif public_key in self.data["inactive_keys"]:
            return self.data["inactive_keys"][public_key]
        elif public_key in self.data["disabled_projects"]:
            return self.data["disabled_projects"][public_key]
        return None

def generate_project_config(key_info: Dict, public_key: str) -> Dict[str, Any]:
    """Generate a project configuration for a valid key"""
    now = datetime.now(timezone.utc).isoformat()

    return {
        "disabled": False,
        "slug": key_info["project_slug"],
        "lastFetch": now,
        "lastChange": now,
        "rev": uuid.uuid4().hex,
        "publicKeys": [{
            "publicKey": public_key,
            "numericId": hash(public_key) % 1000000,
            "isEnabled": True,
        }],
        "config": {
            "allowedDomains": ["*"],
            "trustedRelays": [],
            "piiConfig": None,
            "datascrubbingSettings": {
                "scrubData": True,
                "scrubDefaults": True,
                "scrubIpAddresses": False,
            },
            "features": [],
        },
        "organizationId": key_info["org_id"],
        "projectId": key_info["project_id"],
    }

def process_relay_request(public_keys: List[str], mock_data: MockRelayData) -> Dict[str, Dict[str, Any]]:
    """
    Process relay config request - mimics Sentry's exact behavior:
    1. Start with all keys disabled
    2. Enable only valid, active keys
    """
    configs = {}

    # Initialize all requested keys as disabled
    for public_key in public_keys:
        configs[public_key] = {"disabled": True}

    # Enable valid keys with full configuration
    for public_key in public_keys:
        if mock_data.is_valid_key(public_key):
            key_info = mock_data.get_key_info(public_key)
            configs[public_key] = generate_project_config(key_info, public_key)

    return configs


class MockRelayHandler(BaseHTTPRequestHandler):
    """HTTP request handler for mock relay endpoint"""
    
    # Class variable to hold mock data (can be set before creating server)
    mock_data: Optional[MockRelayData] = None
    
    def _read_json_body(self) -> dict:
        """Read and parse JSON body from request"""
        content_length = int(self.headers.get('Content-Length', 0))
        body = self.rfile.read(content_length) if content_length > 0 else b'{}'
        return json.loads(body)
    
    def _send_json_response(self, status_code: int, data: dict):
        """Send JSON response"""
        self.send_response(status_code)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps(data).encode('utf-8'))
    
    def _send_error_response(self, status_code: int, message: str):
        """Send error response"""
        self._send_json_response(status_code, {"detail": message})
    
    def do_POST(self):
        """Handle POST requests"""
        parsed_path = urlparse(self.path)
        
        # Relay project configs endpoint
        if parsed_path.path == "/api/0/relays/projectconfigs/":
            try:
                request_data = self._read_json_body()
                query_params = parse_qs(parsed_path.query)
                version = query_params.get("version", ["3"])[0]
                public_keys = request_data.get("publicKeys", [])
                
                # Process the request
                configs = process_relay_request(public_keys, self.mock_data)
                response = {"configs": configs}
                
                # Add global config for version 3
                if version == "3" and request_data.get("global"):
                    response["global"] = {
                        "measurements": {
                            "builtinMeasurements": [
                                {"name": "fcp", "unit": "millisecond"},
                                {"name": "lcp", "unit": "millisecond"},
                            ]
                        }
                    }
                    response["global_status"] = "ready"
                
                self._send_json_response(200, response)
                
            except json.JSONDecodeError:
                self._send_error_response(400, "Invalid JSON")
            except Exception as e:
                self._send_error_response(500, str(e))
        else:
            self._send_error_response(404, "Not Found")
    
    def do_GET(self):
        """Handle GET requests"""
        parsed_path = urlparse(self.path)

        # Relay health check endpoint
        if parsed_path.path == "/api/0/relays/live/":
            self._send_json_response(200, {"is_healthy": True})
        else:
            self._send_error_response(404, "Not Found")
    
    def log_message(self, format, *args):
        """Override to customize logging"""
        print(f"{self.address_string()} - {format % args}")

if __name__ == "__main__":
    print("🚀 Mock Relay Endpoint for Integration Testing")
    print("=" * 45)
    print("Server: http://localhost:8000")
    print()
    print("Pre-configured test keys:")

    mock_data = MockRelayData()
    for key in mock_data.data["valid_keys"]:
        info = mock_data.data["valid_keys"][key]
        print(f"  ✅ {key} -> {info['project_slug']} (active)")

    for key in mock_data.data["inactive_keys"]:
        info = mock_data.data["inactive_keys"][key]
        print(f"  ❌ {key} -> {info['project_slug']} (inactive)")

    print()
    print("Test command:")
    print('curl -X POST "http://localhost:8000/api/0/relays/projectconfigs/" \\')
    print('  -H "Content-Type: application/json" \\')
    print('  -d \'{"publicKeys": ["abc123def456", "disabled123", "nonexistent"]}\'')
    print()
    print("Expected response:")
    print('{"configs": {')
    print('  "abc123def456": {"disabled": false, "slug": "test-project", ...},')
    print('  "disabled123": {"disabled": true},')
    print('  "nonexistent": {"disabled": true}')
    print('}}')
    print()
    print("Starting server...")
    
    # Set the mock data on the handler class
    MockRelayHandler.mock_data = mock_data
    
    # Create and run the server
    server = HTTPServer(("0.0.0.0", 8000), MockRelayHandler)
    
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n\n👋 Shutting down server...")
        server.shutdown()
