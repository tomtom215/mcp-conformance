// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// The library's numbers. Every one comes from /api/stats, which is exactly
// what `mcp-conformance library stats --format json` prints for the same
// library, so each chart says which member of that output it draws.

import { h, replace, api, emptyState, plural } from './dom.js';
import { stackedColumns, bars, legend, table, hideTip } from './charts.js';

const VERDICTS = [
  { key: 'pass', label: 'pass', cls: 's-pass' },
  { key: 'pass-with-warnings', label: 'warnings', cls: 's-warn' },
  { key: 'fail', label: 'fail', cls: 's-fail' },
  { key: 'unsupported', label: 'unsupported', cls: 's-unsupported' },
  { key: 'unjudged', label: 'unjudged', cls: 's-unjudged' },
];

// The areas of the protocol by the prefix of their requirement IDs.
const AREAS = {
  AUTH: 'Authorization', BASE: 'Base protocol', CACH: 'Caching', CANC: 'Cancellation', COMP: 'Completion',
  DISC: 'Discovery', ELIC: 'Elicitation', LIFE: 'Lifecycle', LOG: 'Logging', MRTR: 'Multi round-trip requests',
  PAGE: 'Pagination', PING: 'Ping', PROG: 'Progress', PROM: 'Prompts', RES: 'Resources', ROOT: 'Roots',
  SAMP: 'Sampling', SUBS: 'Subscriptions', TASK: 'Tasks', TOOL: 'Tools', TRAN: 'Transports', VERS: 'Versioning',
  'wire-schema': 'Wire schema',
};
const areaName = (area) => AREAS[area] || area;
const areaCell = (area) => (AREAS[area] && AREAS[area] !== area ? `${AREAS[area]} (${area})` : area);

/** One titled chart: the question it answers, the drawing, then the numbers and where they come from. */
function figure(title, about, member, chart, extra) {
  return h('figure', { class: 'panel chart-panel' },
    h('figcaption', {}, h('h2', {}, title), h('p', { class: 'chart-sub' }, about)),
    chart,
    extra,
    h('p', { class: 'repro' }, 'Same numbers from the command line: ', h('code', {}, 'library stats --format json'), ' → ', h('code', {}, member)));
}

function tiles(stats) {
  const tile = (label, value, cls = '', swatch = null) => h('div', { class: `tile ${cls}` },
    h('dt', {}, swatch ? h('span', { class: `swatch ${swatch}`, 'aria-hidden': 'true' }) : null, label), h('dd', {}, String(value)));
  return h('dl', { class: 'tiles' },
    tile('Recordings', stats.sessions, 'lead'),
    VERDICTS.map((v) => tile(v.label, stats.verdicts[v.key], stats.verdicts[v.key] ? '' : 'zero', v.cls)));
}

function coverage(rev, width) {
  return figure(`How much of revision ${rev.revision} your recordings exercise`,
    `${rev.exercised} of ${rev.clauses} rules were checked by ${rev.sessions === 1 ? 'the one recording at this revision' : `at least one of ${rev.sessions} recordings`}. ${rev.excluded} cannot be judged from a recording at all; the rest no recording has exercised yet.`,
    `.coverage[] | select(.revision == "${rev.revision}")`,
    [bars({
      label: `Rules checked by area, revision ${rev.revision}`,
      width,
      items: rev.areas.map((a) => ({ name: areaName(a.area), text: `${a.exercised}/${a.clauses}`, tip: `${areaCell(a.area)}: ${a.exercised} of ${plural(a.clauses, 'rule')} checked, ${a.excluded} not judged from recordings`, ...a })),
      total: (a) => a.clauses,
      parts: [
        { label: 'checked by some recording', cls: 's-covered', value: (a) => a.exercised },
        { label: 'not judged from recordings', cls: 's-set-aside', value: (a) => a.excluded },
      ],
    }),
    legend([{ label: 'checked by some recording', cls: 's-covered' }, { label: 'not judged from recordings', cls: 's-set-aside' }, { label: 'not exercised yet', cls: 's-track' }])],
    table('Show the numbers', ['Area', 'Rules', 'Checked', 'Not judged'], rev.areas.map((a) => [areaCell(a.area), a.clauses, a.exercised, a.excluded])));
}

