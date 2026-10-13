// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// Ad-hoc screenshots of any report, for looking at a design change before the
// baselines move: `node shoot.mjs <report.html> <out-dir>`. Not a test; the
// suite in tests/ is.

import { chromium } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { basename, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [, , report, outDir = 'shots'] = process.argv;
if (!report) {
  console.error('usage: node shoot.mjs <report.html> [out-dir]');
  process.exit(2);
}
mkdirSync(outDir, { recursive: true });
const browser = await chromium.launch();
const name = basename(report, '.html');
for (const [width, label] of [[1280, 'desktop'], [360, 'mobile']]) {
  for (const colorScheme of ['light', 'dark']) {
    const page = await browser.newPage({ viewport: { width, height: 800 }, colorScheme });
    page.on('console', (message) => console.log(`[${label}/${colorScheme}] console: ${message.text()}`));
    await page.goto(pathToFileURL(resolve(report)).href);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
    if (overflow > 0) console.log(`[${label}/${colorScheme}] horizontal overflow: ${overflow}px`);
    await page.screenshot({ path: `${outDir}/${name}-${label}-${colorScheme}.png`, fullPage: true });
    await page.close();
  }
}
await browser.close();
