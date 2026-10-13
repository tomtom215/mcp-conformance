// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// One session: its verdict, where it came from, its findings beside its
// timeline (each finding linked to its event and back), the inspector, and
// following a trace that is still being written.

import { h, replace, api, badge, pill, notice, enc } from './dom.js';
import { eventItem } from './inspector.js';
import { redactDialog, tagsDialog, confirmDialog } from './admin.js';

const FOLLOW_MS = 1000;
let state = null;

function provenance(summary) {
  const source = summary.source || {};
  const rows = [];
  if (source.kind === 'imported') {
    rows.push(h('p', {}, 'Imported from ', h('strong', {}, source.importer), source.imported_from ? [' file ', h('code', {}, source.imported_from)] : null, '.'));
  } else if (source.kind === 'redacted') {
    rows.push(h('p', {}, 'A redacted copy of ', h('a', { href: `#/s/${enc(source.redacted_from)}` }, source.redacted_from), '.'));
    if (source.redactions?.length) rows.push(h('ul', { class: 'gaps' }, source.redactions.map((line) => h('li', {}, line))));
  }
  if (source.not_recorded?.length) {
    rows.push(h('p', { class: 'gaps-head' }, 'What the source does not record, so this trace cannot show (clauses that need it read not observed):'));
    rows.push(h('ul', { class: 'gaps' }, source.not_recorded.map((gap) => h('li', {}, gap))));
  }
  if (summary.sidecar_problem) rows.push(notice(summary.sidecar_problem, 'note'));
  return rows.length ? h('section', { class: 'panel provenance', 'aria-label': 'Provenance' }, rows) : null;
}

function verdictPanel(data) {
  const summary = data.summary;
  if (summary.verdict === 'unjudged') {
    return h('section', { class: 'panel verdict v-incomplete' },
      h('div', { class: 'verdict-line' }, badge('unjudged'), h('span', { class: 'verdict-why' }, summary.unjudged)));
  }
  return h('section', { class: `panel verdict v-${summary.verdict === 'pass-with-warnings' ? 'warn' : summary.verdict}` },
    data.totals.map((report) => h('div', { class: 'rev-totals' },
      h('div', { class: 'verdict-line' }, badge(report.verdict), h('span', { class: 'revision-name' }, report.revision)),
      h('dl', { class: 'tally' }, ['pass', 'fail', 'warn', 'unsupported', 'not_observed', 'not_applicable', 'excluded'].map((key) => {
        const value = report.totals[key];
        return h('div', { class: `t-${key.replace('_', '-')}${value ? '' : ' zero'}` }, h('dt', {}, key.replace('_', ' ')), h('dd', {}, String(value)));
      })))));
}

function findingItem(id, finding, selected) {
  return h('li', { class: `finding o-${finding.outcome}${selected ? ' selected' : ''}`, id: finding.key },
    h('div', { class: 'fhead' },
      h('a', { class: 'req', href: `#/s/${enc(id)}/${finding.key}` }, finding.requirement),
      pill(finding.outcome),
      h('span', { class: 'pill level' }, finding.level),
      h('span', { class: 'pill rev' }, finding.revision)),
    h('p', { class: 'detail' }, finding.detail),
    h('div', { class: 'fnav' },
      finding.seq !== null && finding.seq !== undefined
        ? h('a', { class: 'to-event', href: `#/s/${enc(id)}/e${finding.seq}` }, `Go to event #${finding.seq}`)
        : h('span', { class: 'none' }, 'not at one event'),
      h('span', { class: 'check' }, finding.check)),
    finding.quote ? h('details', { class: 'clause-quote' }, h('summary', {}, 'The clause'),
      h('blockquote', { class: 'quote' }, h('p', {}, finding.quote)),
      finding.url ? h('a', { class: 'spec-link', href: finding.url, target: '_blank' }, 'Specification') : null) : null);
}

function drawFindings() {
  const { id, data, at } = state;
  replace(state.findingsList, data.findings.length
    ? data.findings.map((finding) => findingItem(id, finding, at === finding.key))
    : h('li', { class: 'empty' }, data.summary.verdict === 'unjudged' ? 'Not judged, so no findings.' : 'No findings: every judged clause held.'));
  replace(state.findingsCount, String(data.findings.length));
}

function drawTimeline() {
  const { id, events, at } = state;
  replace(state.timeline, events.length
    ? events.map((event) => eventItem(id, event, at === `e${event.seq}`))
    : h('li', { class: 'empty' }, 'The trace holds no events.'));
  replace(state.eventCount, `${events.length} of ${state.data.total_events} events`);
  state.more.hidden = state.data.next === null || state.data.next === undefined;
}

function scrollTo(anchor) {
  const target = anchor && document.getElementById(anchor);
  if (target) target.scrollIntoView({ block: 'center' });
}

async function loadMore() {
  const page = await api(`/api/sessions/${enc(state.id)}?from=${state.events.length}`);
  state.events.push(...page.events);
  state.data.next = page.next;
  drawTimeline();
}

