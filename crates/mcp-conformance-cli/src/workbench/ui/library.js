// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// The library: every recording with its verdict and tags, the search box,
// and the actions on a selection (compare two, export, delete).

import { h, replace, api, badge, notice, enc, loading, emptyState, plural } from './dom.js';
import { importDialog, retentionDialog, confirmDialog } from './admin.js';

const FIELDS = [
  ['method:tools/call', 'sent or received that method'],
  ['tool:echo', 'called or listed that tool'],
  ['resource:test://static-text', 'read, subscribed to or listed that resource'],
  ['prompt:greeting', 'got or listed that prompt'],
  ['session:<id>', 'carried that Mcp-Session-Id, or is that entry'],
  ['code:-32602', 'holds an error response with that code'],
  ['req:BASE-003', 'has a finding against that requirement'],
  ['server:everything', 'was recorded against that server'],
  ['tag:nightly', 'carries that tag'],
  ['verdict:fail', 'has that verdict (pass, warn, fail, unsupported, unjudged)'],
  ['timeout', 'holds the text in a finding, a name, a tag or a value'],
];

function help() {
  return h('details', { class: 'help' },
    h('summary', {}, 'How to search'),
    h('p', {}, 'Terms are separated by spaces and all must match. ',
      h('code', {}, 'method:tools/*'), ' matches by prefix, ', h('code', {}, '-verdict:pass'),
      ' excludes, and ', h('code', {}, 'tool:"two words"'), ' quotes.'),
    h('dl', { class: 'fields' }, FIELDS.map(([term, meaning]) => [h('dt', {}, h('code', {}, term)), h('dd', {}, meaning)])));
}

function when(day) {
  return day ? h('span', { class: 'when' }, day) : h('span', { class: 'when none' }, 'undated');
}

function source(entry) {
  const kind = entry.source?.kind;
  if (kind === 'imported') return h('span', { class: 'pill src' }, `from ${entry.source.importer}`);
  if (kind === 'redacted') return h('span', { class: 'pill src' }, 'redacted copy');
  return null;
}

/** The column titles over the rows (hidden where the rows stack). */
function columns() {
  return h('li', { class: 'entries-head', 'aria-hidden': 'true' },
    h('span', {}), h('span', {}, 'Recording'), h('span', {}, 'Result'), h('span', {}, 'Server'),
    h('span', {}, 'Recorded'), h('span', { class: 'num' }, 'Events'), h('span', { class: 'num' }, 'Findings'));
}

function row(entry, hit, selected, onToggle) {
  const id = entry.id;
  const facts = entry.facts || {};
  const hits = (hit?.events || []).slice(0, 6);
  const unjudged = entry.verdict === 'unjudged';
  return h('li', { class: `entry${selected.has(id) ? ' selected' : ''}`, 'data-id': id },
    h('input', { type: 'checkbox', class: 'pick', 'aria-label': `Select ${id}`, checked: selected.has(id), on: { change: (e) => onToggle(id, e.target.checked) } }),
    h('div', { class: 'entry-main' },
      h('a', { class: 'entry-name', href: `#/s/${enc(id)}` }, id),
      entry.tags.length || source(entry) ? h('div', { class: 'tags' }, entry.tags.map((tag) => h('span', { class: 'tag' }, tag)), source(entry)) : null,
      hits.length ? h('div', { class: 'hits' }, 'Matched at ',
        hits.map((seq) => h('a', { href: `#/s/${enc(id)}/e${seq}` }, `#${seq}`))) : null),
    h('div', { class: 'entry-facts' },
      h('span', { class: 'entry-result' }, badge(entry.verdict, true),
        entry.revisions.map((revision) => h('span', { class: 'pill rev' }, revision))),
      h('span', {}, facts.server || 'server not recorded'),
      when(entry.day),
      h('span', { class: 'num' }, String(facts.events), h('span', { class: 'unit' }, facts.events === 1 ? ' event' : ' events'), h('span', { class: 'unit' }, ` · ${facts.transport}`)),
      h('span', { class: 'num' }, unjudged ? '—' : [String(entry.findings), h('span', { class: 'unit' }, entry.findings === 1 ? ' finding' : ' findings')])),
    unjudged ? h('p', { class: 'why' }, `Not judged: ${entry.unjudged}`) : null);
}

/** The quick filters: every recording, or those with one result. */
const FILTERS = [
  ['all', 'All', () => true],
  ['attention', 'Needs attention', (entry) => entry.verdict !== 'pass'],
  ['fail', 'Fail', (entry) => entry.verdict === 'fail'],
  ['pass-with-warnings', 'Warnings', (entry) => entry.verdict === 'pass-with-warnings'],
  ['pass', 'Pass', (entry) => entry.verdict === 'pass'],
];

