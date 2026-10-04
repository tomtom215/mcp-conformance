#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

# Cross-SDK interop check: real sessions between the official TypeScript and
# Python SDKs, recorded through mcp-trace-capture and judged by
# mcp-trace-validator. No code from this workspace speaks MCP here — only the
# recorder and the judge — so a pass is evidence about independent traffic.
#
#   ./run.sh [BIN_DIR]    BIN_DIR holds both binaries (default ../target/debug)
#
# Prerequisites: `npm ci` in this directory, and a Python 3.11+ environment with
# `pip install --require-hashes -r requirements.txt` (PYTHON selects it; default
# python3). Traces and reports land in out/.

set -euo pipefail
cd "$(dirname "$0")"
BIN_DIR="$(cd "${1:-../target/debug}" && pwd)"
CAPTURE="$BIN_DIR/mcp-trace-capture"
VALIDATOR="$BIN_DIR/mcp-trace-validator"
PYTHON="${PYTHON:-python3}"
EVERYTHING="node_modules/@modelcontextprotocol/server-everything/dist/index.js"
OUT=out
rm -rf "$OUT" && mkdir -p "$OUT"

failures=0
pids=()
cleanup() { for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done; }
trap cleanup EXIT

# Waits until something accepts TCP connections on PORT, up to 30 s. A bare
# connect, not an HTTP request: a request through the capture proxy would be
# recorded as part of the session it is waiting to start.
wait_for_port() {
  for _ in $(seq 1 60); do
    if (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; then return 0; fi
    sleep 0.5
  done
  echo "nothing listening on port $1 after 30 s" >&2
  return 1
}

# judge NAME EXPECTED_REVISION: the trace must judge `pass` (exit 0) at the
# revision the session declared.
judge() {
  local name=$1 want=$2 status=0
  "$VALIDATOR" validate --format json "$OUT/$name.jsonl" > "$OUT/$name.report.json" || status=$?
  local verdict revision
  verdict=$("$PYTHON" -c 'import json,sys; print(json.load(open(sys.argv[1]))["verdict"])' "$OUT/$name.report.json" 2>/dev/null || echo "none")
  revision=$("$PYTHON" -c 'import json,sys; print(json.load(open(sys.argv[1]))["revision"])' "$OUT/$name.report.json" 2>/dev/null || echo "none")
  if [[ $status -eq 0 && $verdict == pass && $revision == "$want" ]]; then
    printf 'PASS  %-24s revision %s, verdict %s\n' "$name" "$revision" "$verdict"
  else
    printf 'FAIL  %-24s exit %s, revision %s (want %s), verdict %s\n' "$name" "$status" "$revision" "$want" "$verdict"
    "$VALIDATOR" validate "$OUT/$name.jsonl" || true
    failures=$((failures + 1))
  fi
}

# 1. TypeScript client -> TypeScript everything server, stdio.
node ts-client.mjs stdio "$CAPTURE" -o "$OUT/ts-stdio.jsonl" stdio -- node "$EVERYTHING" stdio
judge ts-stdio 2025-11-25

# 2. The same over streamable HTTP, through the proxy.
PORT=38001 node "$EVERYTHING" streamableHttp > "$OUT/ts-server.log" 2>&1 & pids+=($!)
wait_for_port 38001
"$CAPTURE" -o "$OUT/ts-http.jsonl" http --upstream http://127.0.0.1:38001 --listen 127.0.0.1:38101 2> "$OUT/ts-capture.log" & cap=$!; pids+=($cap)
wait_for_port 38101
node ts-client.mjs http http://127.0.0.1:38101/mcp
kill -INT "$cap"; wait "$cap" || true
judge ts-http 2025-11-25

# 3. Python client -> Python server, stdio (2026-07-28, stateless).
"$PYTHON" py-client.py stdio "$CAPTURE" -o "$OUT/py-stdio.jsonl" stdio -- "$PYTHON" py-server.py
judge py-stdio 2026-07-28

# 4. The same over streamable HTTP.
"$PYTHON" py-server.py http 38002 > "$OUT/py-server.log" 2>&1 & pids+=($!)
wait_for_port 38002
"$CAPTURE" -o "$OUT/py-http.jsonl" http --upstream http://127.0.0.1:38002 --listen 127.0.0.1:38102 2> "$OUT/py-capture.log" & cap=$!; pids+=($cap)
wait_for_port 38102
"$PYTHON" py-client.py http http://127.0.0.1:38102/mcp
kill -INT "$cap"; wait "$cap" || true
judge py-http 2026-07-28

# 5. A 2025-06-18 session (an older SDK release) must be refused with exit 2,
#    naming the revision, rather than judged against another revision's rules.
INTEROP_SDK=sdk-2025-06-18 node ts-client.mjs stdio "$CAPTURE" -o "$OUT/ts-2025-06-18.jsonl" stdio -- node "$EVERYTHING" stdio
status=0
"$VALIDATOR" validate "$OUT/ts-2025-06-18.jsonl" 2> "$OUT/ts-2025-06-18.stderr" || status=$?
if [[ $status -eq 2 ]] && grep -q '2025-06-18' "$OUT/ts-2025-06-18.stderr"; then
  printf 'PASS  %-24s refused with exit 2, naming 2025-06-18\n' ts-2025-06-18
else
  printf 'FAIL  %-24s exit %s; stderr:\n' ts-2025-06-18 "$status"
  cat "$OUT/ts-2025-06-18.stderr"
  failures=$((failures + 1))
fi

if [[ $failures -gt 0 ]]; then
  echo "interop: $failures check(s) failed" >&2
  exit 1
fi
echo "interop: all 5 checks passed"