function draw(main, stats, library) {
  const dash = main.querySelector('.dash');
  const width = Math.max(300, Math.min(1100, dash?.clientWidth || main.clientWidth) - 42);
  if (!stats.sessions) {
    replace(dash, emptyState('Nothing to chart yet', 'The library holds no recording. Import one from the Library page and its numbers appear here.',
      h('a', { class: 'button', href: '#/' }, 'Go to the library')));
    return;
  }
  const days = stats.over_time.map((d) => d.day);
  const overTime = stats.over_time.length
    ? stackedColumns({ label: 'Recordings by day recorded, split by result', axis: 'RECORDINGS', categories: days, series: VERDICTS, tick: (day) => day.slice(5), value: (day, key) => stats.over_time.find((d) => d.day === day).verdicts[key], width })
    : h('p', { class: 'empty' }, 'No recording has a date.');
  replace(dash,
    tiles(stats),
    figure('Results over time', 'Recordings by the day they were made, split by how each came out. Days are month-day.', '.over_time', [overTime, legend(VERDICTS)],
      [stats.undated ? h('p', { class: 'chart-sub' }, `${plural(stats.undated, 'recording has', 'recordings have')} no date and ${stats.undated === 1 ? 'is' : 'are'} not drawn.`) : null,
        table('Show the numbers', ['Day', ...VERDICTS.map((v) => v.label)], stats.over_time.map((d) => [d.day, ...VERDICTS.map((v) => d.verdicts[v.key])]))]),
    stats.coverage.map((rev) => coverage(rev, width)),
    figure('Where the findings are', 'Findings across the whole library, by the part of the protocol the broken rule belongs to.', '.findings_by_area',
      stats.findings_by_area.length
        ? bars({ label: 'Findings by area', width, items: stats.findings_by_area.map((a) => ({ name: areaName(a.area), text: String(a.findings), tip: `${areaCell(a.area)}: ${plural(a.findings, 'finding')} in ${plural(a.sessions, 'recording')}`, ...a })), parts: [{ label: 'findings', cls: 's-count', value: (a) => a.findings }] })
        : h('p', { class: 'empty' }, 'No findings in the library.'),
      table('Show the numbers', ['Area', 'Findings', 'Recordings'], stats.findings_by_area.map((a) => [areaCell(a.area), a.findings, a.sessions]))),
    figure('Findings by server', 'Which servers the findings were recorded against.', '.findings_by_server',
      [bars({ label: 'Findings by server', width, items: stats.findings_by_server.map((s) => ({ name: s.server, text: String(s.findings), tip: `${s.server}: ${plural(s.findings, 'finding')} in ${plural(s.sessions, 'recording')}, ${s.failing} failing`, ...s })), parts: [{ label: 'findings', cls: 's-count', value: (s) => s.findings }] })],
      table('Show the numbers', ['Server', 'Recordings', 'Failing', 'Findings'], stats.findings_by_server.map((s) => [s.server, s.sessions, s.failing, s.findings]))),
    h('p', { class: 'lede' }, 'All of it: ', h('code', {}, `mcp-conformance library stats --library ${library} --format json`), ` (mcp-conformance ${stats.tool_version}).`));
}

export async function render(main) {
  const [stats, listing] = await Promise.all([api('/api/stats'), api('/api/library')]);
  replace(main,
    h('div', { class: 'page-head' }, h('div', {}, h('h1', {}, 'Dashboards'),
      h('p', { class: 'page-sub' }, 'The whole library in numbers: how recordings came out, how much of the protocol they exercise, and where the findings are.'))),
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
  return () => {
    window.removeEventListener('resize', resize);
    hideTip();
  };
}
