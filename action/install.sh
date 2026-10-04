#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

# Installs mcp-trace-validator for action.yml: the prebuilt, checksum-verified
# release archive when VERSION is set, else a build from the action's own
# source (the checkout GitHub made at the action ref). Puts it on PATH.
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
  cp "$RUNNER_TEMP"/mcp-conformance/bin/mcp-trace-validator* "$bin_dir/"
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
  if [[ "$ext" == zip ]]; then
    unzip -q "${name}.zip"
  else
    tar -xzf "${name}.tar.gz"
  fi
  cp "${name}"/mcp-trace-validator* "$bin_dir/"
fi
echo "$bin_dir" >> "$GITHUB_PATH"
"$bin_dir"/mcp-trace-validator --version
