// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// The HTML report as people see it: a passing, a failing and a two-revision
// report, at desktop and phone widths, in light and dark — each compared with
// its committed baseline, and each opened without a single request leaving
// the page, without a console error (a Content Security Policy violation is
// reported there), and without horizontal scrolling.

import { test, expect } from '@playwright/test';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, resolve } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const golden = (name) => pathToFileURL(resolve(here, '../../corpus/golden/html', `${name}.html`)).href;

const REPORTS = [
  ['pass', 'http-session'],
  ['fail', 'base-001-request-id-boolean'],
  ['two-revisions', 'disc-002-dual-era-client-skips-probe'],
];
const VIEWPORTS = [
  ['desktop', { width: 1280, height: 900 }],
  ['mobile', { width: 360, height: 780 }],
];
const SCHEMES = ['light', 'dark'];

/**
 * Opens a report and records everything that would make it less than
 * self-contained: every request other than the document itself (each one is
 * also aborted, so nothing leaves even if one is attempted) and every console
 * error.
 */
async function open(page, url) {
  const requests = [];
  const errors = [];
  await page.route('**/*', (route) => {
    const requested = route.request().url();
    if (requested === url) return route.continue();
    requests.push(requested);
    return route.abort();
  });
  page.on('request', (request) => {
    if (request.url() !== url && !requests.includes(request.url())) requests.push(request.url());
  });
  page.on('console', (message) => {
    if (message.type() === 'error' || message.type() === 'warning') errors.push(message.text());
  });
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(url, { waitUntil: 'load' });
  return { requests, errors };
}

for (const [label, name] of REPORTS) {
  for (const [width, viewport] of VIEWPORTS) {
    for (const colorScheme of SCHEMES) {
      test(`${label} report, ${width}, ${colorScheme}`, async ({ page }) => {
        await page.setViewportSize(viewport);
        await page.emulateMedia({ colorScheme });
        const url = golden(name);
        const { requests, errors } = await open(page, url);

        // The stylesheet applied: the CSP admitted it by digest. A wrong digest
        // leaves the page unstyled and logs a console error.
        const background = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
        expect(background).toBe(colorScheme === 'dark' ? 'rgb(13, 17, 23)' : 'rgb(245, 246, 248)');

        const overflow = await page.evaluate(
          () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
        );
        expect(overflow, 'no horizontal scrolling').toBe(0);

        await expect(page).toHaveScreenshot(`${label}-${width}-${colorScheme}.png`, { fullPage: true });

        expect(requests, 'the report made no request').toEqual([]);
        expect(errors, 'no console errors or CSP violations').toEqual([]);
      });
    }
  }
}

test('a finding and its event link to each other', async ({ page }) => {
  const url = golden('base-001-request-id-boolean');
  const { requests } = await open(page, url);
  await page.locator('#BASE-001-1 a.seq').click();
  await expect(page).toHaveURL(/#e3$/);
  await expect(page.locator('#e3')).toBeInViewport();
  await page.locator('#e3 .marks a', { hasText: 'BASE-001' }).click();
  await expect(page).toHaveURL(/#BASE-001-1$/);
  await expect(page.locator('#BASE-001-1')).toBeInViewport();
  expect(requests).toEqual([]);
});

test('the switch shows only the events with findings, without a script', async ({ page }) => {
  await open(page, golden('base-001-request-id-boolean'));
  expect(await page.locator('script').count()).toBe(0);
  await expect(page.locator('.timeline > .ev')).toHaveCount(4);
  await page.getByText('Only events with findings').click();
  await expect(page.locator('.timeline > .ev:visible')).toHaveCount(1);
  await expect(page.locator('#e3')).toBeVisible();
});

test('the accounting opens to its rows', async ({ page }) => {
  await open(page, golden('http-session'));
  const excluded = page.locator('details.g-excluded');
  await expect(excluded.locator('.rows')).toBeHidden();
  await excluded.locator('summary').click();
  await expect(excluded.locator('.rows li').first()).toBeVisible();
});

test('a run page makes no request either', async ({ page }) => {
  const { requests, errors } = await open(page, golden('run'));
  await expect(page.locator('h1')).toHaveText('3 traces');
  expect(requests).toEqual([]);
  expect(errors).toEqual([]);
});
