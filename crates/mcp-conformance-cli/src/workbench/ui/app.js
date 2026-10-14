// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// The router: the address bar's fragment names the view, so every view can be
// linked to, reloaded and gone back to.
//
//   #/                      the library (#/?q=… with a search)
//   #/s/<id>[/e<seq>|/f<n>] a session, at an event or a finding
//   #/diff/<a>/<b>          two sessions compared
//   #/dashboards            the library's numbers

import { replace, notice } from './dom.js';
import * as library from './library.js';
import * as session from './session.js';
import * as diff from './diff.js';
import * as dashboard from './dashboard.js';

const main = document.getElementById('main');
let current = { key: null, view: null, cleanup: null };

function parse(hash) {
  const [path, search = ''] = hash.replace(/^#/, '').split('?');
  const parts = path.split('/').filter(Boolean).map(decodeURIComponent);
  const params = new URLSearchParams(search);
  if (parts[0] === 's' && parts[1]) return { view: session, key: `s/${parts[1]}`, args: { id: parts[1], at: parts[2] || null } };
  if (parts[0] === 'diff' && parts[1] && parts[2]) return { view: diff, key: `diff/${parts[1]}/${parts[2]}`, args: { a: parts[1], b: parts[2] } };
  if (parts[0] === 'dashboards') return { view: dashboard, key: 'dashboards', args: {} };
  return { view: library, key: 'library', args: { q: params.get('q') || '' } };
}

async function route() {
  if (location.hash && !location.hash.startsWith('#/')) return; // an in-page anchor
  const next = parse(location.hash || '#/');
  for (const tab of document.querySelectorAll('.tabs a')) {
    const active = (tab.dataset.tab === 'dashboards') === (next.view === dashboard);
    if (active) tab.setAttribute('aria-current', 'page');
    else tab.removeAttribute('aria-current');
  }
  // Moving within one session (to an event or a finding) does not reload it.
  if (current.key === next.key && current.view?.focus) {
    current.view.focus(next.args);
    return;
  }
  if (current.cleanup) current.cleanup();
  current = { key: next.key, view: next.view, cleanup: null };
  try {
    current.cleanup = (await next.view.render(main, next.args)) || null;
  } catch (error) {
    replace(main, notice(`This view could not be shown: ${error.message}`));
  }
}

window.addEventListener('hashchange', route);
route();
