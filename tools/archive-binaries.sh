#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)
#
# archive-binaries.sh TARGET VERSION [RELEASE_DIR]
#
# Writes the release archive of the three CLIs built for TARGET, in the current
# directory, and prints its file name. release.yml's `binaries` job runs it, so
# the archive it attests is this script's output, and ci.yml's `packages` job
# runs it over a host build, so the npm and PyPI packages CI installs are built
# from an archive of exactly the release's shape (`cargo xtask packages` reads
# it). RELEASE_DIR defaults to target/TARGET/release.
#
# The name is what each crate's [package.metadata.binstall] pkg-url and bin-dir
# templates, and `cargo xtask packages`, expect; change them together.
set -euo pipefail

target="${1:?usage: archive-binaries.sh TARGET VERSION [RELEASE_DIR]}"
version="${2:?usage: archive-binaries.sh TARGET VERSION [RELEASE_DIR]}"
release_dir="${3:-target/${target}/release}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

name="mcp-conformance-v${version}-${target}"
ext=""
if [[ "$target" == *windows* ]]; then ext=".exe"; fi
rm -rf "$name"
mkdir "$name"
for binary in mcp-conformance mcp-trace-validator mcp-trace-capture; do
  cp "${release_dir}/${binary}${ext}" "$name/"
done
cp "$root/LICENSE" "$root/README.md" "$name/"
if [[ "$target" == *windows* ]]; then
  7z a -tzip "$name.zip" "$name" > /dev/null
  file="$name.zip"
else
  tar -czf "$name.tar.gz" "$name"
  file="$name.tar.gz"
fi
rm -rf "$name"
echo "$file"
