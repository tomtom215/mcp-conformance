// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// The workbench end to end: search, finding-to-event navigation, the diff,
// redaction, deletion, import, export, retention and the live tail, each
// against its own `mcp-conformance serve` over a fresh copy of the fixture
// library, with every request checked to stay on the workbench's origin.

import { test as base, expect } from '@playwright/test';
import { appendFileSync, existsSync, readFileSync, readdirSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { startWorkbench, launchCookies, watch } from './workbench/serve.js';

const here = dirname(fileURLToPath(import.meta.url));

const test = base.extend({
  workbench: async ({ playwright, context, page }, use) => {
    const workbench = await startWorkbench();
    await context.addCookies(await launchCookies(playwright, workbench));
    const seen = await watch(page, workbench.origin);
    await use(workbench);
    expect(seen.foreign, 'no request left the workbench').toEqual([]);
    expect(seen.errors, 'no console errors').toEqual([]);
    await workbench.stop();
  },
});

async function ids(page) {
  await expect(page.locator('.entries')).toBeVisible();
  return page.locator('.entries .entry-name').allInnerTexts();
}

test('search finds the sessions a query names', async ({ page, workbench }) => {
  await page.goto(`${workbench.origin}/#/`);
  await expect(page.locator('.entries .entry')).toHaveCount(11);
  const search = async (query, expected) => {
    await page.locator('input.query').fill(query);
    await expect(page.locator('.list-head .lede')).toContainText(`${expected.length} of 11`);
    // Two queries can match as many sessions: wait for these ones.
    await expect.poll(async () => (await ids(page)).sort()).toEqual([...expected].sort());
  };
  await search('req:BASE-001', ['base-001-request-id-boolean']);
  await search('code:-32602', ['hostile-markup', 'sdk-go-2025-11-25', 'sdk-rust-2025-11-25', 'sdk-rust-2026-07-28']);
  await search('code:-32602 -tag:sdk', ['hostile-markup']);
  await search('verdict:fail', ['base-001-request-id-boolean']);
  await search('tool:test_simple_text server:rust*', ['sdk-rust-2025-11-25']);
  await search('method:tools/call tag:needs-redaction', ['leaky-session']);
  // A hit links to the event it matched.
  await search('req:BASE-001', ['base-001-request-id-boolean']);
  await page.locator('.hits a', { hasText: '#3' }).click();
  await expect(page.locator('#e3 .inspector')).toBeVisible();
  // The query is in the address, so it survives a reload.
  await page.goto(`${workbench.origin}/#/?q=${encodeURIComponent('verdict:fail')}`);
  await page.reload();
  expect(await ids(page)).toEqual(['base-001-request-id-boolean']);
  await page.locator('input.query').fill('tool:"unclosed');
  await expect(page.locator('.notice.error')).toContainText('not closed');
});

test('a finding opens its event, and the event links back to the finding', async ({ page, workbench }) => {
  await page.goto(`${workbench.origin}/#/s/base-001-request-id-boolean`);
  await page.locator('#f1 .to-event').click();
  await expect(page).toHaveURL(/#\/s\/base-001-request-id-boolean\/e3$/);
  await expect(page.locator('#e3')).toBeInViewport();
  await expect(page.locator('#e3 .inspector pre')).toContainText('"id": true');
  await page.locator('#e3 .marks a', { hasText: 'BASE-001' }).click();
  await expect(page).toHaveURL(/\/f1$/);
  await expect(page.locator('#f1.selected')).toBeInViewport();
  // Only events with findings.
  await page.getByText('Only events with findings').click();
  await expect(page.locator('.timeline > .ev:visible')).toHaveCount(1);
});

test('the diff shows what changed between two servers', async ({ page, workbench }) => {
  await page.goto(`${workbench.origin}/#/`);
  await page.getByLabel('Select sdk-go-2025-11-25').check();
  await page.getByLabel('Select sdk-rust-2025-11-25').check();
  await page.getByRole('button', { name: 'Compare' }).click();
  await expect(page).toHaveURL(/#\/diff\/sdk-go-2025-11-25\/sdk-rust-2025-11-25$/);
  const row = page.locator('.grid-table tbody tr', { hasText: 'RES-006' });
  await expect(row).toContainText('pass');
  await expect(row).toContainText('not-observed');
  await expect(page.locator('.section .lede').first()).toContainText('8 the same, 9 changed');
  await page.getByText('Only differences').click();
  await expect(page.locator('.drow:visible')).toHaveCount(9);
  const changed = page.locator('.drow.d-changed', { hasText: 'prompts/get' }).last();
  await expect(changed.locator('.cell').first()).toContainText('error');
  await expect(changed.locator('.cell').last()).toContainText('result');
});

const SECRETS = ['fixture-not-a-real-token-1', 'fixture-cookie-2', 'fixture-api-key-3', 'fixture-meta-4'];

test('redaction adds a copy without the secrets and leaves the original', async ({ page, workbench }) => {
  const original = readFileSync(join(workbench.library, 'leaky-session.jsonl'), 'utf8');
  for (const secret of SECRETS) expect(original).toContain(secret);
  await page.goto(`${workbench.origin}/#/s/leaky-session`);
  await page.getByRole('button', { name: 'Redact…' }).click();
  await page.getByLabel('JSON pointers').fill('/params/arguments/apiKey');
  await page.getByLabel('Literal strings').fill('fixture-api-key-3');
  await page.getByRole('button', { name: 'Make a redacted copy' }).click();
  await expect(page).toHaveURL(/#\/s\/leaky-session\.redacted$/);
  await expect(page.locator('.provenance')).toContainText('A redacted copy of leaky-session');
  const copy = readFileSync(join(workbench.library, 'leaky-session.redacted.jsonl'), 'utf8');
  const sidecar = readFileSync(join(workbench.library, 'leaky-session.redacted.meta.json'), 'utf8');
  for (const secret of SECRETS) {
    expect(copy, secret).not.toContain(secret);
    expect(sidecar, secret).not.toContain(secret);
  }
  expect(copy).toContain('[redacted]');
  expect(readFileSync(join(workbench.library, 'leaky-session.jsonl'), 'utf8')).toBe(original);
  // The copy is judged like any recording.
  await expect(page.locator('.verdict .badge').first()).toBeVisible();
});

test('delete removes the recording and its sidecar', async ({ page, workbench }) => {
  await page.goto(`${workbench.origin}/#/`);
  await page.getByLabel('Select tool-005-name-length').check();
  await page.locator('.list-head').getByRole('button', { name: 'Delete' }).click();
  await page.locator('dialog').getByRole('button', { name: 'Delete' }).click();
  await expect(page.locator('.entries .entry')).toHaveCount(10);
  expect(await ids(page)).not.toContain('tool-005-name-length');
  expect(existsSync(join(workbench.library, 'tool-005-name-length.jsonl'))).toBe(false);
  expect(existsSync(join(workbench.library, 'tool-005-name-length.meta.json'))).toBe(false);
});

test('an mcpsnoop capture imports with what it does not record', async ({ page, workbench }) => {
  await page.goto(`${workbench.origin}/#/`);
  await page.getByRole('button', { name: 'Import…' }).click();
  await page.getByLabel('Recording file').setInputFiles(resolve(here, '../../conformance/mcpsnoop/native/http-2025-11-25.mcpsnoop.jsonl'));
  await page.locator('dialog').getByRole('button', { name: 'Import' }).click();
  await expect(page).toHaveURL(/#\/s\/http-2025-11-25\.mcpsnoop$/);
  await expect(page.locator('.provenance')).toContainText('Imported from mcpsnoop');
  await expect(page.locator('.provenance .gaps')).toContainText('Mcp-Session-Id');
  await expect(page.locator('.timeline .ev').first()).toBeVisible();
  expect(readdirSync(workbench.library)).toContain('http-2025-11-25.mcpsnoop.jsonl');
});

test('export downloads the selected recordings as a tar', async ({ page, workbench }) => {
  await page.goto(`${workbench.origin}/#/`);
  await page.getByLabel('Select everything-http').check();
  const download = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Export' }).click();
  const file = await (await download).path();
  const bytes = readFileSync(file);
  expect(bytes.subarray(257, 262).toString()).toBe('ustar');
  expect(bytes.subarray(0, 21).toString()).toBe('everything-http.jsonl');
  expect(bytes.toString('latin1')).toContain('everything-http.meta.json');
});

test('retention lists the recordings older than the days kept', async ({ page, workbench }) => {
  // Every fixture is dated between 2026-09-29 and 2026-10-12.
  const daysSince = (day) => Math.floor((Date.now() - Date.parse(`${day}T00:00:00Z`)) / 86400000);
  await page.goto(`${workbench.origin}/#/`);
  await page.getByRole('button', { name: 'Retention…' }).click();
  await page.getByLabel('Days to keep').fill(String(daysSince('2026-09-30')));
  await expect(page.locator('dialog .ids li')).toHaveCount(2);
  await expect(page.locator('dialog .ids')).toContainText('everything-http');
  await page.locator('dialog').getByRole('button', { name: 'Delete these' }).click();
  await expect(page.locator('.entries .entry')).toHaveCount(9);
});

test('following a trace shows events as they are written', async ({ page, workbench }) => {
  await page.goto(`${workbench.origin}/#/s/everything-stdio`);
  const before = 27; // the fixture's events
  await expect(page.locator('.timeline .ev')).toHaveCount(before);
  await page.getByText('Follow live').click();
  const trace = join(workbench.library, 'everything-stdio.jsonl');
  const lines = readFileSync(trace, 'utf8').trim().split('\n');
  const last = JSON.parse(lines[lines.length - 1]);
  const ping = { seq: last.seq + 1, direction: 'client-to-server', transport: 'stdio', kind: 'message', payload: { jsonrpc: '2.0', id: 9001, method: 'ping' } };
  appendFileSync(trace, `${JSON.stringify(ping)}\n`);
  await expect(page.locator('.timeline .ev')).toHaveCount(before + 1, { timeout: 10000 });
  await expect(page.locator('.timeline .ev').last()).toContainText('ping');
});

test('the launch link works once', async ({ playwright, workbench }) => {
  const request = await playwright.request.newContext();
  const again = await request.get(workbench.launch, { maxRedirects: 0 });
  expect(again.status()).toBe(403);
  const anonymous = await request.get(`${workbench.origin}/api/library`);
  expect(anonymous.status()).toBe(401);
  await request.dispose();
});
