#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

# Runs mcp-trace-validator for action.yml over the traces the TRACES globs
# name: findings to the log and the job summary, SARIF and JUnit to files, and
# the exit status to the `exit-code` output (the step itself always succeeds,
# so the reports are written either way).
set -uo pipefail
shopt -s globstar nullglob

# Split on whitespace with globbing off, then expand each pattern on its own,
# so a pattern that matches nothing is reported rather than silently dropped.
set -f
patterns=( $TRACES )
set +f
traces=()
for pattern in "${patterns[@]}"; do
  matches=( $pattern )
  if [[ ${#matches[@]} -eq 0 ]]; then
    echo "::error::no trace matches ${pattern}"
    echo "exit-code=2" >> "$GITHUB_OUTPUT"
    exit 0
  fi
  traces+=( "${matches[@]}" )
done

args=()
for revision in ${REVISIONS:-}; do args+=( --revision "$revision" ); done
if [[ "${STRICT:-false}" == "true" ]]; then args+=( --strict ); fi

# The human report, to the log and the job summary; its status is the verdict.
status=0
mcp-trace-validator validate "${args[@]}" "${traces[@]}" > "$RUNNER_TEMP/mcp-conformance.txt" || status=$?
cat "$RUNNER_TEMP/mcp-conformance.txt"
{
  echo "### MCP trace conformance"
  echo
  echo '```text'
  cat "$RUNNER_TEMP/mcp-conformance.txt"
  echo '```'
} >> "$GITHUB_STEP_SUMMARY"

# The machine reports carry the same verdict; their own exit status is the
# same run's, so it is not consulted again.
if [[ -n "${SARIF_FILE:-}" ]]; then
  mcp-trace-validator validate --format sarif "${args[@]}" "${traces[@]}" > "$SARIF_FILE" 2> /dev/null || true
fi
if [[ -n "${JUNIT_FILE:-}" ]]; then
  mcp-trace-validator validate --format junit "${args[@]}" "${traces[@]}" > "$JUNIT_FILE" 2> /dev/null || true
fi
echo "exit-code=${status}" >> "$GITHUB_OUTPUT"
