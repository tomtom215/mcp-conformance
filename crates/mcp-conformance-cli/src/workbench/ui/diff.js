// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// Two sessions side by side: the clauses that read differently, then the
// messages aligned by direction, method and id (the server's alignment).

import { h, replace, api, badge, pill, enc } from './dom.js';

const STATUS = { same: 'same', changed: 'changed', 'only-a': 'only in A', 'only-b': 'only in B' };

function side(label, summary) {
  const facts = summary.facts || {};
  return h('div', { class: 'panel side' },
    h('p', { class: 'eyebrow' }, label),
    h('h2', { class: 'trace-name' }, h('a', { href: `#/s/${enc(summary.id)}` }, summary.id)),
    h('div', { class: 'verdict-line' }, badge(summary.verdict),
      summary.revisions.map((revision) => h('span', { class: 'pill rev' }, revision))),
    h('p', { class: 'meta' },
      h('span', {}, 'Server ', h('strong', {}, facts.server || 'not recorded')),
      h('span', {}, h('strong', {}, String(facts.messages)), ' messages'),
      h('span', {}, h('strong', {}, String(summary.findings)), ' findings')));
}

function message(id, m, label) {
  const tag = h('span', { class: 'side-tag' }, label);
  if (!m) return h('div', { class: 'cell empty-cell' }, tag, '—');
  return h('div', { class: `cell ${m.direction}` }, tag,
    h('a', { class: 'ev-seq', href: `#/s/${enc(id)}/e${m.seq}` }, String(m.seq)),
    h('span', { class: 'ev-dir' }, m.direction === 'c2s' ? 'c → s' : 's → c'),
    h('span', { class: `kind k-${m.kind === 'error' ? 'error' : m.kind}` }, m.kind),
    m.method ? h('code', { class: 'method' }, m.method) : null,
    m.id !== null && m.id !== undefined ? h('span', { class: 'id' }, 'id ', h('code', {}, m.id)) : null);
}

function clauses(diff) {
  if (!diff.clauses.length) {
    return h('p', { class: 'empty' }, 'Every clause reads the same in both, at every revision both were judged at.');
  }
  return h('div', { class: 'table-scroll' }, h('table', { class: 'grid-table' },
    h('thead', {}, h('tr', {}, ['Revision', 'Clause', 'A', 'B', 'Findings A → B'].map((title) => h('th', { scope: 'col' }, title)))),
    h('tbody', {}, diff.clauses.map((c) => h('tr', {},
      h('td', {}, h('code', {}, c.revision)),
      h('td', {}, h('code', {}, c.id)),
      h('td', {}, pill(c.a)),
      h('td', {}, pill(c.b)),
      h('td', { class: 'num' }, `${c.a_findings} → ${c.b_findings}`))))));
}

export async function render(main, { a, b }) {
  const diff = await api(`/api/diff?a=${enc(a)}&b=${enc(b)}`);
  const counts = { same: 0, changed: 0, 'only-a': 0, 'only-b': 0 };
  for (const row of diff.rows) counts[row.status] += 1;
  const rows = h('ol', { class: 'diff-rows' }, diff.rows.map((row) => h('li', { class: `drow d-${row.status}` },
    message(a, row.a, 'A'),
    h('span', { class: 'dmark', title: STATUS[row.status] }, STATUS[row.status]),
    message(b, row.b, 'B'),
    row.payloads ? h('details', { class: 'dpayloads' }, h('summary', {}, 'Payloads'),
      h('div', { class: 'pair-pre' }, h('pre', {}, h('code', {}, row.payloads[0])), h('pre', {}, h('code', {}, row.payloads[1])))) : null)));
  const only = h('input', { type: 'checkbox', id: 'only-diff' });
  only.addEventListener('change', () => rows.classList.toggle('only-diff', only.checked));
  replace(main,
    h('div', { class: 'page-head' },
      h('div', {}, h('p', { class: 'eyebrow' }, h('a', { href: '#/' }, 'Library'), ' / Compare'), h('h1', {}, 'Two sessions')),
      h('div', { class: 'actions' }, h('a', { class: 'button', href: `#/diff/${enc(b)}/${enc(a)}` }, 'Swap A and B'))),
    h('div', { class: 'sides' }, side('A', diff.a), side('B', diff.b)),
    h('section', { class: 'section' }, h('h2', {}, 'Clauses that read differently ', h('span', { class: 'count' }, String(diff.clauses.length))),
      clauses(diff),
      diff.revisions_only_a.length || diff.revisions_only_b.length
        ? h('p', { class: 'lede' }, 'Judged at one side only: ', [...diff.revisions_only_a.map((r) => `${r} (A)`), ...diff.revisions_only_b.map((r) => `${r} (B)`)].join(', '))
        : null),
    h('section', { class: 'section' }, h('h2', {}, 'Messages'),
      h('p', { class: 'lede' }, `${counts.same} the same, ${counts.changed} changed, ${counts['only-a']} only in A, ${counts['only-b']} only in B${diff.truncated ? ' (the first 2000 messages of each side compared)' : ''}.`),
      h('label', { class: 'check', for: 'only-diff' }, only, 'Only differences'),
      rows));
}
