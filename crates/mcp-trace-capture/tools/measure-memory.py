#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)
"""Peak resident memory of mcp-trace-capture per recorded message.

The README's "Writing a message costs far more than its size" figures come
from this script. Method: one message of SIZE bytes and a given shape is sent
once through (a) the HTTP proxy, in front of a loopback upstream that reads the
body and answers a small JSON, and (b) the stdio wrapper around `cat`, which
echoes it. VmHWM (the process's peak resident set, from /proc/<pid>/status, so
Linux only) is read after the response (or the echo) has arrived in full and
before the process is stopped. Loopback only; nothing is left behind but the
two trace files in the scratch directory given, overwritten each run.

usage: measure-memory.py BINARY SCRATCH_DIR SHAPE RUNS [SIZE_MIB]
  SHAPE: numbers (an array of one-digit numbers) | string (one long string
         member) | object (short members, `"k0000000":0`)
e.g. python3 -I tools/measure-memory.py target/release/mcp-trace-capture /tmp/m numbers 3
"""

import http.client
import http.server
import os
import re
import signal
import subprocess
import sys
import threading
import time


def payload(shape: str, size: int) -> bytes:
    if shape == "numbers":
        # [1,2,3,...,9,1,...]: one digit and a comma per element.
        n = (size - 2 + 1) // 2
        body = "[" + ",".join(str((i % 9) + 1) for i in range(n)) + "]"
    elif shape == "string":
        head, tail = '{"blob":"', '"}'
        body = head + "x" * (size - len(head) - len(tail)) + tail
    elif shape == "object":
        # {"k0000000":0,"k0000001":0,...}: 13 bytes per member.
        n = (size - 2) // 13
        body = "{" + ",".join(f'"k{i:07d}":0' for i in range(n)) + "}"
    else:
        raise SystemExit(f"unknown shape {shape}")
    return body.encode()


def vmhwm_kib(pid: int) -> int:
    with open(f"/proc/{pid}/status") as status:
        for line in status:
            if line.startswith("VmHWM:"):
                return int(line.split()[1])
    raise SystemExit("no VmHWM in /proc/<pid>/status")


class Upstream(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_POST(self):  # noqa: N802
        remaining = int(self.headers.get("content-length", "0"))
        while remaining:
            chunk = self.rfile.read(min(remaining, 1 << 20))
            if not chunk:
                break
            remaining -= len(chunk)
        answer = b'{"jsonrpc":"2.0","id":1,"result":{}}'
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(answer)))
        self.end_headers()
        self.wfile.write(answer)

    def log_message(self, *_):
        pass


def start_upstream() -> int:
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Upstream)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server.server_address[1]


def measure_http(binary: str, scratch: str, body: bytes, upstream_port: int) -> int:
    trace = os.path.join(scratch, "http.jsonl")
    proxy = subprocess.Popen(
        [binary, "-o", trace, "--force", "http", "--upstream",
         f"http://127.0.0.1:{upstream_port}", "--listen", "127.0.0.1:0"],
        stderr=subprocess.PIPE, text=True,
    )
    address = None
    for line in proxy.stderr:
        found = re.search(r"listening on http://([^,]+),", line)
        if found:
            address = found.group(1)
            break
    assert address, "the proxy announced no address"
    # Keep draining stderr so the proxy never blocks on it.
    threading.Thread(target=lambda: proxy.stderr.read(), daemon=True).start()
    connection = http.client.HTTPConnection(address, timeout=120)
    connection.request("POST", "/mcp", body=body,
                       headers={"content-type": "application/json"})
    response = connection.getresponse()
    response.read()
    assert response.status == 200, response.status
    connection.close()
    time.sleep(0.2)
    peak = vmhwm_kib(proxy.pid)
    proxy.send_signal(signal.SIGINT)
    proxy.wait(timeout=30)
    return peak


def measure_stdio(binary: str, scratch: str, body: bytes) -> int:
    trace = os.path.join(scratch, "stdio.jsonl")
    wrapper = subprocess.Popen(
        [binary, "-o", trace, "--force", "stdio", "--", "cat"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
    )
    line = body + b"\n"
    writer = threading.Thread(
        target=lambda: (wrapper.stdin.write(line), wrapper.stdin.flush()))
    writer.start()
    received = 0
    while received < len(line):
        chunk = wrapper.stdout.read(min(len(line) - received, 1 << 20))
        if not chunk:
            break
        received += len(chunk)
    assert received == len(line), received
    writer.join()
    time.sleep(0.2)
    peak = vmhwm_kib(wrapper.pid)
    wrapper.stdin.close()
    wrapper.wait(timeout=30)
    return peak


def main() -> None:
    binary, scratch, shape, runs = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
    size = int(sys.argv[5]) << 20 if len(sys.argv) > 5 else 16 << 20
    os.makedirs(scratch, exist_ok=True)
    body = payload(shape, size)
    mib = len(body) / (1 << 20)
    print(f"shape={shape} bytes={len(body)} ({mib:.2f} MiB) binary={binary}")
    port = start_upstream()
    for mode, measure in (("http", lambda: measure_http(binary, scratch, body, port)),
                          ("stdio", lambda: measure_stdio(binary, scratch, body))):
        peaks = [measure() / 1024 for _ in range(runs)]
        spread = max(peaks) - min(peaks)
        print(f"  {mode:5} VmHWM MiB: " + ", ".join(f"{p:.1f}" for p in peaks)
              + f"  (spread {spread:.1f}; x{max(peaks) / mib:.1f} of the message at the peak)")


if __name__ == "__main__":
    main()
