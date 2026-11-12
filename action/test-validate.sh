#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

# Checks the output contract of action/validate.sh: whatever the run decided,
# each configured report exists and parses, and an output that could not be
# written is an exit status of 2 with an `::error::`, never an exit 0 over a
# missing file. Run from the repository root with `mcp-trace-validator` on
# PATH (CI builds it first); needs jq and python3.
#
#   PATH="$PWD/target/debug:$PATH" bash action/test-validate.sh
set -uo pipefail

root=$(pwd)
script="$root/action/validate.sh"
passing="$root/corpus/good/stdio-full-session.jsonl"
failures=0
pre=

fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}

# Runs validate.sh in a fresh directory with the given environment (NAME=value
# arguments after the case name); leaves $out (stdout and stderr) and the
# `exit-code` it wrote in $code.
run_case() {
  local name=$1
  shift
  work=$(mktemp -d)
  : > "$work/output"
  : > "$work/summary"
  mkdir "$work/runner"
  (cd "$work" && eval "${pre:-:}")
  out=$(cd "$work" && env "$@" GITHUB_OUTPUT="$work/output" GITHUB_STEP_SUMMARY="$work/summary" \
    RUNNER_TEMP="$work/runner" bash "$script" 2>&1)
  script_status=$?
  code=$(sed -n 's/^exit-code=//p' "$work/output" | tail -n 1)
  [[ $script_status -eq 0 ]] || fail "$name: the step itself must always succeed, exited $script_status"
}

parses_as_sarif() { jq -e '.version == "2.1.0" and (.runs | length) == 1' "$1" > /dev/null 2>&1; }
parses_as_xml() { python3 -c 'import sys, xml.dom.minidom; xml.dom.minidom.parse(sys.argv[1])' "$1" 2> /dev/null; }

# A passing trace: both reports, exit 0.
run_case control TRACES="$passing" SARIF_FILE=r.sarif JUNIT_FILE=r.xml
[[ $code == 0 ]] || fail "control: exit-code=$code, expected 0"
parses_as_sarif "$work/r.sarif" || fail "control: r.sarif is not a SARIF log"
parses_as_xml "$work/r.xml" || fail "control: r.xml is not XML"

# Report paths under a directory that does not exist yet: made, not skipped.
run_case missing-parent TRACES="$passing" SARIF_FILE=out/nested/r.sarif JUNIT_FILE=out/nested/r.xml
[[ $code == 0 ]] || fail "missing-parent: exit-code=$code, expected 0"
parses_as_sarif "$work/out/nested/r.sarif" || fail "missing-parent: no SARIF log was written"
parses_as_xml "$work/out/nested/r.xml" || fail "missing-parent: no JUnit report was written"

# A report path whose parent cannot be made (a file is in the way): the
# promised file cannot exist, so the run is exit 2 and says so.
pre=': > blocked'
run_case unwritable TRACES="$passing" SARIF_FILE=blocked/r.sarif JUNIT_FILE=blocked/r.xml
pre=
[[ $code == 2 ]] || fail "unwritable: exit-code=$code, expected 2 (output not written)"
[[ $out == *"::error::"*"blocked/r.sarif"* ]] || fail "unwritable: no ::error:: naming the SARIF path: $out"

# A trace named with characters the report formats must escape: a missing
# trace is exit 2 and both fallback reports still parse and name it. Under
# bash 5.2 an unescaped `&` in a replacement means the matched text, which
# turned `<` into `<lt;` here.
for hostile in 'missing<&trace.jsonl' 'quote"and\slash.jsonl' $'ctrl\001char.jsonl' $'tab\there.jsonl'; do
  run_case "hostile" TRACES="$hostile" SARIF_FILE=r.sarif JUNIT_FILE=r.xml
  [[ $code == 2 ]] || fail "$hostile: exit-code=$code, expected 2"
  parses_as_sarif "$work/r.sarif" || fail "$(printf '%q' "$hostile"): the SARIF log does not parse"
  parses_as_xml "$work/r.xml" || fail "$(printf '%q' "$hostile"): the JUnit report is not XML"
done
# The name survives the escaping (control characters excepted).
run_case named TRACES='missing<&trace.jsonl' SARIF_FILE=r.sarif JUNIT_FILE=r.xml
jq -e '.runs[0].invocations[0].toolExecutionNotifications[0].message.text | contains("missing<&trace.jsonl")' \
  "$work/r.sarif" > /dev/null || fail "named: the SARIF message lost the trace name"
python3 - "$work/r.xml" << 'PY' || fail "named: the JUnit message lost the trace name"
import sys, xml.dom.minidom
error = xml.dom.minidom.parse(sys.argv[1]).getElementsByTagName("error")[0]
sys.exit(0 if "missing<&trace.jsonl" in error.getAttribute("message") else 1)
PY

if [[ $failures -gt 0 ]]; then
  echo "$failures check(s) failed" >&2
  exit 1
fi
echo "validate.sh: output contract holds"
