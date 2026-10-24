"""Fetches URLs for devices in LAN-only mode: GET /fetch?url=<url> returns that URL's body.

    python fetch-proxy.py 8080

Then point a component at it, such as the PATH component's
"source": "http://<this computer>:8080/fetch?url=https://www.panynj.gov/bin/portauthority/ridepath.json".
"""
import sys
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        url = urllib.parse.parse_qs(urllib.parse.urlparse(self.path).query).get("url", [""])[0]
        if not url.startswith(("http://", "https://")):
            self.send_error(400, "pass ?url=http(s)://...")
            return
        try:
            with urllib.request.urlopen(url, timeout=15) as r:
                body, kind = r.read(), r.headers.get("Content-Type", "application/octet-stream")
        except Exception as e:
            self.send_error(502, str(e))
            return
        self.send_response(200)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


ThreadingHTTPServer(("0.0.0.0", int(sys.argv[1]) if len(sys.argv) > 1 else 8080), Handler).serve_forever()
