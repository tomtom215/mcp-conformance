// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// Every workbench view as people see it — the library, a session at a
// finding's event, a hostile session, two sessions compared, the dashboards —
// at desktop and phone widths, light and dark, over the committed fixture
// library, each compared with its baseline. Each is opened with every request
// recorded: anything not to the workbench's own origin fails the test (and is
// aborted), as does any console error (where a CSP violation is reported) or
// horizontal scrolling.

import { test, expect } from '@playwright/test';
import { startWorkbench, launchCookies, watch } from './workbench/serve.js';

const VIEWS = [
  ['library', '#/', '.entries .entry'],
  ['session', '#/s/base-001-request-id-boolean/e3', '#e3 .inspector'],
  ['hostile', '#/s/hostile-markup', '.timeline .ev'],
  ['diff', '#/diff/sdk-go-2025-11-25/sdk-rust-2025-11-25', '.diff-rows .drow'],
  ['dashboards', '#/dashboards', 'svg.chart'],
];
const VIEWPORTS = [
  ['desktop', { width: 1280, height: 900 }],
  ['mobile', { width: 360, height: 780 }],
];

let workbench;
let cookies;
test.beforeAll(async ({ playwright }) => {
  workbench = await startWorkbench();
  cookies = await launchCookies(playwright, workbench);
});
test.afterAll(async () => workbench?.stop());

for (const [name, hash, ready] of VIEWS) {
  for (const [width, viewport] of VIEWPORTS) {
    for (const colorScheme of ['light', 'dark']) {
      test(`workbench ${name}, ${width}, ${colorScheme}`, async ({ page, context }) => {
        await context.addCookies(cookies);
        await page.setViewportSize(viewport);
        await page.emulateMedia({ colorScheme });
        const seen = await watch(page, workbench.origin);
        await page.goto(`${workbench.origin}/${hash}`);
        await expect(page.locator(ready).first()).toBeVisible();
        await page.evaluate(() => document.fonts.ready);
        // Both stylesheets applied (served from 'self', so the CSP admitted them).
        const background = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
        expect(background).toBe(colorScheme === 'dark' ? 'rgb(13, 17, 23)' : 'rgb(245, 246, 248)');
        const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
        expect(overflow, 'no horizontal scrolling').toBe(0);
        await page.evaluate(() => window.scrollTo(0, 0));
        await expect(page).toHaveScreenshot(`workbench-${name}-${width}-${colorScheme}.png`, { fullPage: true });
        expect(seen.foreign, 'no request left the workbench').toEqual([]);
        expect(seen.errors, 'no console errors or CSP violations').toEqual([]);
      });
    }
  }
}

test('a hostile recording renders as text and runs nothing', async ({ page, context }) => {
  await context.addCookies(cookies);
  const seen = await watch(page, workbench.origin);
  const dialogs = [];
  page.on('dialog', (dialog) => { dialogs.push(dialog.message()); dialog.dismiss(); });
  await page.goto(`${workbench.origin}/#/s/hostile-markup`);
  await expect(page.locator('.timeline .ev')).toHaveCount(8);
  // Open every event in turn, so every payload is on the page once.
  let shown = '';
  for (const seq of [0, 1, 3, 4, 5, 6, 7]) {
    await page.goto(`${workbench.origin}/#/s/hostile-markup/e${seq}`);
    await expect(page.locator(`#e${seq} .inspector`)).toBeVisible();
    shown += await page.locator('main').innerText();
    expect(await page.locator('main img, main script, main iframe, main object, main svg').count()).toBe(0);
  }
  for (const text of [
    '<script>alert(1)</script>',
    '"><img src=x onerror=alert(2)>',
    '<iframe src=\\"javascript:alert(4)\\"></iframe>',
    '<img src=x onerror=alert(5)>',
    '<svg onload=alert(6)>',
    'id "<b>id</b>"',
    // The bidirectional override and the escape character are shown, not obeyed.
    'evil\\u{202e}txt.exe',
    'notifications/\\u{1b}[31mred',
  ]) {
    expect(shown, text).toContain(text);
  }
  expect(dialogs).toEqual([]);
  expect(seen.foreign).toEqual([]);
  expect(seen.errors).toEqual([]);
});