export async function render(main, { q }) {
  const selected = new Set();
  let entries = new Map();
  let query = q;
  let filter = 'all';
  let hits = [];
  const status = h('div', { class: 'status-area' });
  const list = h('ol', { class: 'entries', 'aria-live': 'polite' }, loading('Reading the library…'));
  const count = h('p', { class: 'lede' });
  const chips = h('div', { class: 'chips', role: 'group', 'aria-label': 'Show recordings by result' });
  const compare = h('button', { type: 'button', disabled: true, title: 'Select exactly two recordings to compare them' }, 'Compare');
  const exportButton = h('button', { type: 'button', disabled: true }, 'Export');
  const remove = h('button', { type: 'button', class: 'danger', disabled: true }, 'Delete');
  const input = h('input', {
    type: 'search', class: 'query', value: query, 'aria-label': 'Search the library', spellcheck: 'false',
    placeholder: 'Search: a name, method:tools/call, req:BASE-003, code:-32602 …',
  });

  const refreshButtons = () => {
    compare.disabled = selected.size !== 2;
    exportButton.disabled = selected.size === 0;
    remove.disabled = selected.size === 0;
  };
  const toggle = (id, on) => {
    if (on) selected.add(id); else selected.delete(id);
    list.querySelector(`li[data-id="${CSS.escape(id)}"]`)?.classList.toggle('selected', on);
    refreshButtons();
  };
  const clearSearch = () => {
    input.value = '';
    filter = 'all';
    input.dispatchEvent(new Event('input'));
  };

  /** Draws the rows the search returned, narrowed by the chosen filter. */
  function draw() {
    const matches = hits.map((hit) => [hit, entries.get(hit.id)]);
    const test = FILTERS.find(([key]) => key === filter)[2];
    const shown = matches.filter(([, entry]) => test(entry));
    replace(chips, FILTERS.map(([key, label, fits]) => h('button', {
      type: 'button', class: 'chip', 'aria-pressed': key === filter ? 'true' : 'false',
      on: { click: () => { filter = key; draw(); } },
    }, label, h('span', { class: 'count' }, String(matches.filter(([, entry]) => fits(entry)).length)))));
    replace(count, query || filter !== 'all'
      ? `${shown.length} of ${entries.size} recordings match.`
      : plural(entries.size, 'recording') + '.');
    if (shown.length) {
      replace(list, columns(), shown.map(([hit, entry]) => row(entry, query ? hit : null, selected, toggle)));
    } else if (entries.size) {
      replace(list, emptyState('No recording matches', 'Nothing in the library fits this search and filter.',
        h('button', { type: 'button', on: { click: clearSearch } }, 'Clear the search'), 'li'));
    } else {
      replace(list, emptyState('The library is empty', 'Import a trace, an mcpsnoop capture or an Inspector export to see what a session did and whether it followed the protocol.',
        h('button', { type: 'button', class: 'primary', on: { click: () => importDialog(load) } }, 'Import a recording'), 'li'));
    }
    refreshButtons();
  }

  async function load() {
    const listing = await api('/api/library');
    entries = new Map(listing.sessions.map((entry) => [entry.id, entry]));
    for (const id of [...selected]) if (!entries.has(id)) selected.delete(id);
    replace(skipped, listing.skipped.length
      ? h('details', { class: 'skipped' }, h('summary', {}, `${plural(listing.skipped.length, 'file')} not opened`),
        h('ul', {}, listing.skipped.map((s) => h('li', {}, h('code', {}, s.file), ' — ', s.reason))))
      : null);
    replace(where, 'Directory ', h('code', {}, listing.library));
    await search();
  }

  async function search() {
    replace(status);
    const result = await api(`/api/search?q=${enc(query)}`);
    if (result.error) {
      replace(status, notice(result.error));
      replace(list);
      replace(count);
      replace(chips);
      return;
    }
    if (result.notes.length) replace(status, result.notes.map((note) => notice(note, 'note')));
    hits = result.hits.filter((hit) => entries.has(hit.id));
    // Without a query, the most recently recorded first.
    if (!query) hits.sort((a, b) => (entries.get(b.id).when || '').localeCompare(entries.get(a.id).when || '') || a.id.localeCompare(b.id));
    draw();
  }

  let timer = null;
  input.addEventListener('input', () => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      query = input.value.trim();
      history.replaceState(null, '', query ? `#/?q=${enc(query)}` : '#/');
      search().catch((error) => replace(status, notice(error.message)));
    }, 150);
  });
  compare.addEventListener('click', () => {
    const [a, b] = [...selected];
    location.hash = `#/diff/${enc(a)}/${enc(b)}`;
  });
  exportButton.addEventListener('click', () => {
    const link = h('a', { href: `/api/export?ids=${[...selected].map(enc).join(',')}`, download: 'mcp-library-export.tar' });
    document.body.appendChild(link);
    link.click();
    link.remove();
  });
  remove.addEventListener('click', () => {
    const ids = [...selected];
    confirmDialog(`Delete ${ids.length === 1 ? ids[0] : `${ids.length} recordings`}?`,
      'The trace and its sidecar are removed from the library directory. This cannot be undone.',
      'Delete', async () => {
        for (const id of ids) await api(`/api/sessions/${enc(id)}/delete`, {});
        selected.clear();
        await load();
      });
  });

  const where = h('p', { class: 'where' });
  const skipped = h('div');
  replace(main,
    h('div', { class: 'page-head' },
      h('div', {}, h('h1', {}, 'Library'),
        h('p', { class: 'page-sub' }, 'Every recorded session and whether it followed the protocol. Open one to see what happened, message by message.'),
        where),
      h('div', { class: 'actions' },
        h('button', { type: 'button', class: 'primary', on: { click: () => importDialog(load) } }, 'Import…'),
        h('button', { type: 'button', on: { click: () => retentionDialog(load) } }, 'Retention…'))),
    h('div', { class: 'searchbar' }, input, help()),
    status,
    h('div', { class: 'list-head' },
      h('div', { class: 'toolbar' }, chips, h('div', { class: 'actions' }, compare, exportButton, remove)),
      count),
    list,
    skipped);
  await load();
  return () => clearTimeout(timer);
}

/** A path within the library view: a new query from a link. */
export function focus({ q }) {
  const input = document.querySelector('input.query');
  if (input && input.value !== q) {
    input.value = q;
    input.dispatchEvent(new Event('input'));
  }
}
