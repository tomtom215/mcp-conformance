// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// The library's numbers. Every one comes from /api/stats, which is exactly
// what `mcp-conformance library stats --format json` prints for the same
// library, so each chart says which member of that output it draws.

import { h, replace, api } from './dom.js';
import { stackedColumns, bars, legend, table } from './charts.js';

const VERDICTS = [
  { key: 'pass', label: 'pass', cls: 's-pass' },
  { key: 'pass-with-warnings', label: 'warnings', cls: 's-warn' },
  { key: 'fail', label: 'fail', cls: 's-fail' },
  { key: 'unsupported', label: 'unsupported', cls: 's-unsupported' },
  { key: 'unjudged', label: 'unjudged', cls: 's-unjudged' },
];

function figure(title, member, chart, extra) {
  return h('figure', { class: 'panel chart-panel' },
    h('figcaption', {}, h('h2', {}, title), h('p', { class: 'repro' }, 'Reproduce: ', h('code', {}, `library stats --format json`), ' → ', h('code', {}, member))),
    chart,
    extra);
}

function tiles(stats) {
  const tile = (label, value, cls = '') => h('div', { class: `tile ${cls}` }, h('dt', {}, label), h('dd', {}, String(value)));
  return h('dl', { class: 'tiles' },
    tile('Recordings', stats.sessions),
    VERDICTS.map((v) => tile(v.label, stats.verdicts[v.key], `t-${v.key}${stats.verdicts[v.key] ? '' : ' zero'}`)));
}

function draw(main, stats, library) {
  const width = Math.max(300, Math.min(900, main.querySelector('.dash')?.clientWidth || main.clientWidth) - 40);
  const days = stats.over_time.map((d) => d.day);
  const overTime = stats.over_time.length
    ? stackedColumns({ label: 'Verdicts by day recorded', categories: days, series: VERDICTS, value: (day, key) => stats.over_time.find((d) => d.day === day).verdicts[key], width })
    : h('p', { class: 'empty' }, 'No recording has a date.');
  const coverage = stats.coverage.map((rev) => figure(`Clause coverage, ${rev.revision}`, `.coverage[] | select(.revision == "${rev.revision}")`,
    [h('p', { class: 'lede' }, `${rev.exercised} of ${rev.clauses} clauses judged by ${rev.sessions === 1 ? 'the one recording at this revision' : `at least one of ${rev.sessions} recordings`}; ${rev.excluded} the registry excludes from judging; the rest no recording has shown.`),
      bars({
        label: `Clause coverage by area, ${rev.revision}`,
        width,
        items: rev.areas.map((a) => ({ name: a.area, text: `${a.exercised}/${a.clauses}`, ...a })),
        total: (a) => a.clauses,
        parts: [
          { label: 'judged by some recording', cls: 's-covered', value: (a) => a.exercised },
          { label: 'excluded from judging', cls: 's-excluded', value: (a) => a.excluded },
        ],
      }),
      legend([{ label: 'judged by some recording', cls: 's-covered' }, { label: 'excluded from judging', cls: 's-excluded' }, { label: 'never observed', cls: 's-track' }])],
    table('The numbers', ['Area', 'Clauses', 'Judged', 'Excluded'], rev.areas.map((a) => [a.area, a.clauses, a.exercised, a.excluded]))));
  replace(main.querySelector('.dash'),
    tiles(stats),
    figure('Verdicts over time', '.over_time', [overTime, legend(VERDICTS)],
      [stats.undated ? h('p', { class: 'lede' }, `${stats.undated} recordings have no date and are not drawn.`) : null,
        table('The numbers', ['Day', ...VERDICTS.map((v) => v.label)], stats.over_time.map((d) => [d.day, ...VERDICTS.map((v) => d.verdicts[v.key])]))]),
    coverage,
    figure('Findings by area', '.findings_by_area',
      stats.findings_by_area.length
        ? bars({ label: 'Findings by area', width, items: stats.findings_by_area.map((a) => ({ name: a.area, text: String(a.findings), ...a })), parts: [{ label: 'findings', cls: 's-fail', value: (a) => a.findings }] })
        : h('p', { class: 'empty' }, 'No findings in the library.'),
      table('The numbers', ['Area', 'Findings', 'Recordings'], stats.findings_by_area.map((a) => [a.area, a.findings, a.sessions]))),
    figure('Findings by server', '.findings_by_server',
      [bars({ label: 'Findings by server', width, items: stats.findings_by_server.map((s) => ({ name: s.server, text: String(s.findings), ...s })), parts: [{ label: 'findings', cls: 's-fail', value: (s) => s.findings }] })],
      table('The numbers', ['Server', 'Recordings', 'Failing', 'Findings'], stats.findings_by_server.map((s) => [s.server, s.sessions, s.failing, s.findings]))),
    h('p', { class: 'lede' }, 'All of it: ', h('code', {}, `mcp-conformance library stats --library ${library} --format json`), ` (mcp-conformance ${stats.tool_version}).`));
}

export async function render(main) {
  const [stats, listing] = await Promise.all([api('/api/stats'), api('/api/library')]);
  replace(main,
    h('div', { class: 'page-head' }, h('div', {}, h('p', { class: 'eyebrow' }, 'The library in numbers'), h('h1', {}, 'Dashboards'))),
    h('div', { class: 'dash' }));
  draw(main, stats, listing.library);
  let timer = null;
  let last = window.innerWidth;
  const resize = () => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      if (Math.abs(window.innerWidth - last) > 40) {
        last = window.innerWidth;
        draw(main, stats, listing.library);
      }
    }, 150);
  };
  window.addEventListener('resize', resize);
  return () => window.removeEventListener('resize', resize);
}
