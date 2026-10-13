// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// One event on the timeline, and the inspector it opens to. The markup is the
// HTML report's own (.ev, .ev-line, .marks, .payload, .headers), styled by its
// stylesheet, so a session reads the same here as in a report.

import { h, enc } from './dom.js';

const ARROWS = { c2s: 'client → server', s2c: 'server → client' };

function kind(event) {
  switch (event.kind) {
    case 'request': return [h('span', { class: 'kind k-request' }, 'request'), h('code', { class: 'method' }, event.method)];
    case 'notification': return [h('span', { class: 'kind k-note' }, 'notification'), h('code', { class: 'method' }, event.method)];
    case 'result': return [h('span', { class: 'kind k-result' }, 'result'), event.method ? h('code', { class: 'method' }, event.method) : null];
    case 'error': return [h('span', { class: 'kind k-error' }, event.error), event.method ? h('code', { class: 'method' }, event.method) : null];
    case 'batch': return h('span', { class: 'kind k-other' }, `batch of ${event.batch}`);
    case 'malformed': return h('span', { class: 'kind k-other' }, 'malformed message');
    case 'http':
      if (event.http_method) return [h('span', { class: 'kind k-http' }, 'HTTP request'), h('code', { class: 'method' }, event.http_method)];
      if (event.status !== null && event.status !== undefined) return [h('span', { class: 'kind k-http' }, 'HTTP response'), h('code', { class: 'method' }, String(event.status))];
      return h('span', { class: 'kind k-http' }, 'HTTP request, method not recorded');
    case 'lifecycle': return h('span', { class: 'kind k-life' }, event.lifecycle);
    default: return h('span', { class: 'kind' }, 'event');
  }
}

function pairing(event, link) {
  const parts = [];
  if (event.id !== null && event.id !== undefined) parts.push(h('span', { class: 'id' }, 'id ', h('code', {}, event.id)));
  if (event.kind === 'request') {
    parts.push(event.answered_by !== null && event.answered_by !== undefined
      ? h('span', { class: 'pair' }, 'answered at ', link(event.answered_by))
      : h('span', { class: 'pair none' }, 'no answer recorded'));
  } else if (event.kind === 'result' || event.kind === 'error') {
    parts.push(event.answers !== null && event.answers !== undefined
      ? h('span', { class: 'pair' }, 'answers ', link(event.answers))
      : h('span', { class: 'pair none' }, 'answers no open request'));
  }
  if (event.latency_ms !== undefined) parts.push(h('span', { class: 'lat', title: 'time from request to answer' }, `${event.latency_ms} ms`));
  return parts;
}

/** The inspector: everything the event holds. */
export function inspect(event) {
  const parts = [];
  if (event.payload !== undefined) {
    parts.push(h('div', { class: 'payload' },
      h('p', { class: 'insp-label' }, 'Payload'),
      h('pre', {}, h('code', {}, event.payload)),
      event.payload_more_bytes > 0
        ? h('p', { class: 'truncated' }, `… ${event.payload_more_bytes} more bytes; the trace line holds the whole message.`)
        : null));
  }
  if (event.headers && Object.keys(event.headers).length) {
    parts.push(h('div', { class: 'payload' },
      h('p', { class: 'insp-label' }, `Headers (${Object.keys(event.headers).length})`),
      h('dl', { class: 'headers' }, Object.entries(event.headers).map(([name, value]) => [h('dt', {}, name), h('dd', {}, value)]))));
  }
  if (event.kind === 'http' && !(event.headers && Object.keys(event.headers).length)) {
    parts.push(h('p', { class: 'truncated' }, 'No header was recorded for this exchange.'));
  }
  if (event.ts) parts.push(h('p', { class: 'truncated' }, `Recorded at ${event.ts}`));
  return h('div', { class: 'inspector' }, parts);
}

/** One timeline item. `selected` opens its inspector. */
export function eventItem(id, event, selected) {
  const link = (seq) => h('a', { href: `#/s/${enc(id)}/e${seq}` }, `#${seq}`);
  const flagged = event.marks.length > 0;
  const toggle = selected ? `#/s/${enc(id)}` : `#/s/${enc(id)}/e${event.seq}`;
  return h('li', {
    class: `ev ${event.direction}${flagged ? ' flagged' : ''}${selected ? ' selected' : ''} k-${event.kind}`,
    id: `e${event.seq}`, 'data-seq': event.seq,
  },
  h('div', { class: 'ev-line' },
    h('a', { class: 'ev-seq', href: toggle, 'aria-label': `event ${event.seq}`, 'aria-expanded': selected ? 'true' : 'false' }, String(event.seq)),
    h('span', { class: 'ev-dir' }, ARROWS[event.direction]),
    kind(event),
    pairing(event, link),
    h('a', { class: 'open', href: toggle }, selected ? 'Close' : 'Inspect')),
  flagged
    ? h('ul', { class: 'marks' }, event.marks.map((mark) => h('li', { class: `o-${mark.outcome}` },
      h('a', { href: `#/s/${enc(id)}/${mark.key}` }, mark.requirement),
      h('span', { class: 'detail' }, mark.detail))))
    : null,
  selected ? inspect(event) : null);
}
