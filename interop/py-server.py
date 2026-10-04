# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

"""A small server on the official Python SDK, which speaks 2026-07-28.

Usage: python py-server.py            (stdio)
       python py-server.py http PORT  (streamable HTTP on 127.0.0.1:PORT/mcp)
"""

import sys

from mcp.server.mcpserver import MCPServer

app = MCPServer("interop-py-server")


@app.tool()
def add(a: int, b: int) -> int:
    """Add two integers."""
    return a + b


@app.resource("interop://greeting")
def greeting() -> str:
    """A fixed greeting."""
    return "hello"


@app.prompt()
def review(code: str) -> str:
    """Ask for a code review."""
    return f"Please review: {code}"


if __name__ == "__main__":
    if len(sys.argv) > 2 and sys.argv[1] == "http":
        app.run("streamable-http", port=int(sys.argv[2]))
    else:
        app.run()
