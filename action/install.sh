#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

# Installs mcp-trace-validator for action.yml: the prebuilt release archive,
# checksum- and attestation-verified, when VERSION is set, else a build from the action's own
# source (the checkout GitHub made at the action ref). Puts it on PATH. With
# CAPTURE set, mcp-trace-capture too (the archive carries both; a source build
# compiles it only when it will be used).
set -euo pipefail

bin_dir="$RUNNER_TEMP/mcp-conformance-bin"
mkdir -p "$bin_dir"

if [[ -z "${VERSION:-}" ]]; then
  echo "Building mcp-trace-validator from the action's source ($GITHUB_ACTION_PATH)"
  # The workspace pins its toolchain (rust-toolchain.toml); rustup installs it
  # on first use. --locked builds exactly the lockfile the action ref carries.
  (cd "$GITHUB_ACTION_PATH" &&
    cargo install --locked --quiet --root "$RUNNER_TEMP/mcp-conformance" \
      --path crates/mcp-trace-validator)
  if [[ -n "${CAPTURE:-}" ]]; then
    echo "Building mcp-trace-capture from the action's source"
    (cd "$GITHUB_ACTION_PATH" &&
      cargo install --locked --quiet --root "$RUNNER_TEMP/mcp-conformance" \
        --path crates/mcp-trace-capture)
  fi
  cp "$RUNNER_TEMP"/mcp-conformance/bin/mcp-trace-* "$bin_dir/"
else
  case "${RUNNER_OS}-${RUNNER_ARCH}" in
    Linux-X64) target=x86_64-unknown-linux-musl ext=tar.gz ;;
    Linux-ARM64) target=aarch64-unknown-linux-musl ext=tar.gz ;;
    macOS-ARM64) target=aarch64-apple-darwin ext=tar.gz ;;
    macOS-X64) target=x86_64-apple-darwin ext=tar.gz ;;
    Windows-X64) target=x86_64-pc-windows-msvc ext=zip ;;
    *) echo "::error::no prebuilt binary for ${RUNNER_OS}-${RUNNER_ARCH}; leave version empty to build from source"; exit 1 ;;
  esac
  version="${VERSION#v}"
  name="mcp-conformance-v${version}-${target}"
  base="https://github.com/${REPOSITORY}/releases/download/v${version}"
  cd "$RUNNER_TEMP"
  curl -sSfL --retry 3 -o "${name}.${ext}" "${base}/${name}.${ext}"
  curl -sSfL --retry 3 -o SHA256SUMS-binaries "${base}/SHA256SUMS-binaries"
  # The release lists each archive as `<hash>  ./<name>`; check ours alone.
  expected="$(awk -v file="./${name}.${ext}" '$2 == file { print $1 }' SHA256SUMS-binaries)"
  if [[ -z "$expected" ]]; then
    echo "::error::${name}.${ext} is not listed in the release's SHA256SUMS-binaries"
    exit 1
  fi
  if command -v sha256sum > /dev/null; then
    actual="$(sha256sum "${name}.${ext}" | awk '{ print $1 }')"
  else
    actual="$(shasum -a 256 "${name}.${ext}" | awk '{ print $1 }')"
  fi
  if [[ "$actual" != "$expected" ]]; then
    echo "::error::checksum mismatch for ${name}.${ext}: expected ${expected}, got ${actual}"
    exit 1
  fi
  # The checksum list comes from the same release as the archive, so it shows the
  # download was not corrupted and nothing more: whoever can replace the archive
  # can replace the list. The build attestation is what ties the archive to this
  # repository's release workflow at this tag, so it is checked too, and
  # failing closed: no `gh`, or an attestation that does not match, is no install.
  # VERIFY_ATTESTATION=false skips it (a release that predates attestations).
  if [[ "${VERIFY_ATTESTATION:-true}" == "false" ]]; then
    echo "::warning::verify-attestation is false: ${name}.${ext} is checked against the release's own checksum list only, which does not show who built it"
  else
    if ! command -v gh > /dev/null; then
      echo "::error::the GitHub CLI (gh) is needed to verify the attestation of ${name}.${ext}; install it, or set verify-attestation: false"
      exit 1
    fi
    if ! gh attestation verify "${name}.${ext}" \
      --repo "${REPOSITORY}" \
      --signer-workflow "${REPOSITORY}/.github/workflows/release.yml" \
      --source-ref "refs/tags/v${version}"; then
      echo "::error::${name}.${ext} has no valid attestation from ${REPOSITORY}'s release workflow at v${version}; not installing it (verify-attestation: false skips the check, for a release that predates attestations)"
      exit 1
    fi
  fi
  if [[ "$ext" == zip ]]; then
    unzip -q "${name}.zip"
  else
    tar -xzf "${name}.tar.gz"
  fi
  cp "${name}"/mcp-trace-validator* "${name}"/mcp-trace-capture* "$bin_dir/"
fi
echo "$bin_dir" >> "$GITHUB_PATH"
"$bin_dir"/mcp-trace-validator --version
if [[ -n "${CAPTURE:-}" ]]; then "$bin_dir"/mcp-trace-capture --version; fi
