// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// Hand-written SVG charts (ADR-0021: no chart library). Geometry is in
// attributes and colour in classes the stylesheet defines, so the Content
// Security Policy, which admits no inline style, has nothing to refuse.
// Every mark carries a <title> (the hover tooltip) and every chart has a
// table of the same numbers beside it.

import { h } from './dom.js';

const ROW = 26;
const GAP = 2;

/** Stacked vertical bars: one per category, segments in `series` order. */
export function stackedColumns({ label, categories, series, value, width = 640 }) {
  const height = 220;
  const top = 12;
  const bottom = 36;
  const left = 34;
  const totals = categories.map((c) => series.reduce((sum, s) => sum + value(c, s.key), 0));
  const max = Math.max(1, ...totals);
  const step = (width - left) / Math.max(1, categories.length);
  const bar = Math.min(48, step * 0.6);
  const scale = (n) => ((height - top - bottom) * n) / max;
  const ticks = [0, Math.ceil(max / 2), max];
  return h('svg', { class: 'chart', viewBox: `0 0 ${width} ${height}`, role: 'img', 'aria-label': label, preserveAspectRatio: 'xMidYMid meet' },
    ticks.map((tick) => {
      const y = height - bottom - scale(tick);
      return h('g', { class: 'grid' }, h('line', { x1: left, x2: width, y1: y, y2: y }), h('text', { x: left - 6, y: y + 4, class: 'axis end' }, String(tick)));
    }),
    categories.map((category, index) => {
      let y = height - bottom;
      const x = left + index * step + (step - bar) / 2;
      return h('g', {},
        series.map((s) => {
          const n = value(category, s.key);
          if (!n) return null;
          const tall = Math.max(1, scale(n) - GAP);
          y -= scale(n);
          return h('rect', { x, y: y + GAP, width: bar, height: tall, rx: 3, class: `mark ${s.cls}` },
            h('title', {}, `${category}: ${n} ${s.label}`));
        }),
        h('text', { x: x + bar / 2, y: height - bottom + 16, class: 'axis mid' }, category.slice(5)));
    }));
}

/**
 * Horizontal bars: one row per item, each split into `parts` (in order),
 * against a common maximum. `total` (optional) draws a track the parts fill.
 */
export function bars({ label, items, parts, total, width = 640 }) {
  // Narrow: each label sits above its bar rather than beside it.
  const narrow = width < 480;
  const labelWidth = narrow ? 0 : Math.min(250, width * 0.3);
  const valueWidth = 76;
  const plot = width - labelWidth - valueWidth;
  const row = narrow ? ROW + 16 : ROW;
  const height = items.length * row + 8;
  const max = Math.max(1, ...items.map((item) => (total ? total(item) : parts.reduce((sum, p) => sum + p.value(item), 0))));
  return h('svg', { class: 'chart', viewBox: `0 0 ${width} ${height}`, role: 'img', 'aria-label': label },
    items.map((item, index) => {
      const y = 4 + index * row + (narrow ? 16 : 0);
      let x = labelWidth;
      const marks = parts.map((part) => {
        const n = part.value(item);
        if (!n) return null;
        const w = Math.max(2, (plot * n) / max - GAP);
        const rect = h('rect', { x, y: y + 4, width: w, height: ROW - 10, rx: 3, class: `mark ${part.cls}` },
          h('title', {}, `${item.name}: ${n} ${part.label}`));
        x += (plot * n) / max;
        return rect;
      });
      const track = total ? h('rect', { x: labelWidth, y: y + 4, width: plot, height: ROW - 10, rx: 3, class: 'track' }) : null;
      return h('g', {},
        narrow
          ? h('text', { x: 0, y: y - 3, class: 'axis' }, item.name)
          : h('text', { x: labelWidth - 8, y: y + ROW / 2 + 3, class: 'axis end' }, fit(item.name, labelWidth - 12), h('title', {}, item.name)),
        track,
        marks,
        h('text', { x: labelWidth + plot + 8, y: y + ROW / 2 + 3, class: 'axis value' }, item.text));
    }));
}

/** `text` cut with an ellipsis to fit about `pixels` of 11px type. */
function fit(text, pixels) {
  const room = Math.max(4, Math.floor(pixels / 6.6));
  return text.length > room ? `${text.slice(0, room - 1)}…` : text;
}

/** A legend: identity never by colour alone. */
export function legend(series) {
  return h('ul', { class: 'legend' }, series.map((s) => h('li', {}, h('span', { class: `swatch ${s.cls}`, 'aria-hidden': 'true' }), s.label)));
}

/** The same numbers as a table. */
export function table(caption, headings, rows) {
  return h('details', { class: 'numbers' }, h('summary', {}, caption),
    h('div', { class: 'table-scroll' }, h('table', { class: 'grid-table' },
      h('thead', {}, h('tr', {}, headings.map((title) => h('th', { scope: 'col' }, title)))),
      h('tbody', {}, rows.map((cells) => h('tr', {}, cells.map((cell, i) => h('td', { class: i ? 'num' : null }, String(cell)))))))));
}
