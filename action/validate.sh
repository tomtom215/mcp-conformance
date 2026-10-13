#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

# Runs mcp-trace-validator for action.yml over the traces the TRACES globs
# name and the sessions the capture step recorded (CAPTURED): findings to the
# log and the job summary, SARIF and JUnit to files, and the exit status to the
# `exit-code` output (the step itself always succeeds, so the reports are
# written either way).
#
# "Either way" includes a run the validator cannot start — a pattern matching
# no trace, or a lone trace it cannot read, for which it writes no document.
# Each configured report is then a SARIF run with `executionSuccessful: false`
# or a JUnit `<error>` naming the problem — the shapes the validator itself
# gives a trace it could not judge — so a following `upload-sarif` or
# test-report step (usually `if: always()`) reads a valid file rather than
# failing on a missing or empty one. The exit-code output stays the run's own.
set -uo pipefail
shopt -s globstar nullglob

# $1 with the characters JSON strings escape replaced.
json_escape() {
  local text=$1
  text=${text//\\/\\\\}
  text=${text//\"/\\\"}
  text=${text//$'\n'/\\n}
  text=${text//$'\r'/\\r}
  text=${text//$'\t'/\\t}
  printf '%s' "$text"
}

# $1 with the characters XML attributes escape replaced.
xml_escape() {
  local text=$1
  text=${text//&/&amp;}
  text=${text//</&lt;}
  text=${text//>/&gt;}
  text=${text//\"/&quot;}
  text=${text//$'\n'/&#10;}
  printf '%s' "$text"
}

# Writes the SARIF log $1 for a run that judged nothing because of problem $2.
write_unjudged_sarif() {
  local file=$1 problem=$2 version
  version=$(mcp-trace-validator --version 2> /dev/null | head -n 1)
  version=${version##* }
  cat > "$file" << EOF
{
  "\$schema": "https://json.schemastore.org/sarif-2.1.0.json",
  "version": "2.1.0",
  "runs": [
    {
      "tool": {
        "driver": {
          "name": "mcp-trace-validator",
          "informationUri": "https://github.com/tomtom215/mcp-conformance",
          "semanticVersion": "$(json_escape "${version:-unknown}")",
          "rules": []
        }
      },
      "invocations": [
        {
          "executionSuccessful": false,
          "toolExecutionNotifications": [
            { "level": "error", "message": { "text": "$(json_escape "$problem")" } }
          ]
        }
      ],
      "results": []
    }
  ]
}
EOF
}

# Writes the JUnit report $1 for a run that judged nothing because of problem $2.
write_unjudged_junit() {
  local file=$1 problem=$2
  cat > "$file" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<testsuites tests="1" failures="0" errors="1" skipped="0">
  <testsuite name="mcp-trace-validator" tests="1" failures="0" errors="1" skipped="0">
    <testcase classname="mcp-trace-validator" name="traces could not be judged"><error message="$(xml_escape "$problem")"/></testcase>
  </testsuite>
</testsuites>
EOF
}

# Writes each configured report for a run that judged nothing, because of $1.
write_unjudged_reports() {
  if [[ -n "${SARIF_FILE:-}" ]]; then write_unjudged_sarif "$SARIF_FILE" "$1"; fi
  if [[ -n "${JUNIT_FILE:-}" ]]; then write_unjudged_junit "$JUNIT_FILE" "$1"; fi
}

# Split on whitespace with globbing off, then expand each pattern on its own,
# so a pattern that matches nothing is reported rather than silently dropped.
# CAPTURED is the capture step's glob, judged beside any `traces` given.
set -f
# shellcheck disable=SC2206 # word splitting is the point; globbing is off
patterns=( ${TRACES:-} ${CAPTURED:-} )
set +f
if [[ ${#patterns[@]} -eq 0 ]]; then
  echo "::error::nothing to validate: set \`traces\`, or \`capture\` to record a session"
  write_unjudged_reports "nothing to validate: no traces given and nothing captured"
  echo "exit-code=2" >> "$GITHUB_OUTPUT"
  exit 0
fi
traces=()
for pattern in "${patterns[@]}"; do
  # shellcheck disable=SC2206 # the glob expansion is the point
  matches=( $pattern )
  if [[ ${#matches[@]} -eq 0 ]]; then
    echo "::error::no trace matches ${pattern}"
    write_unjudged_reports "no trace matches ${pattern}"
    echo "exit-code=2" >> "$GITHUB_OUTPUT"
    exit 0
  fi
  traces+=( "${matches[@]}" )
done

args=()
for revision in ${REVISIONS:-}; do args+=( --revision "$revision" ); done
if [[ "${STRICT:-false}" == "true" ]]; then args+=( --strict ); fi

# The human report, to the log and the job summary; its status is the verdict.
# Its stderr is kept too: it names the problem when no report could be made.
status=0
mcp-trace-validator validate "${args[@]}" "${traces[@]}" > "$RUNNER_TEMP/mcp-conformance.txt" 2> "$RUNNER_TEMP/mcp-conformance.err" || status=$?
cat "$RUNNER_TEMP/mcp-conformance.txt"
cat "$RUNNER_TEMP/mcp-conformance.err" >&2
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

# A run that could not start wrote no document, leaving an empty file.
problem=$(grep -m 1 '^error: ' "$RUNNER_TEMP/mcp-conformance.err")
problem=${problem#error: }
problem=${problem:-mcp-trace-validator exited ${status} without a report}
if [[ -n "${SARIF_FILE:-}" && ! -s "$SARIF_FILE" ]]; then write_unjudged_sarif "$SARIF_FILE" "$problem"; fi
if [[ -n "${JUNIT_FILE:-}" && ! -s "$JUNIT_FILE" ]]; then write_unjudged_junit "$JUNIT_FILE" "$problem"; fi
echo "exit-code=${status}" >> "$GITHUB_OUTPUT"
