#!/usr/bin/env python3
"""Serve generated API docs for local Pitchfork development."""

from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import os


class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, format, *args):
        return


def main():
    host = os.environ.get("HOST", "0.0.0.0")
    port = int(os.environ.get("PORT", "8765"))
    directory = os.environ.get("MMR_API_DOCS_DIR", "docs/api")
    handler = partial(QuietHandler, directory=directory)
    server = ThreadingHTTPServer((host, port), handler)
    print(f"Serving API docs on http://{host}:{port}/ from {directory}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
