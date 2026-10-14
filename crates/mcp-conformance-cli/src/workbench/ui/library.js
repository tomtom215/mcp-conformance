// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// The library: every recording with its verdict and tags, the search box,
// and the actions on a selection (compare two, export, delete).

import { h, replace, api, badge, notice, enc } from './dom.js';
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
    h('summary', {}, 'Query syntax'),
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

function row(entry, hit, selected, onToggle) {
  const id = entry.id;
  const facts = entry.facts || {};
  const hits = (hit?.events || []).slice(0, 6);
  return h('li', { class: `entry${selected.has(id) ? ' selected' : ''}`, 'data-id': id },
    h('input', { type: 'checkbox', class: 'pick', 'aria-label': `Select ${id}`, checked: selected.has(id), on: { change: (e) => onToggle(id, e.target.checked) } }),
    h('div', { class: 'entry-main' },
      h('div', { class: 'entry-line' },
        h('a', { class: 'entry-name', href: `#/s/${enc(id)}` }, id),
        badge(entry.verdict, true),
        entry.revisions.map((revision) => h('span', { class: 'pill rev' }, revision)),
        source(entry)),
      h('div', { class: 'entry-meta' },
        when(entry.day),
        h('span', {}, facts.server || 'server not recorded'),
        h('span', {}, `${facts.events} events`),
        h('span', {}, facts.transport),
        entry.verdict === 'unjudged'
          ? h('span', { class: 'why' }, entry.unjudged)
          : h('span', {}, `${entry.findings} ${entry.findings === 1 ? 'finding' : 'findings'}`)),
      entry.tags.length ? h('div', { class: 'tags' }, entry.tags.map((tag) => h('span', { class: 'tag' }, tag))) : null,
      hits.length ? h('div', { class: 'hits' }, 'Matched at ',
        hits.map((seq) => h('a', { href: `#/s/${enc(id)}/e${seq}` }, `#${seq}`))) : null));
}

export async function render(main, { q }) {
  const selected = new Set();
  let entries = new Map();
  let query = q;
  const status = h('div', { class: 'status-area' });
  const list = h('ol', { class: 'entries', 'aria-live': 'polite' });
  const count = h('p', { class: 'lede' });
  const compare = h('button', { type: 'button', disabled: true }, 'Compare');
  const exportButton = h('button', { type: 'button', disabled: true }, 'Export');
  const remove = h('button', { type: 'button', class: 'danger', disabled: true }, 'Delete');
  const input = h('input', {
    type: 'search', class: 'query', value: query, 'aria-label': 'Search the library', spellcheck: 'false',
    placeholder: 'method:tools/call req:BASE-003 code:-32602 timeout',
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

  async function load() {
    const listing = await api('/api/library');
    entries = new Map(listing.sessions.map((entry) => [entry.id, entry]));
    for (const id of [...selected]) if (!entries.has(id)) selected.delete(id);
    replace(skipped, listing.skipped.length
      ? h('details', { class: 'skipped' }, h('summary', {}, `${listing.skipped.length} files not opened`),
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
      return;
    }
    if (result.notes.length) replace(status, result.notes.map((note) => notice(note, 'note')));
    const shown = result.hits.filter((hit) => entries.has(hit.id));
    // Without a query, the most recently recorded first.
    if (!query) shown.sort((a, b) => (entries.get(b.id).when || '').localeCompare(entries.get(a.id).when || '') || a.id.localeCompare(b.id));
    replace(count, query
      ? `${shown.length} of ${entries.size} recordings match.`
      : `${entries.size} ${entries.size === 1 ? 'recording' : 'recordings'}.`);
    replace(list, shown.length
      ? shown.map((hit) => row(entries.get(hit.id), query ? hit : null, selected, toggle))
      : h('li', { class: 'empty' }, entries.size ? 'No recording matches.' : 'The library is empty: import a trace, an mcpsnoop capture or an Inspector export.'));
    refreshButtons();
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
      h('div', {}, h('p', { class: 'eyebrow' }, 'Recordings'), h('h1', {}, 'Library'), where),
      h('div', { class: 'actions' },
        h('button', { type: 'button', class: 'primary', on: { click: () => importDialog(load) } }, 'Import…'),
        h('button', { type: 'button', on: { click: () => retentionDialog(load) } }, 'Retention…'))),
    h('div', { class: 'searchbar' }, input, help()),
    status,
    h('div', { class: 'list-head' }, count, h('div', { class: 'actions' }, compare, exportButton, remove)),
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
