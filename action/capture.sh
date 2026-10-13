#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

# Records MCP sessions for action.yml with mcp-trace-capture, driven by the
# workflow's own client command, so the validate step can judge them.
#
#   CAPTURE=stdio  SERVER is the stdio server command. The client is given
#                  MCP_SERVER_COMMAND, the path of an executable that runs
#                  SERVER under `mcp-trace-capture stdio`; it launches that in
#                  place of the server.
#   CAPTURE=http   UPSTREAM is the server's URL. SERVER, when set, is started in
#                  the background first and stopped afterwards. The client is
#                  given MCP_PROXY_URL, the recording proxy's address, which
#                  reaches UPSTREAM itself (any path is appended to its path).
#
# CLIENT and SERVER are bash command lines, run from the workspace. Each session
# is written to CAPTURE_DIR/NNN.jsonl. Outputs: `traces` (the glob the validate
# step reads) and `client-exit-code`. The step itself fails only when the
# recording could not be set up; a failing client is reported through the
# output, so the sessions it did record are still judged and reported.
set -uo pipefail

fail() {
  echo "::error::$1"
  exit 1
}

case "${CAPTURE:-}" in
  stdio | http) ;;
  *) fail "capture must be \`stdio\` or \`http\`, not \`${CAPTURE:-}\`" ;;
esac
[[ -n "${CLIENT:-}" ]] || fail "capture: ${CAPTURE} needs a \`client\` command to drive the session"
if [[ "$CAPTURE" == stdio && -z "${SERVER:-}" ]]; then
  fail "capture: stdio needs a \`server\` command to record"
fi
if [[ "$CAPTURE" == http && -z "${UPSTREAM:-}" ]]; then
  fail "capture: http needs the \`upstream\` URL of the server to record"
fi
case "${CAPTURE_DIR:-}" in
  "" | *[[:space:]*?[]*) fail "capture-dir must be a non-empty path without spaces or glob characters" ;;
esac
capture_bin=$(command -v mcp-trace-capture) || fail "mcp-trace-capture is not on PATH"

# Absolute, because the client may launch the wrapper from anywhere. A
# directory left from an earlier run would be judged with this one.
if [[ -e "$CAPTURE_DIR" ]] && [[ -n "$(ls -A "$CAPTURE_DIR")" ]]; then
  fail "capture-dir ${CAPTURE_DIR} already exists and is not empty"
fi
mkdir -p "$CAPTURE_DIR" || fail "cannot create ${CAPTURE_DIR}"
dir=$(cd "$CAPTURE_DIR" && pwd)
output="${dir}/{session}.jsonl"
workspace=$PWD
state="$RUNNER_TEMP/mcp-conformance-capture"
rm -rf "$state" && mkdir -p "$state/pids"

# Puts each background job in its own process group, so stopping SERVER stops
# whatever its command line started, not just the bash running it.
set -m

# Waits until something accepts TCP connections at host $1 port $2, up to 60 s.
# A bare connect, not an HTTP request, which the server would see as traffic.
wait_for_port() {
  local _
  for _ in $(seq 1 120); do
    if (exec 3<> "/dev/tcp/$1/$2") 2> /dev/null; then return 0; fi
    sleep 0.5
  done
  return 1
}

server_pid=""
stop_server() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2> /dev/null; then
    kill -TERM -- "-$server_pid" 2> /dev/null || kill -TERM "$server_pid" 2> /dev/null
    wait "$server_pid" 2> /dev/null
  fi
  server_pid=""
}
trap stop_server EXIT

