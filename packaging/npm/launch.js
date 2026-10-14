// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// The launcher behind every `bin` of the `mcp-conformance` npm package.
//
// The binaries are not in this package. Each platform's are in its own
// package, `@mcp-conformance/<os>-<cpu>`, which npm installs as an optional
// dependency only where its `os` and `cpu` fields match — the esbuild and
// Biome pattern: no install script, no download at install time, the release
// binary byte for byte. This file finds that package's binary and runs it,
// passing arguments, standard streams and the exit status through.
//
// Generated into the package by `cargo xtask packages`; the source is
// packaging/npm/launch.js in https://github.com/tomtom215/mcp-conformance.

"use strict";

const { spawn } = require("node:child_process");

/**
 * The platform package for this machine, or undefined where none is built.
 * The package's own optionalDependencies are the list of what is built.
 */
function platformPackage() {
  const candidate = `@mcp-conformance/${process.platform}-${process.arch}`;
  const built = Object.keys(require("../package.json").optionalDependencies ?? {});
  return built.includes(candidate) ? candidate : undefined;
}

/** The absolute path of `name`'s binary, or exits 2 saying why it cannot. */
function locate(name) {
  const pkg = platformPackage();
  if (pkg === undefined) {
    process.stderr.write(
      `${name}: no prebuilt binary for ${process.platform}-${process.arch}; ` +
        "install from source instead: cargo install mcp-trace-validator mcp-trace-capture\n",
    );
    process.exit(2);
  }
  const file = process.platform === "win32" ? `${name}.exe` : name;
  try {
    return require.resolve(`${pkg}/bin/${file}`);
  } catch {
    process.stderr.write(
      `${name}: the platform package ${pkg} is not installed. npm skips optional ` +
        "dependencies when run with --no-optional or --omit=optional, and when a " +
        "lockfile was written on another platform; reinstall without those.\n",
    );
    process.exit(2);
  }
}

/**
 * Runs `name` with this process's arguments and exits with its status.
 *
 * SIGTERM and SIGHUP sent to this process are passed on to the binary, so a
 * supervisor stopping the launcher stops the tool (the capture wrapper relays
 * them to the server it runs). SIGINT is not passed on: a terminal's Ctrl-C
 * already reaches every process in the foreground group, the binary included,
 * and a second copy would arrive as a second interrupt. This process waits for
 * the binary to finish either way. A binary killed by a signal kills this
 * process with the same signal, so callers see what they would have seen.
 */
function run(name) {
  const child = spawn(locate(name), process.argv.slice(2), { stdio: "inherit" });
  const forward = (signal) => () => {
    if (child.exitCode === null && child.signalCode === null) child.kill(signal);
  };
  process.on("SIGTERM", forward("SIGTERM"));
  process.on("SIGHUP", forward("SIGHUP"));
  process.on("SIGINT", () => {});
  child.on("error", (error) => {
    process.stderr.write(`${name}: cannot run the binary: ${error.message}\n`);
    process.exit(2);
  });
  child.on("exit", (code, signal) => {
    if (signal !== null) {
      process.removeAllListeners(signal);
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code ?? 2);
  });
}

module.exports = { run, platformPackage };
