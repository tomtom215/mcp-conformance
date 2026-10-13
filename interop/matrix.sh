#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

# The cross-SDK matrix: each official SDK's own conformance server, recorded
# through mcp-trace-capture's HTTP proxy while an official SDK client drives
# it, then judged by mcp-trace-validator. Two clients, one per revision:
#
#   2025-11-25  ts-client.mjs (TypeScript SDK), the handshake era
#   2026-07-28  py-client.py (Python SDK), the stateless era
#
# The same client drives all four servers at a revision, so the difference
# between two rows of the matrix is the server and nothing else.
#
#   ./matrix.sh [BIN_DIR]   BIN_DIR holds both binaries (default ../target/debug)
#
# Run ./setup.sh first. Traces and JSON reports land in out-matrix/. A finding
# is not a failure of this script — the matrix reports findings, it does not
# hide them; `cargo xtask sdk-matrix --recaptured interop/out-matrix` compares
# the run with the committed corpus. The script fails only when a session
# could not be recorded at all.

set -euo pipefail
cd "$(dirname "$0")"
BIN_DIR="$(cd "${1:-../target/debug}" && pwd)"
CAPTURE="$BIN_DIR/mcp-trace-capture"
VALIDATOR="$BIN_DIR/mcp-trace-validator"
PYTHON="${PYTHON:-.venv/bin/python}"
OUT=out-matrix
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

# ANCHOR: record
# record NAME REVISION PORT -- SERVER...
#
# Starts SERVER (listening on 127.0.0.1:PORT), puts the capture proxy in
# front of it on PORT+100, drives one session through the proxy with the
# client for REVISION, stops the proxy (which writes the trace), and judges
# the trace. The ports are fixed rather than ephemeral because the proxy
# records the Host header, and a committed trace should not change with them.
record() {
  local name=$1 revision=$2 port=$3 proxy=$(($3 + 100)) status=0
  shift 4
  "$@" > "$OUT/$name.server.log" 2>&1 & local server=$!
  pids+=("$server")
  wait_for_port "$port"
  "$CAPTURE" -o "$OUT/$name.jsonl" http \
    --upstream "http://127.0.0.1:$port" --listen "127.0.0.1:$proxy" \
    2> "$OUT/$name.capture.log" & local capture=$!
  pids+=("$capture")
  wait_for_port "$proxy"
  export INTEROP_ERRORS=record
  case $revision in
    2025-11-25) node ts-client.mjs http "http://127.0.0.1:$proxy/mcp" || status=$? ;;
    2026-07-28) "$PYTHON" py-client.py http "http://127.0.0.1:$proxy/mcp" || status=$? ;;
  esac
  kill -INT "$capture"; wait "$capture" || true
  kill "$server" 2>/dev/null || true; wait "$server" 2>/dev/null || true
  judge "$name" "$revision" "$status"
}
# ANCHOR_END: record

# judge NAME REVISION CLIENT_STATUS: validate the trace and print one line.
# The validator exits 1 when it has findings, which is a result, not an
# error; anything else it says, or a client that failed, is.
judge() {
  local name=$1 revision=$2 client=$3 status=0
  if [[ $client -eq 3 ]]; then
    # py-client.py's "the server negotiated an older revision": this server
    # does not serve 2026-07-28, which is a cell of the matrix, not an error.
    mv "$OUT/$name.jsonl" "$OUT/$name.not-served.jsonl"
    printf 'NOT SERVED  %-28s the server does not serve %s\n' "$name" "$revision"
    return
  fi
  if [[ $client -ne 0 ]]; then
    printf 'ERROR       %-28s the client exited %s\n' "$name" "$client"
    failures=$((failures + 1))
    return
  fi
  "$VALIDATOR" validate --format json "$OUT/$name.jsonl" > "$OUT/$name.report.json" || status=$?
  if [[ $status -gt 1 ]]; then
    printf 'ERROR       %-28s the validator exited %s\n' "$name" "$status"
    "$VALIDATOR" validate "$OUT/$name.jsonl" || true
    failures=$((failures + 1))
    return
  fi
  local judged
  judged=$("$PYTHON" -c 'import json,sys; print(json.load(open(sys.argv[1]))["revision"])' "$OUT/$name.report.json")
  if [[ $judged != "$revision" ]]; then
    printf 'ERROR       %-28s judged at %s, not %s\n' "$name" "$judged" "$revision"
    failures=$((failures + 1))
    return
  fi
  printf 'JUDGED      %-28s %s; %s\n' "$name" \
    "$("$VALIDATOR" validate "$OUT/$name.jsonl" | grep -m1 '^verdict:')" \
    "$("$VALIDATOR" validate "$OUT/$name.jsonl" | grep -m1 '^totals:')"
}

# ANCHOR: typescript
record sdk-typescript-2025-11-25 2025-11-25 38301 -- env PORT=38301 node_modules/.bin/tsx upstream/ts-everything-server.ts
record sdk-typescript-2026-07-28 2026-07-28 38301 -- env PORT=38301 node_modules/.bin/tsx upstream/ts-everything-server.ts
# ANCHOR_END: typescript
# ANCHOR: python
record sdk-python-2025-11-25 2025-11-25 38302 -- "$PYTHON" upstream/py_everything_server.py --port 38302
record sdk-python-2026-07-28 2026-07-28 38302 -- "$PYTHON" upstream/py_everything_server.py --port 38302
# ANCHOR_END: python
# ANCHOR: go
# Stateful for the handshake era and stateless for 2026-07-28, as the Go
# SDK's own conformance workflow runs it for each revision.
record sdk-go-2025-11-25 2025-11-25 38303 -- upstream/bin/go-everything-server -http=127.0.0.1:38303 -stateless=false
record sdk-go-2026-07-28 2026-07-28 38303 -- upstream/bin/go-everything-server -http=127.0.0.1:38303 -stateless
# ANCHOR_END: go
# ANCHOR: rust
record sdk-rust-2025-11-25 2025-11-25 38304 -- env PORT=38304 upstream/bin/rust-conformance-server
record sdk-rust-2026-07-28 2026-07-28 38304 -- env PORT=38304 upstream/bin/rust-conformance-server
# ANCHOR_END: rust

if [[ $failures -gt 0 ]]; then
  echo "matrix: $failures session(s) could not be recorded" >&2
  exit 1
fi
echo "matrix: every session recorded and judged; reports in $OUT/"
