#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)
#
# test-packages.sh PACKAGES_DIR VERSION
#
# Installs what `cargo xtask packages` wrote to PACKAGES_DIR from the local
# files alone — no registry is consulted — and runs it as a user would:
#
#   npm: `npm pack` the main package and this platform's, `npm install
#        --offline` both tarballs into a scratch project, then
#        `npx mcp-conformance --version` and the other two binaries' through
#        npx, and judge corpus traces through npx;
#   pip: `pip install --no-index --find-links` into a fresh venv, so pip itself
#        picks the wheel whose tags fit this machine (a wrong tag fails here),
#        then the same through the installed scripts and
#        `python -m mcp_conformance`.
#
# Each `--version` must name VERSION; a passing trace must exit 0 and a
# violation trace 1. ci.yml's `registry-packages` job runs this on Linux, macOS
# and Windows (Git Bash), release.yml's on Linux; it runs locally the same
# way. PYTHON (default `python3`, else `python`) makes the venv.
set -euo pipefail

packages="$(cd "${1:?usage: test-packages.sh PACKAGES_DIR VERSION}" && pwd)"
version="${2:?usage: test-packages.sh PACKAGES_DIR VERSION}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
good="$root/corpus/good/stdio-minimal-init.jsonl"
bad="$root/corpus/draft/violations/mrtr-019-retry-reuses-id.jsonl"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

fail() { echo "::error::$*" >&2; exit 1; }

# Runs "$@" over the two traces: the passing one must exit 0, the violation 1.
judge() {
  "$@" validate --quiet "$good" > /dev/null || fail "$* judged a passing trace as failing"
  local status=0
  "$@" validate --quiet "$bad" > /dev/null || status=$?
  [[ "$status" -eq 1 ]] || fail "$* exited ${status} on a violation trace, expected 1"
}

# Runs "$@" --version for each binary and checks the version it names.
versions() {
  local binary got
  for binary in mcp-conformance mcp-trace-validator mcp-trace-capture; do
    got="$("$@" "$binary" --version | head -n 1 | tr -d '\r')"
    [[ "$got" == "$binary $version" ]] || fail "$binary --version printed '${got}', expected '$binary $version'"
    echo "ok: $got"
  done
}

# ── npm ─────────────────────────────────────────────────────────────────────
platform="$(node -p 'process.platform + "-" + process.arch')"
[[ -d "$packages/npm/$platform" ]] || fail "no npm package for this platform ($platform) in $packages/npm"
mkdir "$scratch/tgz" "$scratch/project"
for dir in mcp-conformance "$platform"; do
  (cd "$packages/npm/$dir" && npm pack --silent --pack-destination "$scratch/tgz" > /dev/null)
done
ls -l "$scratch/tgz"
(
  cd "$scratch/project"
  npm init -y > /dev/null
  # --offline: every other platform's optional dependency is unpublished (or,
  # after a release, would come from the registry); npm skips an optional
  # dependency it cannot fetch, which is the behaviour being relied on.
  npm install --offline --no-audit --no-fund "$scratch"/tgz/*.tgz
  versions npx --no-install
  judge npx --no-install mcp-trace-validator
  # The launcher passes the exit status through for the workbench binary too.
  status=0
  npx --no-install mcp-conformance --no-such-flag > /dev/null 2>&1 || status=$?
  [[ "$status" -eq 2 ]] || fail "mcp-conformance --no-such-flag exited ${status} through npx, expected 2"
)
echo "npm: installed from local tarballs and ran on $platform"

# ── pip ─────────────────────────────────────────────────────────────────────
python="${PYTHON:-}"
if [[ -z "$python" ]]; then
  if command -v python3 > /dev/null && python3 -c '' 2> /dev/null; then python=python3; else python=python; fi
fi
"$python" -I -m venv "$scratch/venv"
if [[ -d "$scratch/venv/Scripts" ]]; then bin="$scratch/venv/Scripts"; else bin="$scratch/venv/bin"; fi
# --pre: a pre-release (0.7.0rc1) is the only version there, and pip skips
# pre-releases unless told otherwise.
"$bin/python" -I -m pip install --no-index --pre --find-links "$packages/pypi" mcp-conformance
run_script() { local name="$1"; shift; "$bin/$name" "$@"; }
versions run_script
judge "$bin/mcp-trace-validator"
got="$("$bin/python" -I -m mcp_conformance --version | tr -d '\r')"
[[ "$got" == "mcp-conformance $version" ]] || fail "python -m mcp_conformance --version printed '${got}'"
echo "ok: python -m mcp_conformance --version -> $got"
echo "pip: installed the wheel pip chose for this machine into a venv and ran it"
