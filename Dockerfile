# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

# The container image: `mcp-trace-validator` and `mcp-trace-capture`, built from
# this workspace and nothing else, published as ghcr.io/tomtom215/mcp-conformance
# by release.yml on a release tag and built (never pushed) by ci.yml's `image`
# job on every pull request and push.
#
#   docker run --rm -v "$PWD:/work" ghcr.io/tomtom215/mcp-conformance:X.Y.Z \
#     mcp-trace-validator validate traces/*.jsonl
#
# Every input is pinned: both base images by digest (the tag beside each is for
# the reader; the digest is what is pulled), the toolchain by
# rust-toolchain.toml, which rustup honours inside the build exactly as it does
# in a checkout (`cargo xtask toolchain-pin` holds the builder's tag to the
# same version, so the compiler is the image's own and rustup fetches only the
# file's clippy and rustfmt components, checked against the channel
# manifest's hashes), and every crate by
# Cargo.lock (`--locked`). The binaries are built with `cargo auditable`, as
# release.yml builds the archives, so image scanners can read their dependency
# list without a lockfile.
#
# The runtime is distroless `cc` (Debian 13), not `static` and not a slim
# distribution:
#   - The builder is the official Debian-based Rust image, whose default target
#     is glibc. Building static musl binaries instead would mean installing
#     musl-tools from apt, an input this file cannot pin by digest; `cc` carries
#     exactly the glibc and libgcc the binaries link, from the same Debian
#     release as the builder, so the linked glibc is never newer than the one
#     the image runs.
#   - No shell, no package manager: nothing in the image runs but the two
#     binaries. It ships CA certificates, which `mcp-trace-capture http` needs
#     to reach an `https://` upstream (it trusts the platform's roots).
#   - The `nonroot` variant runs as UID/GID 65532. Mount the working directory
#     writable by that user when recording into it.
# The cost of having no shell: `mcp-trace-capture stdio -- <server>` can only
# launch a server that exists in the image, so stdio recording means building
# FROM this image with the server added. `http` mode and validation need
# nothing more.
#
# The builder is pulled for the platform being built (linux/amd64 is what CI
# builds and the release publishes); its default target is native, so no
# cross-compilation is involved.

FROM docker.io/library/rust:1.99.1-slim-trixie@sha256:aab42391a986424e22d6a3ec14f37bc6dd3dacff4d457ca7cccd781e02297cc6 AS build
WORKDIR /src
# Installed before the source is copied, so a source change does not rebuild
# it. Pinned and built --locked: it wraps the build of a shipped artifact.
RUN cargo install cargo-auditable --version 0.7.7 --locked
# .dockerignore admits only what the build reads: the manifests, the lockfile,
# the toolchain pin, and the workspace members.
COPY . .
RUN rustup show active-toolchain \
    && cargo auditable build --release --locked \
        -p mcp-trace-validator -p mcp-trace-capture --bins

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:e792ab3d241a468a4fd7519ddbbebe66b49b5f365771716ea688ad40b6c6f1c2
LABEL org.opencontainers.image.title="mcp-conformance" \
      org.opencontainers.image.description="mcp-trace-validator and mcp-trace-capture: record Model Context Protocol sessions and judge them clause by clause" \
      org.opencontainers.image.source="https://github.com/tomtom215/mcp-conformance" \
      org.opencontainers.image.licenses="MIT"
COPY --from=build /src/target/release/mcp-trace-validator /src/target/release/mcp-trace-capture /usr/local/bin/
COPY LICENSE /usr/share/doc/mcp-conformance/LICENSE
USER 65532:65532
WORKDIR /work
CMD ["mcp-trace-validator", "--help"]
