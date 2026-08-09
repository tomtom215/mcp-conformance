#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)
"""Extract RFC 2119 clauses from spec pages as spec-drift-verifiable quotes.

The registry's extraction method is "live fetch -> verbatim quote -> check or
documented exclusion" (docs/plan/03-conformance-strategy.md). The error-prone
half is the quote: it must survive `xtask/src/spec_drift.rs`'s normalization
and be found in the published page, or the gate fails. This tool removes that
class of error by porting that normalization exactly, so a quote it emits is
already in the form the gate compares against.

**Calibration is the reason to trust it.** The port is verified against the
shipped registry: all 140 committed `2025-11-25` quotes verify under this
file's `normalize` + `quote_present`, including LIFE-009, which exercises the
intro-colon path. A port that drifts from the Rust fails that check loudly.

Beware one trap this file has already fallen into: `"resources.json"` ends with
`"sources.json"`, so a loop that skips the page manifest with
`endswith("sources.json")` silently drops ten requirements. Compare basenames.

Two subtleties the port must honour, both learned by the check failing first:

  * `spec-drift` does NOT strip MDX component tags, so a clause may never span
    one. Tags are treated here as clause boundaries.
  * Markdown tables restate prose as a matrix and their pipes corrupt sentence
    boundaries, so table rows are dropped.

Emits candidates only. `id`, `actor`, the capability gate, the `applies` range
and the check-or-exclusion decision are per-clause judgments and are curated by
hand; this tool does not guess them.

Usage:
    python3 tools/extract-clauses.py <spec-root> <page> [<page> ...]

where <spec-root> holds `<page>.mdx` files fetched from
docs/specification/<revision>/ in the modelcontextprotocol repository.
"""
import json
import re
import sys
from pathlib import Path

LEVELS = ["MUST NOT", "SHALL NOT", "SHOULD NOT", "NOT RECOMMENDED",
          "MUST", "SHALL", "SHOULD", "REQUIRED", "RECOMMENDED", "MAY", "OPTIONAL"]


def strip_numbered_marker(line):
    digits = 0
    for ch in line:
        if ch.isdigit():
            digits += 1
        else:
            break
    if digits == 0:
        return None
    rest = line[digits:]
    return rest[2:] if rest.startswith(". ") else None


def unwrap_links(text):
    out = text
    while True:
        mid = out.find("](")
        if mid < 0:
            return out
        open_ = out.rfind("[", 0, mid)
        if open_ < 0:
            return out
        close_rel = out.find(")", mid + 2)
        if close_rel < 0:
            return out
        out = out[:open_] + out[open_ + 1:mid] + out[close_rel + 1:]


def strip_italics(text):
    chars = list(text)
    out = []
    for i, ch in enumerate(chars):
        if ch == "_":
            prev_word = i > 0 and chars[i - 1].isalnum()
            next_word = i + 1 < len(chars) and chars[i + 1].isalnum()
            if prev_word != next_word:
                continue
        out.append(ch)
    return "".join(out)


def normalize(text):
    joined = []
    for line in text.split("\n"):
        trimmed = line.lstrip()
        for prefix in ("- ", "* "):
            if trimmed.startswith(prefix):
                trimmed = trimmed[len(prefix):]
                break
        else:
            stripped = strip_numbered_marker(trimmed)
            if stripped is not None:
                trimmed = stripped
        joined.append(" " + trimmed)
    joined = "".join(joined)
    unstyled = strip_italics(unwrap_links(joined).replace("**", "").replace("\\_", "_"))
    unstyled = unstyled.replace("“", '"').replace("”", '"').replace("’", "'")
    collapsed = []
    last_space = False
    for ch in unstyled:
        if ch.isspace():
            if not last_space:
                collapsed.append(" ")
            last_space = True
        else:
            collapsed.append(ch)
            last_space = False
    return "".join(collapsed).strip()


def strip_frontmatter_and_code(text):
    """Drop frontmatter, fenced code, MDX component tags and table rows.

    None of the four is normative prose: tables restate prose as a matrix and
    their pipes corrupt sentence boundaries, and MDX tags such as `<Note>` are
    presentation. Dropping them here keeps quotes to the clause itself."""
    if text.startswith("---"):
        end = text.find("\n---", 3)
        if end >= 0:
            text = text[end + 4:]
    out, in_fence = [], False
    for line in text.split("\n"):
        if line.lstrip().startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        stripped = line.strip()
        if stripped.startswith("|"):          # markdown table row
            continue
        # MDX component tags, opening or closing, alone or inline.
        line = re.sub(r"</?[A-Z][A-Za-z0-9]*(\s[^>]*)?/?>", "\x00", line)
        out.append(line)
    return "\n".join(out)


def sections(text):
    """(anchor, body) per heading, so each clause can cite `page#anchor`."""
    parts, current, buf = [], None, []
    for line in text.split("\n"):
        m = re.match(r"^(#{2,6})\s+(.*)$", line)
        if m:
            if buf:
                parts.append((current, "\n".join(buf)))
            title = m.group(2).strip()
            anchor = re.sub(r"[^a-z0-9\s-]", "", title.lower()).strip().replace(" ", "-")
            current, buf = anchor, []
        else:
            buf.append(line)
    if buf:
        parts.append((current, "\n".join(buf)))
    return parts


SENTENCE_END = re.compile(r'(?<=[.:])\s+(?=[A-Z"`\[])')


def clauses(body):
    """Normalized sentences carrying at least one RFC 2119 keyword."""
    norm = normalize(body)
    if not norm:
        return []
    out = []
    # `\x00` marks where an MDX tag stood. spec-drift's normalizer leaves the
    # tag in the page text, so a clause may never span one.
    pieces = [seg for block in norm.split("\x00") for seg in SENTENCE_END.split(block)]
    for sentence in pieces:
        sentence = sentence.strip()
        if not sentence:
            continue
        found = [lv for lv in LEVELS if re.search(r"\b" + re.escape(lv) + r"\b", sentence)]
        if not found:
            continue
        # Strongest level present decides the entry's level.
        level = found[0]
        out.append((level, sentence))
    return out


def main():
    spec_root = Path(sys.argv[1])
    pages = sys.argv[2:]
    result = {}
    for page in pages:
        f = spec_root / (page + ".mdx")
        text = strip_frontmatter_and_code(f.read_text())
        entries = []
        for anchor, body in sections(text):
            for level, quote in clauses(body):
                entries.append({
                    "section": f"{page}#{anchor}" if anchor else page,
                    "level": level,
                    "quote": quote,
                })
        result[page] = entries
    json.dump(result, sys.stdout, indent=2)


if __name__ == "__main__":
    main()


def quote_present(page_normalized, quote):
    """Exact port of `spec_drift.rs::quote_present`, including the intro-colon path."""
    relaxed = page_normalized.replace("; ", " ")
    nq = normalize(quote)
    if nq.replace("; ", " ") in relaxed:
        return True
    frags = nq.split("; ")
    if all(f and f in relaxed for f in frags):
        return True
    if ": " in nq:
        intro, items = nq.split(": ", 1)
        if (intro + ":") in relaxed and all(f and f in relaxed for f in items.split("; ")):
            return True
    return False
