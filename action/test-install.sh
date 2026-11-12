#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

# Checks the release path of action/install.sh without a network: `curl` and
# `gh` are stand-ins on PATH, so what is tested is the script's decisions —
# that an archive is installed only when its checksum matches AND `gh
# attestation verify` accepts it for this repository's release workflow at the
# tag, failing closed when either does not, when `gh` is missing, and skipping
# the attestation (with a warning) only when asked. The real `gh attestation
# verify` is not run here; CI's `action` job runs the script against a real
# release.
#
#   bash action/test-install.sh        # from the repository root; Linux x86_64
set -uo pipefail

root=$(pwd)
script="$root/action/install.sh"
failures=0
fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}

# A work directory holding a fake release: the archive for this runner, its
# SHA256SUMS-binaries, a stub curl that serves them, and a stub gh.
#   $1 case name; $2 gh behaviour (accept|reject|absent); $3 tamper (yes|no)
setup_case() {
  local name=$1 gh=$2 tamper=$3
  case_dir=$(mktemp -d)
  local release="$case_dir/release" stubs="$case_dir/stubs"
  mkdir -p "$release" "$stubs" "$case_dir/runner" "$case_dir/pkg"
  local target=x86_64-unknown-linux-musl
  local pkg="mcp-conformance-v9.9.9-${target}"
  mkdir -p "$case_dir/pkg/$pkg"
  printf '#!/bin/sh\necho "mcp-trace-validator 9.9.9"\n' > "$case_dir/pkg/$pkg/mcp-trace-validator"
  printf '#!/bin/sh\necho "mcp-trace-capture 9.9.9"\n' > "$case_dir/pkg/$pkg/mcp-trace-capture"
  chmod +x "$case_dir/pkg/$pkg/"*
  (cd "$case_dir/pkg" && tar -czf "$release/$pkg.tar.gz" "$pkg")
  (cd "$release" && printf '%s  ./%s\n' "$(sha256sum "$pkg.tar.gz" | awk '{print $1}')" "$pkg.tar.gz" > SHA256SUMS-binaries)
  if [[ $tamper == yes ]]; then
    # The archive changes after the list was written: the checksum catches it.
    printf 'tampered' >> "$release/$pkg.tar.gz"
  fi
  cat > "$stubs/curl" << 'EOF'
#!/usr/bin/env bash
# Serves the fake release: the file named last on the command line.
out=""
url=""
while [[ $# -gt 0 ]]; do
  case $1 in
    -o) out=$2; shift 2 ;;
    -*) shift ;;
    *) url=$1; shift ;;
  esac
done
cp "$RELEASE_DIR/$(basename "$url")" "$out"
EOF
  chmod +x "$stubs/curl"
  case $gh in
    accept) printf '#!/bin/sh\necho "gh $*" >> "$GH_LOG"\nexit 0\n' > "$stubs/gh" ;;
    reject) printf '#!/bin/sh\necho "gh $*" >> "$GH_LOG"\necho "no matching attestations" >&2\nexit 1\n' > "$stubs/gh" ;;
    absent) ;;
  esac
  [[ -f "$stubs/gh" ]] && chmod +x "$stubs/gh"
  : > "$case_dir/gh.log"
  : > "$case_dir/path"
  echo "$name" > /dev/null
}

# Runs install.sh for the fake release with `extra` environment; sets $out and $status.
run_install() {
  # `gh` absent: PATH holds the stubs and the system's basic tools, but no gh.
  out=$(env -i HOME="$case_dir" PATH="$case_dir/stubs:/usr/bin:/bin" \
    RUNNER_TEMP="$case_dir/runner" RUNNER_OS=Linux RUNNER_ARCH=X64 \
    GITHUB_PATH="$case_dir/path" GITHUB_ACTION_PATH="$root" \
    REPOSITORY=tomtom215/mcp-conformance VERSION=9.9.9 \
    RELEASE_DIR="$case_dir/release" GH_LOG="$case_dir/gh.log" "$@" \
    bash "$script" 2>&1)
  status=$?
}

# The attestation accepted: installed, and gh was asked the right question.
setup_case accepted accept no
run_install
[[ $status -eq 0 ]] || fail "accepted: exited $status: $out"
grep -q 'attestation verify' "$case_dir/gh.log" || fail "accepted: gh was never asked to verify"
for want in '--repo tomtom215/mcp-conformance' \
  '--signer-workflow tomtom215/mcp-conformance/.github/workflows/release.yml' \
  '--source-ref refs/tags/v9.9.9'; do
  grep -qF -- "$want" "$case_dir/gh.log" || fail "accepted: gh was not given $want: $(cat "$case_dir/gh.log")"
done
[[ -x "$case_dir/runner/mcp-conformance-bin/mcp-trace-validator" ]] || fail "accepted: nothing was installed"

# The attestation rejected: refused, nothing installed.
setup_case rejected reject no
run_install
[[ $status -ne 0 ]] || fail "rejected: install.sh succeeded without a valid attestation"
[[ $out == *"::error::"*"attestation"* ]] || fail "rejected: no ::error:: naming the attestation: $out"
[[ -e "$case_dir/runner/mcp-conformance-bin/mcp-trace-validator" ]] && fail "rejected: the binary was installed anyway"

# gh missing: fail closed, and say how to proceed.
setup_case no-gh absent no
run_install
[[ $status -ne 0 ]] || fail "no-gh: install.sh succeeded with no way to verify"
[[ $out == *"::error::"*"gh"*"verify-attestation"* ]] || fail "no-gh: the error does not name gh and the opt-out: $out"

# The opt-out: gh is never called, and the run says what was skipped.
setup_case opt-out reject no
run_install VERIFY_ATTESTATION=false
[[ $status -eq 0 ]] || fail "opt-out: exited $status: $out"
[[ ! -s "$case_dir/gh.log" ]] || fail "opt-out: gh was called: $(cat "$case_dir/gh.log")"
[[ $out == *"::warning::"*"verify-attestation"* ]] || fail "opt-out: no warning that the attestation was skipped: $out"

# A tampered archive: the checksum refuses it before the attestation is asked.
setup_case tampered accept yes
run_install
[[ $status -ne 0 ]] || fail "tampered: install.sh succeeded"
[[ $out == *"checksum mismatch"* ]] || fail "tampered: no checksum mismatch: $out"
[[ ! -s "$case_dir/gh.log" ]] || fail "tampered: gh was asked about an archive the checksum had already refused"

if [[ $failures -gt 0 ]]; then
  echo "$failures check(s) failed" >&2
  exit 1
fi
echo "install.sh: release path holds"
