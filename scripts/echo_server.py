from http.server import BaseHTTPRequestHandler, HTTPServer


class EchoHandler(BaseHTTPRequestHandler):
    HOP_BY_HOP = {
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    }

    def _handle(self):
        length = int(self.headers.get("content-length", 0) or 0)
        body = self.rfile.read(length) if length else b""

        print(f"\n{self.command} {self.path}")
        for k, v in self.headers.items():
            print(f"{k}: {v}")
        if body:
            print("\n" + body.decode(errors="replace"))

        self.send_response(200)
        for k, v in self.headers.items():
            if k.lower() not in self.HOP_BY_HOP:
                self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body)

    # map all methods to one handler
    do_GET = do_POST = do_PUT = do_DELETE = do_PATCH = do_OPTIONS = _handle


if __name__ == "__main__":
    server = HTTPServer(("127.0.0.1", 9000), EchoHandler)
    print("Echo server listening on http://127.0.0.1:9000")
    server.serve_forever()
