// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// The one way anything reaches the page. Every string becomes a text node,
// never markup: a recording's method name, id, header or payload is shown as
// the characters it holds, whatever they are. Control characters and the
// Unicode bidirectional controls are shown as \u{…} escapes rather than
// obeyed — the same set the HTML report's escaper shows
// (mcp-trace-validator, src/html/escape.rs), since U+202E can make a method
// name read backwards (CVE-2021-42574).

const SHOWN_ESCAPED = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f‎‏‪-‮⁦-⁩]/gu;

/** `text` with control and bidirectional characters made visible. */
export function visible(text) {
  return String(text).replace(SHOWN_ESCAPED, (c) => `\\u{${c.codePointAt(0).toString(16)}}`);
}

const SVG = 'http://www.w3.org/2000/svg';
const SVG_TAGS = new Set(['svg', 'g', 'rect', 'line', 'text', 'title', 'path', 'circle', 'tspan', 'pattern', 'defs']);

/** Only links the workbench makes itself, or to the published specification. */
function safeHref(value) {
  const href = String(value);
  if (href.startsWith('#/') || href.startsWith('/api/')) return href;
  if (/^https?:\/\//i.test(href)) return href;
  return null;
}

/**
 * Builds an element. `props`: attributes by name, except `on` (event
 * listeners by name), `class`, and `href` (internal or http(s) only).
 * Children: strings and numbers become text nodes; nodes are appended;
 * arrays are flattened; null, undefined and false are skipped.
 */
export function h(tag, props = {}, ...children) {
  const node = SVG_TAGS.has(tag) ? document.createElementNS(SVG, tag) : document.createElement(tag);
  for (const [name, value] of Object.entries(props || {})) {
    if (value === null || value === undefined || value === false) continue;
    if (name === 'on') {
      for (const [event, listener] of Object.entries(value)) node.addEventListener(event, listener);
    } else if (name === 'href') {
      const href = safeHref(value);
      if (href) node.setAttribute('href', href);
      if (href && /^https?:/i.test(href)) node.setAttribute('rel', 'noreferrer noopener');
    } else if (name === 'value' && 'value' in node) {
      node.value = String(value);
    } else if (name === 'checked' || name === 'selected') {
      node[name] = Boolean(value);
    } else {
      node.setAttribute(name, value === true ? '' : String(value));
    }
  }
  append(node, children);
  return node;
}

function append(node, children) {
  for (const child of children) {
    if (child === null || child === undefined || child === false) continue;
    if (Array.isArray(child)) append(node, child);
    else if (child instanceof Node) node.appendChild(child);
    else node.appendChild(document.createTextNode(visible(child)));
  }
}

/** Replaces `node`'s children. */
export function replace(node, ...children) {
  node.replaceChildren();
  append(node, children);
  return node;
}

/** The workbench's JSON API. Same origin, cookie credentials, nothing else. */
export async function api(path, body) {
  const init = { credentials: 'same-origin', headers: {} };
  if (body !== undefined) {
    init.method = 'POST';
    init.headers['Content-Type'] = 'application/json';
    init.body = JSON.stringify(body);
  }
  const response = await fetch(path, init);
  const text = await response.text();
  let value;
  try {
    value = JSON.parse(text);
  } catch {
    value = { error: text.trim() || `HTTP ${response.status}` };
  }
  if (!response.ok) throw new Error(value.error || `HTTP ${response.status}`);
  return value;
}

/** A verdict badge, worded as the report words it. */
export function badge(verdict, small = false) {
  const words = {
    pass: 'pass',
    'pass-with-warnings': 'warnings',
    fail: 'fail',
    unsupported: 'unsupported',
    unjudged: 'unjudged',
  };
  const tone = { 'pass-with-warnings': 'warn', unjudged: 'incomplete' }[verdict] || verdict;
  return h('span', { class: `badge v-${tone}${small ? ' small' : ''}` }, words[verdict] || verdict);
}

/** An outcome pill (`fail`, `warn`, `pass`, …). */
export function pill(outcome, text = outcome) {
  return h('span', { class: `pill o-${outcome}` }, text);
}

/** A message for the reader: an error, or a note. */
export function notice(text, kind = 'error') {
  return h('p', { class: `notice ${kind}`, role: kind === 'error' ? 'alert' : 'status' }, text);
}

/** Encodes one path segment or query value. */
export const enc = encodeURIComponent;
