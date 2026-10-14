// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// Administering the library: import, retention, redaction, and confirming
// anything that deletes. Each is a modal <dialog>; nothing changes until its
// primary button is pressed, and every change is a POST the server checks.

import { h, replace, api, notice, enc } from './dom.js';

/** Opens a modal dialog with `title`, `body` and buttons; returns it. */
function dialog(title, body, primary, onPrimary, danger = false) {
  const status = h('div', { class: 'status-area' });
  const go = h('button', { type: 'button', class: danger ? 'danger' : 'primary' }, primary);
  const modal = h('dialog', { class: 'modal', 'aria-label': title },
    h('h2', {}, title),
    body,
    status,
    h('div', { class: 'actions end' },
      h('button', { type: 'button', on: { click: () => modal.close() } }, 'Cancel'),
      go));
  go.addEventListener('click', async () => {
    go.disabled = true;
    try {
      const keep = await onPrimary(status);
      if (!keep) modal.close();
    } catch (error) {
      replace(status, notice(error.message));
    } finally {
      go.disabled = false;
    }
  });
  modal.addEventListener('close', () => modal.remove());
  document.body.appendChild(modal);
  modal.showModal();
  return modal;
}

/** Asks before something that cannot be undone. */
export function confirmDialog(title, text, primary, onConfirm) {
  return dialog(title, h('p', {}, text), primary, async () => { await onConfirm(); }, true);
}

/** Imports a file: a trace, an mcpsnoop capture or an Inspector export. */
export function importDialog(onDone) {
  const file = h('input', { type: 'file', accept: '.jsonl,.json,application/json', 'aria-label': 'Recording file' });
  const format = h('select', { 'aria-label': 'Format' },
    h('option', { value: 'auto' }, 'Detect from the file'),
    h('option', { value: 'trace' }, 'mcp-conformance trace (.jsonl)'),
    h('option', { value: 'mcpsnoop' }, 'mcpsnoop capture (.jsonl)'),
    h('option', { value: 'inspector' }, 'MCP Inspector protocol export (.json)'));
  const transport = h('select', { 'aria-label': 'Transport' },
    h('option', { value: '' }, 'Not stated'),
    h('option', { value: 'stdio' }, 'stdio'),
    h('option', { value: 'streamable-http' }, 'Streamable HTTP'));
  const body = h('div', { class: 'form' },
    h('p', {}, 'The file is copied into the library as a new recording, converted to a trace. What its format does not record is listed with it, never filled in.'),
    h('label', {}, 'File', file),
    h('label', {}, 'Format', format),
    h('label', {}, 'Transport ', h('span', { class: 'hint' }, '(an Inspector export does not record it)'), transport));
  return dialog('Import a recording', body, 'Import', async (status) => {
    const chosen = file.files[0];
    if (!chosen) {
      replace(status, notice('Choose a file first.'));
      return true;
    }
    const text = await chosen.text();
    const entry = await api('/api/import', { name: chosen.name, format: format.value, transport: transport.value, text });
    await onDone();
    location.hash = `#/s/${enc(entry.id)}`;
    return false;
  });
}

/** Lists, then deletes, the recordings older than a number of days. */
export function retentionDialog(onDone) {
  const days = h('input', { type: 'number', min: '0', value: '30', 'aria-label': 'Days to keep' });
  const preview = h('div', { class: 'preview' });
  let listed = null;
  const show = async () => {
    const result = await api('/api/retention', { days: Number(days.value), apply: false });
    listed = result;
    replace(preview, result.entries.length
      ? [h('p', {}, `${result.entries.length} recorded more than ${result.days} days ago:`),
        h('ul', { class: 'ids' }, result.entries.map((id) => h('li', {}, h('code', {}, id))))]
      : h('p', {}, `None was recorded more than ${result.days} days ago.`));
  };
  days.addEventListener('input', () => show().catch((error) => replace(preview, notice(error.message))));
  const body = h('div', { class: 'form' },
    h('p', {}, 'Delete every recording made more than this many days ago. A recording with no date is never deleted by this.'),
    h('label', {}, 'Keep the last', days, 'days'),
    preview);
  const modal = dialog('Retention', body, 'Delete these', async (status) => {
    if (!listed?.entries.length) {
      replace(status, notice('Nothing to delete.', 'note'));
      return true;
    }
    await api('/api/retention', { days: listed.days, apply: true });
    await onDone();
    return false;
  }, true);
  show().catch((error) => replace(preview, notice(error.message)));
  return modal;
}

function lines(text) {
  return text.split('\n').map((line) => line.trim()).filter(Boolean);
}

/** Makes a redacted copy of a recording. */
export function redactDialog(id) {
  const headers = h('textarea', { rows: '3', 'aria-label': 'Header names', spellcheck: 'false' });
  headers.value = 'authorization\ncookie\nset-cookie\nproxy-authorization';
  const pointers = h('textarea', { rows: '3', 'aria-label': 'JSON pointers', spellcheck: 'false', placeholder: '/params/arguments/apiKey\n/result/content/*/text' });
  const meta = h('input', { type: 'checkbox', checked: true });
  const strings = h('textarea', { rows: '2', 'aria-label': 'Literal strings', spellcheck: 'false', placeholder: 'one secret per line' });
  const body = h('div', { class: 'form' },
    h('p', {}, 'A redacted copy is added as a new recording; ', h('strong', {}, 'the original is not changed'),
      '. Every value a rule names becomes "[redacted]".'),
    h('label', {}, 'Headers, by name (one per line)', headers),
    h('label', {}, 'JSON pointers into each message (one per line; * matches any member)', pointers),
    h('label', { class: 'check' }, meta, 'Every _meta value'),
    h('label', {}, 'Literal strings, wherever they occur (one per line)', strings));
  return dialog(`Redact ${id}`, body, 'Make a redacted copy', async () => {
    const copy = await api(`/api/sessions/${enc(id)}/redact`, {
      headers: lines(headers.value), pointers: lines(pointers.value), meta: meta.checked, strings: lines(strings.value),
    });
    location.hash = `#/s/${enc(copy.id)}`;
    return false;
  });
}

/** Edits a recording's tags. */
export function tagsDialog(id, tags, onDone) {
  const input = h('input', { type: 'text', value: tags.join(', '), 'aria-label': 'Tags, comma-separated', spellcheck: 'false' });
  const body = h('div', { class: 'form' }, h('label', {}, 'Tags, separated by commas', input));
  return dialog(`Tags of ${id}`, body, 'Save', async () => {
    await api(`/api/sessions/${enc(id)}/tags`, { tags: input.value.split(',').map((t) => t.trim()).filter(Boolean) });
    await onDone();
    return false;
  });
}