async function poll() {
  const page = await api(`/api/sessions/${enc(state.id)}?from=${state.events.length}`);
  const changed = page.events.length || page.findings.length !== state.data.findings.length || page.summary.sha256 !== state.data.summary.sha256;
  if (!changed) return;
  if (page.total_events < state.events.length) return render(state.main, { id: state.id, at: state.at });
  state.events.push(...page.events);
  state.data = { ...page, events: undefined };
  // A new finding can point at an event already shown: redraw it all.
  replace(state.verdict, verdictPanel(state.data));
  drawFindings();
  drawTimeline();
  replace(state.liveNote, `Following: ${page.total_events} events, last change ${new Date().toLocaleTimeString()}.`);
  if (state.followBottom) state.timeline.lastElementChild?.scrollIntoView({ block: 'end' });
}

function header(summary) {
  const facts = summary.facts || {};
  return h('div', { class: 'page-head session-head' },
    h('div', { class: 'head-main' },
      h('p', { class: 'eyebrow' }, h('a', { href: '#/' }, 'Library'), ' / Session'),
      h('h1', { class: 'trace-name' }, summary.id),
      h('p', { class: 'meta' },
        h('span', {}, 'Recorded ', h('strong', {}, summary.when || 'date unknown')),
        h('span', {}, 'Server ', h('strong', {}, facts.server || 'not recorded')),
        h('span', {}, 'Client ', h('strong', {}, facts.client || 'not recorded')),
        h('span', {}, 'Transport ', h('strong', {}, facts.transport)),
        h('span', {}, 'SHA-256 ', h('strong', { title: summary.sha256 }, summary.sha256.slice(0, 12)))),
      h('div', { class: 'tags' }, summary.tags.map((tag) => h('span', { class: 'tag' }, tag)),
        h('button', { type: 'button', class: 'link', on: { click: () => tagsDialog(summary.id, summary.tags, () => render(state.main, { id: summary.id, at: state.at })) } }, summary.tags.length ? 'Edit tags' : 'Add tags'))),
    h('div', { class: 'actions' },
      summary.verdict !== 'unjudged' ? h('a', { class: 'button', href: `/api/sessions/${enc(summary.id)}/report.html`, download: `${summary.id}.html` }, 'HTML report') : null,
      h('button', { type: 'button', on: { click: () => redactDialog(summary.id) } }, 'Redact…'),
      h('button', { type: 'button', class: 'danger', on: { click: () => confirmDialog(`Delete ${summary.id}?`, 'The trace and its sidecar are removed from the library directory. This cannot be undone.', 'Delete', async () => {
        await api(`/api/sessions/${enc(summary.id)}/delete`, {});
        location.hash = '#/';
      }) } }, 'Delete')));
}

async function compareControl(id) {
  const select = h('select', { 'aria-label': 'Compare with' }, h('option', { value: '' }, 'Compare with…'));
  select.addEventListener('change', () => {
    if (select.value) location.hash = `#/diff/${enc(id)}/${enc(select.value)}`;
  });
  api('/api/library').then((listing) => {
    for (const entry of listing.sessions) if (entry.id !== id) select.appendChild(h('option', { value: entry.id }, entry.id));
  }).catch(() => {});
  return select;
}

export async function render(main, { id, at }) {
  if (state?.timer) clearInterval(state.timer);
  const data = await api(`/api/sessions/${enc(id)}`);
  state = { main, id, at, data, events: data.events, timer: null, followBottom: true };
  state.findingsList = h('ol', { class: 'flist' });
  state.findingsCount = h('span', { class: 'count' });
  state.timeline = h('ol', { class: 'timeline' });
  state.eventCount = h('span', { class: 'lede' });
  state.more = h('button', { type: 'button', on: { click: () => loadMore().catch((e) => replace(state.liveNote, notice(e.message))) } }, 'Load more events');
  state.liveNote = h('span', { class: 'live-note', role: 'status' });
  state.verdict = h('div', {}, verdictPanel(data));
  const flaggedOnly = h('input', { type: 'checkbox', id: 'only-flagged' });
  flaggedOnly.addEventListener('change', () => state.timeline.classList.toggle('only-flagged', flaggedOnly.checked));
  const follow = h('input', { type: 'checkbox', id: 'follow' });
  follow.addEventListener('change', () => {
    clearInterval(state.timer);
    state.timer = follow.checked ? setInterval(() => poll().catch((e) => replace(state.liveNote, notice(e.message))), FOLLOW_MS) : null;
    replace(state.liveNote, follow.checked ? 'Following the trace file…' : '');
  });
  replace(main,
    header(data.summary),
    provenance(data.summary),
    state.verdict,
    h('div', { class: 'session-grid' },
      h('section', { class: 'findings-pane', 'aria-labelledby': 'findings-title' },
        h('h2', { id: 'findings-title' }, 'Findings ', state.findingsCount),
        state.findingsList),
      h('section', { class: 'timeline-pane', 'aria-labelledby': 'timeline-title' },
        h('div', { class: 'pane-head' },
          h('h2', { id: 'timeline-title' }, 'Timeline'),
          await compareControl(id)),
        h('div', { class: 'controls' },
          h('label', { class: 'check', for: 'only-flagged' }, flaggedOnly, 'Only events with findings'),
          h('label', { class: 'check', for: 'follow' }, follow, 'Follow live'),
          state.eventCount,
          state.liveNote),
        state.timeline,
        state.more)));
  drawFindings();
  drawTimeline();
  scrollTo(at);
  return () => clearInterval(state?.timer);
}

/** Moves within the session: opens an event, or highlights a finding. */
export function focus({ at }) {
  if (!state) return;
  state.at = at;
  drawFindings();
  drawTimeline();
  scrollTo(at);
}
