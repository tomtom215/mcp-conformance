# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

"""A client on the official Python SDK, run against py-server.py.

Usage: python py-client.py stdio <command> [args...]
       python py-client.py http <url>
"""

import asyncio
import sys

from mcp import Client, StdioServerParameters


async def main() -> None:
    mode = sys.argv[1]
    if mode == "http":
        target = sys.argv[2]
    else:
        target = StdioServerParameters(command=sys.argv[2], args=sys.argv[3:])
    async with Client(target) as client:
        await client.list_tools()
        result = await client.call_tool("add", {"a": 2, "b": 3})
        assert not result.is_error, result
        await client.list_resources()
        await client.read_resource("interop://greeting")
        await client.list_prompts()
        await client.get_prompt("review", {"code": "x = 1"})
        # An unknown tool: the server answers with an error result.
        result = await client.call_tool("no-such-tool", {})
        assert result.is_error, result


asyncio.run(main())
