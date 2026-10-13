// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// Starts `mcp-conformance serve` over a fresh copy of the committed fixture
// library (visual/fixtures/library), and opens browser contexts on it.
//
// The binary is $MCP_CONFORMANCE_BIN, else target/debug/mcp-conformance at the
// repository root (`cargo build -p mcp-conformance-cli`). The copy lives in a
// new temporary directory and is served as the relative path `library`, so the
// page shows the same path on every machine.

import { spawn } from 'node:child_process';
import { cpSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '../../..');
export const fixtures = join(root, 'visual/fixtures/library');
const binary = process.env.MCP_CONFORMANCE_BIN || join(root, 'target/debug/mcp-conformance');

/** A running workbench: its origin, its library copy, and how to stop it. */
export async function startWorkbench() {
  const work = mkdtempSync(join(tmpdir(), 'mcp-workbench-'));
  const library = join(work, 'library');
  cpSync(fixtures, library, { recursive: true });
  const child = spawn(binary, ['serve', '--library', 'library'], { cwd: work, stdio: ['ignore', 'pipe', 'pipe'] });
  let stderr = '';
  child.stderr.on('data', (chunk) => { stderr += chunk; });
  const launch = await new Promise((resolveUrl, reject) => {
    let out = '';
    const timer = setTimeout(() => reject(new Error(`serve printed no link: ${out}${stderr}`)), 20000);
    child.stdout.on('data', (chunk) => {
      out += chunk;
      const match = out.match(/open (http:\/\/\S+)/);
      if (match) {
        clearTimeout(timer);
        resolveUrl(match[1]);
      }
    });
    child.on('exit', (code) => reject(new Error(`serve exited ${code}: ${stderr}`)));
  });
  const origin = new URL(launch).origin;
  return {
    origin,
    launch,
    library,
    stderr: () => stderr,
    async stop() {
      child.kill('SIGINT');
      await new Promise((done) => { child.once('exit', done); setTimeout(done, 3000); });
      rmSync(work, { recursive: true, force: true });
    },
  };
}

/**
 * Exchanges the single-use launch link for the session cookie, as a browser
 * does on first opening it; returns the cookies to give each new context.
 */
export async function launchCookies(playwright, workbench) {
  const request = await playwright.request.newContext();
  const response = await request.get(workbench.launch, { maxRedirects: 0 });
  if (response.status() !== 303) throw new Error(`launch answered ${response.status()}`);
  const { cookies } = await request.storageState();
  await request.dispose();
  return cookies;
}

/**
 * Records everything that would make the workbench less than local: every
 * request that is not to its own origin (each is also aborted) and every
 * console error, warning or uncaught exception (a CSP violation is one).
 */
export async function watch(page, origin) {
  const foreign = [];
  const errors = [];
  await page.route('**/*', (route) => {
    const url = route.request().url();
    if (url.startsWith(`${origin}/`)) return route.continue();
    foreign.push(url);
    return route.abort();
  });
  page.on('console', (message) => {
    if (message.type() === 'error' || message.type() === 'warning') errors.push(message.text());
  });
  page.on('pageerror', (error) => errors.push(error.message));
  return { foreign, errors };
}
