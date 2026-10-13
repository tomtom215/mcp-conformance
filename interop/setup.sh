#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

# Installs the pinned SDKs and builds each one's conformance server, for
# run.sh and matrix.sh. Every input is pinned with integrity: npm by
# package-lock.json (`npm ci`), Python by requirements.txt's hashes, Go by
# go/go.sum, Rust by rust/Cargo.lock, and each file copied out of an SDK's
# repository by the commit and SHA-256 in sdk-pins.env.
#
#   ./setup.sh
#
# Needs node/npm, Python 3.11+ (PYTHON selects it; default python3), go 1.21+
# (it fetches the 1.25 toolchain go/go.mod asks for), cargo, and git. Writes
# node_modules/, .venv/, upstream/ and rust/upstream/, all ignored by git.

set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=sdk-pins.env
source sdk-pins.env
PYTHON="${PYTHON:-python3}"
rm -rf upstream rust/upstream
mkdir -p upstream/bin rust/upstream

# The package managers' pins must be the ones sdk-pins.env states: a bump
# that edits one and not the other would publish a matrix under the wrong
# version.
pinned() {
  if ! grep -qF -- "$2" "$1"; then
    echo "setup: $1 does not pin $2, which sdk-pins.env names" >&2
    exit 1
  fi
}
pinned package.json "\"@modelcontextprotocol/sdk\": \"$TS_SDK_VERSION\""
pinned requirements.in "mcp==$PY_SDK_VERSION"
pinned requirements.txt "mcp==$PY_SDK_VERSION "
pinned go/go.mod "github.com/modelcontextprotocol/go-sdk $GO_SDK_VERSION"
pinned rust/Cargo.toml "rmcp = { version = \"=$RUST_SDK_VERSION\""

# from_upstream REPO COMMIT PATH SHA256 DEST: one file of REPO at exactly
# COMMIT (fetched by its id, so no tag can be moved underneath it), refused
# unless its SHA-256 is the pinned one.
from_upstream() {
  local scratch
  scratch="$(mktemp -d)"
  git -C "$scratch" init --quiet
  git -C "$scratch" fetch --quiet --depth 1 "$1" "$2"
  git -C "$scratch" show "$2:$3" > "$5"
  rm -rf "$scratch"
  if ! echo "$4  $5" | sha256sum --check --strict --quiet; then
    echo "setup: $3 at $2 does not match its pinned SHA-256" >&2
    exit 1
  fi
}

# TypeScript: the SDK and the server's other imports, from the lockfile.
npm ci --no-audit --no-fund
from_upstream "$TS_SDK_REPO" "$TS_SDK_COMMIT" "$TS_SERVER_PATH" "$TS_SERVER_SHA256" upstream/everythingServer.ts
# The server imports the SDK by its path inside the SDK's own repository
# (`../../../src/…`); the published package exposes the same modules under
# its name, so this one rewrite runs it on the pinned npm release instead.
sed "s#'\.\./\.\./\.\./src/#'@modelcontextprotocol/sdk/#" upstream/everythingServer.ts > upstream/ts-everything-server.ts
if grep -q "\.\./\.\./\.\./src/" upstream/ts-everything-server.ts; then
  echo "setup: the TypeScript server still imports from the SDK's source tree" >&2
  exit 1
fi

# Python: the SDK from the hash-locked requirements; the server is one file.
"$PYTHON" -m venv .venv
.venv/bin/pip install --quiet --require-hashes -r requirements.txt
from_upstream "$PY_SDK_REPO" "$PY_SDK_COMMIT" "$PY_SERVER_PATH" "$PY_SERVER_SHA256" upstream/py_everything_server.py

# Go: the server ships in the module, so go.sum covers it.
(cd go && go build -o ../upstream/bin/go-everything-server github.com/modelcontextprotocol/go-sdk/conformance/everything-server)

# Rust: upstream's server source, compiled against the crates.io release.
from_upstream "$RUST_SDK_REPO" "$RUST_SDK_COMMIT" "$RUST_SERVER_PATH" "$RUST_SERVER_SHA256" rust/upstream/server.rs
cargo build --quiet --locked --manifest-path rust/Cargo.toml
cp "${CARGO_TARGET_DIR:-rust/target}/debug/conformance-server" upstream/bin/rust-conformance-server

echo "setup: TypeScript $TS_SDK_VERSION, Python $PY_SDK_VERSION, Go $GO_SDK_VERSION, Rust $RUST_SDK_VERSION ready"