client_env=()
proxy_pid=""
if [[ "$CAPTURE" == stdio ]]; then
  # The wrapper carries every value itself, quoted with %q, rather than reading
  # the environment: clients commonly launch stdio servers with a reduced one.
  # It records its PID (which `exec` keeps) so the step can wait for the
  # recording to finish after the client exits.
  wrapper="$state/mcp-server"
  {
    echo '#!/usr/bin/env bash'
    printf 'echo "$$" > %q/"$$"\n' "$state/pids"
    printf 'cd %q || exit 1\n' "$workspace"
    printf 'exec %q -o %q stdio -- bash -c %q\n' "$capture_bin" "$output" "$SERVER"
  } > "$wrapper"
  chmod +x "$wrapper"
  echo "Recording stdio sessions to ${dir}/NNN.jsonl; the client launches ${wrapper}"
  client_env=("MCP_SERVER_COMMAND=$wrapper")
else
  authority=${UPSTREAM#*://}
  authority=${authority%%/*}
  authority=${authority##*@}
  if [[ "$authority" == \[* ]]; then
    host=${authority%%]*}
    host=${host#[}
    port=${authority#*]}
    port=${port#:}
  elif [[ "$authority" == *:* ]]; then
    host=${authority%%:*}
    port=${authority##*:}
  else
    host=$authority
    port=""
  fi
  if [[ -z "$port" ]]; then
    if [[ "$UPSTREAM" == https://* ]]; then port=443; else port=80; fi
  fi
  if [[ -n "${SERVER:-}" ]]; then
    echo "Starting the server: ${SERVER}"
    bash -c "$SERVER" > "$state/server.log" 2>&1 &
    server_pid=$!
    if ! wait_for_port "$host" "$port"; then
      cat "$state/server.log"
      fail "nothing accepted connections at ${host}:${port} within 60 s of starting the server"
    fi
  fi
  "$capture_bin" -o "$output" http --upstream "$UPSTREAM" --listen 127.0.0.1:0 2> "$state/proxy.log" &
  proxy_pid=$!
  address=""
  for _ in $(seq 1 60); do
    address=$(sed -n 's|.*listening on http://\([^ ,]*\).*|\1|p' "$state/proxy.log" | head -n 1)
    [[ -n "$address" ]] && break
    kill -0 "$proxy_pid" 2> /dev/null || break
    sleep 0.5
  done
  if [[ -z "$address" ]]; then
    cat "$state/proxy.log"
    fail "the recording proxy did not start"
  fi
  echo "Recording HTTP sessions to ${dir}/NNN.jsonl through http://${address} (forwarding to ${UPSTREAM})"
  client_env=("MCP_PROXY_URL=http://${address}")
fi

echo "::group::Client: ${CLIENT}"
status=0
env "${client_env[@]}" bash -c "$CLIENT" || status=$?
echo "::endgroup::"
if [[ "$status" -ne 0 ]]; then
  echo "::error::the client exited ${status}"
fi

if [[ -n "$proxy_pid" ]]; then
  # SIGINT is the proxy's graceful stop: it lets requests in flight finish and
  # completes the trace before exiting.
  kill -INT "$proxy_pid" 2> /dev/null
  wait "$proxy_pid"
  proxy_status=$?
  cat "$state/proxy.log"
  if [[ "$proxy_status" -ne 0 ]]; then
    echo "::warning::mcp-trace-capture exited ${proxy_status}; a recording may be incomplete"
  fi
else
  # A client may exit without waiting for its server, and the wrapper writes
  # the trace's last lines as the server goes; wait for every wrapper it ran.
  for pidfile in "$state"/pids/*; do
    [[ -e "$pidfile" ]] || continue
    pid=$(< "$pidfile")
    for _ in $(seq 1 60); do
      kill -0 "$pid" 2> /dev/null || break
      sleep 0.5
    done
    if kill -0 "$pid" 2> /dev/null; then
      echo "::warning::the stdio recording (PID ${pid}) was still running 30 s after the client exited"
    fi
  done
fi
stop_server

shopt -s nullglob
recorded=("$dir"/*.jsonl)
echo "${#recorded[@]} session(s) recorded to ${dir}"
{
  echo "traces=${dir}/*.jsonl"
  echo "client-exit-code=${status}"
} >> "$GITHUB_OUTPUT"
