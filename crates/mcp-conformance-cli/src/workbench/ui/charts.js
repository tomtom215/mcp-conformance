// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// Hand-written SVG charts (ADR-0021: no chart library). Geometry is in
// attributes and colour in classes the stylesheet defines, so the Content
// Security Policy, which admits no inline style, has nothing to refuse.
// Every chart has a title, direct value labels, a legend when it draws more
// than one series, a tooltip on hover (the whole row or column is the target,
// not only the mark) and a table of the same numbers beside it.

import { h } from './dom.js';

const ROW = 28;
const GAP = 2;
const BAR = 14;

let tip = null;

/** Shows `text` beside the pointer; the position is set through the CSSOM. */
function showTip(text, x, y) {
  if (!tip) {
    tip = h('div', { class: 'tip', role: 'presentation', hidden: true });
    document.body.appendChild(tip);
  }
  tip.textContent = text;
  tip.hidden = false;
  const box = tip.getBoundingClientRect();
  tip.style.left = `${Math.max(4, Math.min(x + 14, window.innerWidth - box.width - 4))}px`;
  tip.style.top = `${Math.max(4, y - box.height - 10)}px`;
}

/** Removes the tooltip (a view calls this when it is left). */
export function hideTip() {
  if (tip) tip.hidden = true;
}

/** Gives `svg` a tooltip: whatever is under the pointer that carries `data-tip`. */
function withTips(svg) {
  svg.addEventListener('pointermove', (event) => {
    const mark = event.target.closest?.('[data-tip]');
    if (mark) showTip(mark.getAttribute('data-tip'), event.clientX, event.clientY);
    else hideTip();
  });
  svg.addEventListener('pointerleave', hideTip);
  return svg;
}

/**
 * Stacked vertical bars: one per category, segments in `series` order, on one
 * axis that starts at zero. `axis` names what the height counts.
 */
export function stackedColumns({ label, categories, series, value, axis, tick = (c) => c, width = 640 }) {
  const height = 240;
  const top = 26;
  const bottom = 30;
  const left = 34;
  const totals = categories.map((c) => series.reduce((sum, s) => sum + value(c, s.key), 0));
  const max = Math.max(1, ...totals);
  const step = (width - left) / Math.max(1, categories.length);
  const bar = Math.min(28, step * 0.45);
  const scale = (n) => ((height - top - bottom) * n) / max;
  const ticks = [...new Set([0, Math.ceil(max / 2), max])];
  const every = Math.ceil(46 / step);
  return withTips(h('svg', { class: 'chart', viewBox: `0 0 ${width} ${height}`, role: 'img', 'aria-label': label, preserveAspectRatio: 'xMidYMid meet' },
    h('text', { x: 0, y: 10, class: 'axis title' }, axis),
    ticks.map((n) => {
      const y = height - bottom - scale(n);
      return h('g', { class: 'grid' },
        h('line', { x1: left, x2: width, y1: y, y2: y, class: n ? null : 'base' }),
        h('text', { x: left - 6, y: y + 4, class: 'axis end' }, String(n)));
    }),
    categories.map((category, index) => {
      let y = height - bottom;
      const x = left + index * step + (step - bar) / 2;
      const parts = series.filter((s) => value(category, s.key)).map((s) => `${value(category, s.key)} ${s.label}`);
      return h('g', { class: 'col' },
        h('rect', { x: left + index * step, y: top, width: step, height: height - top - bottom, class: 'hit', 'data-tip': `${category}: ${parts.join(', ') || 'none'}` }),
        series.map((s) => {
          const n = value(category, s.key);
          if (!n) return null;
          y -= scale(n);
          return h('rect', { x, y: y + GAP, width: bar, height: Math.max(1, scale(n) - GAP), rx: 2, class: `mark ${s.cls}`, 'data-tip': `${category}: ${n} ${s.label}` });
        }),
        h('text', { x: x + bar / 2, y: y - 4, class: 'axis mid value' }, String(totals[index])),
        index % every === 0 ? h('text', { x: x + bar / 2, y: height - bottom + 16, class: 'axis mid' }, tick(category)) : null);
    })));
}

/**
 * Horizontal bars: one row per item, each split into `parts` (in order),
 * against a common maximum, the value written at the end of the row.
 * `total` (optional) draws a track the parts fill.
 */
export function bars({ label, items, parts, total, width = 640 }) {
  // Narrow: each label sits above its bar rather than beside it.
  const narrow = width < 480;
  const labelWidth = narrow ? 0 : Math.min(230, width * 0.3);
  const valueWidth = 64;
  const plot = width - labelWidth - valueWidth;
  const row = narrow ? ROW + 16 : ROW;
  const height = items.length * row + 4;
  const max = Math.max(1, ...items.map((item) => (total ? total(item) : parts.reduce((sum, p) => sum + p.value(item), 0))));
  return withTips(h('svg', { class: 'chart', viewBox: `0 0 ${width} ${height}`, role: 'img', 'aria-label': label },
    items.map((item, index) => {
      const y = index * row + (narrow ? 16 : 0);
      const barY = y + (ROW - BAR) / 2;
      let x = labelWidth;
      const marks = parts.map((part) => {
        const n = part.value(item);
        if (!n) return null;
        const w = Math.max(2, (plot * n) / max - GAP);
        const rect = h('rect', { x, y: barY, width: w, height: BAR, rx: 3, class: `mark ${part.cls}`, 'data-tip': `${item.name}: ${n} ${part.label}` });
        x += (plot * n) / max;
        return rect;
      });
      return h('g', { class: 'row' },
        h('rect', { x: 0, y: index * row, width, height: row, class: 'hit', 'data-tip': item.tip || `${item.name}: ${item.text}` }),
        narrow
          ? h('text', { x: 0, y: y - 3, class: 'axis' }, item.name)
          : h('text', { x: labelWidth - 10, y: y + ROW / 2 + 4, class: 'axis end' }, fit(item.name, labelWidth - 14)),
        total ? h('rect', { x: labelWidth, y: barY, width: (plot * total(item)) / max, height: BAR, rx: 3, class: 'track' }) : null,
        marks,
        h('text', { x: labelWidth + plot + 8, y: y + ROW / 2 + 4, class: 'axis value' }, item.text));
    })));
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
    h('div', { class: 'table-scroll', tabindex: '0', role: 'group', 'aria-label': caption }, h('table', { class: 'grid-table' },
      h('thead', {}, h('tr', {}, headings.map((title, i) => h('th', { scope: 'col', class: i ? 'num' : null }, title)))),
      h('tbody', {}, rows.map((cells) => h('tr', {}, cells.map((cell, i) => h('td', { class: i ? 'num' : null }, String(cell)))))))));
}
