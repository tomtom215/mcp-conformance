# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

"""A client on the official Python SDK that exercises the common surface of
whichever server it reaches, the Python counterpart of ts-client.mjs.

Usage: python py-client.py stdio <command> [args...]
       python py-client.py http <url>

It connects in the SDK's default mode, which probes `server/discover` and
falls back to the `initialize` handshake on a server that predates
`2026-07-28`. A session that fell back is not the session this client exists
to record, so it exits 3 without driving it: matrix.sh reads that as "this
server does not serve 2026-07-28". Any other failure exits non-zero too —
unless INTEROP_ERRORS=record, which matrix.sh sets: there the session is the
evidence, so an error answer is logged and the session carries on.
"""

import asyncio
import os
import sys

from mcp import Client, StdioServerParameters
from mcp.shared.exceptions import MCPError

REVISION = "2026-07-28"
NOT_SERVED = 3
RECORD_ERRORS = os.environ.get("INTEROP_ERRORS") == "record"


async def call(what, awaitable):
    """One advertised call: its result, or, when errors are being recorded,
    the error logged and None returned."""
    try:
        return await awaitable
    except Exception as error:  # noqa: BLE001 - any answer is evidence here
        if not RECORD_ERRORS:
            raise
        print(f"py-client: {what} answered with an error: {error}", file=sys.stderr)
        return None


def prompt_arguments(prompt) -> dict[str, str]:
    """A value for every required argument; the optional ones are left out."""
    return {argument.name: "x" for argument in prompt.arguments or [] if argument.required}


async def main() -> int:
    mode = sys.argv[1]
    if mode == "http":
        target = sys.argv[2]
    else:
        target = StdioServerParameters(command=sys.argv[2], args=sys.argv[3:])
    async with Client(target) as client:
        if client.protocol_version != REVISION:
            print(f"py-client: the server negotiated {client.protocol_version}, not {REVISION}", file=sys.stderr)
            return NOT_SERVED
        caps = client.server_capabilities
        if caps.tools is not None:
            listed = await call("tools/list", client.list_tools())
            names = {tool.name for tool in listed.tools} if listed else set()
            if "add" in names:
                result = await call("tools/call add", client.call_tool("add", {"a": 2, "b": 3}))
                assert RECORD_ERRORS or not result.is_error, result
            if "echo" in names:
                result = await call("tools/call echo", client.call_tool("echo", {"message": "interop"}))
                assert RECORD_ERRORS or not result.is_error, result
            # An unknown tool: the server answers with an error the trace must carry.
            # Either a protocol error or an error result is a correct answer.
            try:
                result = await client.call_tool("no-such-tool", {})
            except MCPError:
                pass
            else:
                assert result.is_error, result
        if caps.resources is not None:
            listed = await call("resources/list", client.list_resources())
            if listed and listed.resources:
                await call("resources/read", client.read_resource(str(listed.resources[0].uri)))
        if caps.prompts is not None:
            listed = await call("prompts/list", client.list_prompts())
            if listed and listed.prompts:
                first = listed.prompts[0]
                await call("prompts/get", client.get_prompt(first.name, prompt_arguments(first)))
    return 0


sys.exit(asyncio.run(main()))
