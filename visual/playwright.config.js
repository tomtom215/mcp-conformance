// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// Screenshot suite for mcp-trace-validator's HTML report and the mcp-conformance
// workbench (ADR-0021, "Verification of what people see"). The report pages under test
// are the committed golden pages, corpus/golden/html/*.html, which the Rust golden test
// pins byte for byte; the workbench is the binary serving visual/fixtures/library.

import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  // Baselines are named for what they show, not for the platform: they are
  // only ever taken on Linux with DejaVu installed (see README.md).
  snapshotPathTemplate: '{testDir}/__screenshots__/{arg}{ext}',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    browserName: 'chromium',
    // Rendering must not depend on the machine's settings.
    deviceScaleFactor: 1,
    locale: 'en-US',
    timezoneId: 'UTC',
  },
  expect: {
    toHaveScreenshot: {
      // Exact: no pixel may differ. A visual change is a reviewed baseline
      // update (`npm run bless`), like a golden report.
      maxDiffPixels: 0,
      animations: 'disabled',
      caret: 'hide',
    },
  },
});
