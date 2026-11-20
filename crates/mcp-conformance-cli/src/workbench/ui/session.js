// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// One session: its verdict, where it came from, its findings beside its
// timeline (each finding linked to its event and back), the inspector, and
// following a trace that is still being written.

import { h, replace, api, badge, pill, notice, enc, stackedBar, emptyState, plural } from './dom.js';
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

const GLYPHS = { pass: '✓', 'pass-with-warnings': '!', fail: '✕' };
const TONES = { 'pass-with-warnings': 'warn', unjudged: 'incomplete' };

/** What the verdict means, in a sentence a newcomer can act on. */
function headline(data) {
  const { summary, totals } = data;
  if (summary.verdict === 'unjudged') return 'This recording could not be judged, so it has no result.';
  if (totals.length > 1) return `Judged at ${totals.length} protocol revisions; the result shown is the most severe of them.`;
  const t = totals[0]?.totals || {};
  switch (summary.verdict) {
    case 'fail':
      return t.fail
        ? `This session broke ${plural(t.fail, 'MUST-level rule')}${t.warn ? ` and did not follow ${plural(t.warn, 'recommendation')}` : ''}.`
        : 'This session sent messages that do not match the protocol\'s schema.';
    case 'pass-with-warnings':
      return `No required rule was broken, but ${plural(t.warn, 'recommendation was', 'recommendations were')} not followed.`;
    case 'pass':
      return `No rule was broken: all ${t.pass} rules this session could be checked against held.`;
    default:
      return `${plural(t.unsupported, 'rule')} could not be checked by this version of the tool, so the result is not a pass.`;
  }
}

/** The line under the headline: where to start. */
function nextStep(data) {
  const { summary, findings } = data;
  const label = h('strong', {}, 'What to do next');
  if (summary.verdict === 'unjudged') return h('p', { class: 'next' }, label, summary.unjudged);
  if (!findings.length) return h('p', { class: 'next' }, label, 'Nothing to fix. A pass covers only what this session did; the counts below say how many rules never came up in it.');
  const first = findings[0];
  return h('p', { class: 'next' }, label, 'Start with ',
    h('a', { href: `#/s/${enc(summary.id)}/${first.key}` }, first.requirement),
    ': it says what was seen and quotes the rule, and “Go to event” opens the exact message.');
}

/** One revision's counts: two bars, then every number by name. */
function counts(report) {
  const t = report.totals;
  const checked = t.pass + t.fail + t.warn;
  const unchecked = t.not_observed + t.not_applicable + t.excluded + t.unsupported;
  const ORDER = ['fail', 'warn', 'pass', 'unsupported', 'not_observed', 'not_applicable', 'excluded'];
  return [
    h('div', { class: 'breakdown' },
      h('div', { class: 'bk-row' },
        h('p', { class: 'bk-label' }, h('strong', {}, plural(checked, 'rule')), ' checked against this session'),
        stackedBar([['fail', t.fail], ['warn', t.warn], ['pass', t.pass]])),
      h('div', { class: 'bk-row' },
        h('p', { class: 'bk-label' }, h('strong', {}, plural(unchecked, 'rule')), ' not checked here; the counts below say why'),
        stackedBar([['unsupported', t.unsupported], ['not-observed', t.not_observed], ['not-applicable', t.not_applicable], ['excluded', t.excluded]]))),
    h('dl', { class: 'tally' }, ORDER.map((key) => {
      const value = t[key];
      return h('div', { class: `t-${key.replace('_', '-')}${value ? '' : ' zero'}` }, h('dt', {}, key.replace('_', ' ')), h('dd', {}, String(value)));
    })),
  ];
}

function verdictPanel(data) {
  const summary = data.summary;
  const tone = TONES[summary.verdict] || summary.verdict;
  const several = data.totals.length > 1;
  return h('section', { class: `panel verdict v-${tone}`, 'aria-label': 'Result' },
    h('div', { class: 'hero' },
      h('span', { class: 'hero-icon glyph', 'aria-hidden': 'true' }, GLYPHS[summary.verdict] || '?'),
      h('div', { class: 'hero-body' },
        h('p', { class: 'verdict-line' }, badge(summary.verdict)),
        h('p', { class: 'headline' }, headline(data)),
        nextStep(data))),
    summary.verdict === 'unjudged' ? null : data.totals.map((report) => h('div', { class: 'rev-totals' },
      several ? h('p', { class: 'verdict-line' }, h('span', { class: 'revision-name' }, report.revision), badge(report.verdict, true)) : null,
      counts(report))));
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
    finding.quote ? h('details', { class: 'clause-quote' }, h('summary', {}, 'The rule it is held to'),
      h('blockquote', { class: 'quote' }, h('p', {}, finding.quote)),
      finding.url ? h('a', { class: 'spec-link', href: finding.url, target: '_blank' }, 'Read it in the specification') : null) : null);
}

function drawFindings() {
  const { id, data, at } = state;
  replace(state.findingsList, data.findings.length
    ? data.findings.map((finding) => findingItem(id, finding, at === finding.key))
    : data.summary.verdict === 'unjudged'
      ? emptyState('No findings', 'This recording was not judged.', null, 'li')
      : emptyState('No findings', 'Every rule this session could be checked against held.', null, 'li'));
  replace(state.findingsCount, String(data.findings.length));
}

function drawTimeline() {
  const { id, events, at } = state;
  replace(state.timeline, events.length
    ? events.map((event) => eventItem(id, event, at === `e${event.seq}`))
    : emptyState('No events', 'The trace holds no events.', null, 'li'));
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

function fact(label, value, title = null) {
  return h('div', {}, h('dt', {}, label), h('dd', { title }, value));
}

function header(summary) {
  const facts = summary.facts || {};
  return h('div', { class: 'page-head session-head' },
    h('div', { class: 'head-main' },
      h('p', { class: 'eyebrow' }, h('a', { href: '#/' }, 'Library'), ' / Session'),
      h('h1', { class: 'trace-name' }, summary.id),
      h('dl', { class: 'facts' },
        fact('Recorded', summary.when || 'date unknown'),
        fact('Server', facts.server || 'not recorded'),
        fact('Client', facts.client || 'not recorded'),
        fact('Transport', facts.transport),
        fact('SHA-256', summary.sha256.slice(0, 12), summary.sha256)),
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
