#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

# Runs `mcp-conformance check` for action.yml: exercises the server CHECK (a
# bash command line, a stdio server) or CHECK_URL (a Streamable HTTP endpoint)
# as a conforming client would, and records the session for the validate step,
# which judges it and writes the reports. This step's own report is discarded:
# one judge, one set of files.
#
# What the run exercised (and what it left alone, and why) goes to the log and
# the job summary. A run that could not hold a session leaves no trace; the
# `problem` output then tells the validate step what to report.
set -uo pipefail

if [[ -n "${CHECK:-}" && -n "${CHECK_URL:-}" ]]; then
  echo "::error::set one of \`check\` (a stdio server's command) and \`check-url\` (an HTTP endpoint), not both"
  echo "problem=both check and check-url were set" >> "$GITHUB_OUTPUT"
  exit 0
fi

dir="${CAPTURE_DIR:-mcp-conformance-capture}"
mkdir -p "$dir"
trace="$dir/check.jsonl"
rm -f "$trace"

# Extra flags, split on whitespace with globbing off (`--call-all`,
# `--spec-version 2025-11-25`, `--call my_tool`, a bound).
set -f
# shellcheck disable=SC2206 # word splitting is the point; globbing is off
options=( ${CHECK_OPTIONS:-} )
set +f

if [[ -n "${CHECK_URL:-}" ]]; then
  subject=( --url "$CHECK_URL" )
else
  # Through bash, as the `server` input is: the command line may use
  # variables, quoting and `&&` the way a workflow author expects.
  subject=( -- bash -c "$CHECK" )
fi

status=0
mcp-conformance check "${options[@]}" -o "$trace" --force "${subject[@]}" \
  > /dev/null 2> "$RUNNER_TEMP/mcp-conformance-check.txt" || status=$?
cat "$RUNNER_TEMP/mcp-conformance-check.txt" >&2
{
  echo "### MCP server check"
  echo
  echo '```text'
  cat "$RUNNER_TEMP/mcp-conformance-check.txt"
  echo '```'
} >> "$GITHUB_STEP_SUMMARY"

if [[ ! -s "$trace" || "$status" -eq 2 || "$status" -eq 130 ]]; then
  # The last line the command addressed to its user: its first is the summary's heading.
  problem=$(grep '^mcp-conformance check: ' "$RUNNER_TEMP/mcp-conformance-check.txt" | tail -n 1)
  problem=${problem#mcp-conformance check: }
  problem=${problem:-mcp-conformance check exited ${status} without a session to judge}
  echo "::error::${problem}"
  echo "problem=${problem}" >> "$GITHUB_OUTPUT"
  exit 0
fi
echo "traces=${trace}" >> "$GITHUB_OUTPUT"
