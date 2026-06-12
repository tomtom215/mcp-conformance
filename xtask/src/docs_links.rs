// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The documentation link gate: every relative link in every tracked Markdown
//! file must resolve — to an existing file or directory, and, when it carries
//! an anchor, to a real heading in the target document. Absolute links
//! (`http://`, `https://`, `mailto:`) are out of scope: checking them needs a
//! network and external sites change under us; in-repo navigability is ours
//! to guarantee and is checked offline and deterministically.
//!
//! Fenced code blocks and inline code spans are stripped before scanning, so
//! example Markdown inside documentation is never mistaken for a live link.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::path::Path;
use std::process::Command;

/// Runs the gate over every tracked `*.md` file; `true` when every relative
/// link resolves.
pub(crate) fn run() -> bool {
    let root = crate::workspace_root();
    let Some(files) = tracked_markdown(&root) else {
        eprintln!("xtask: docs links — cannot list tracked files (is git available?)");
        return false;
    };
    let mut broken: Vec<String> = Vec::new();
    for relative in &files {
        let path = root.join(relative);
        let Ok(text) = std::fs::read_to_string(&path) else {
            broken.push(format!("{relative}: unreadable"));
            continue;
        };
        for (line, target) in links(&text) {
            if let Some(problem) = check_target(&root, &path, &target) {
                broken.push(format!("{relative}:{line}: ({target}) {problem}"));
            }
        }
    }
    if broken.is_empty() {
        eprintln!(
            "xtask: docs links — every relative link in {} Markdown files resolves",
            files.len()
        );
        true
    } else {
        for failure in &broken {
            eprintln!("xtask: docs links — {failure}");
        }
        false
    }
}

/// Tracked Markdown files, relative to the workspace root.
fn tracked_markdown(root: &Path) -> Option<Vec<String>> {
    let output = Command::new("git")
        .args(["ls-files", "*.md"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let listing = String::from_utf8(output.stdout).ok()?;
    Some(listing.lines().map(str::to_owned).collect())
}

/// Every link target in `text` with its 1-based line number, after stripping
/// fenced code blocks and inline code spans: inline `[text](target)` links
/// and reference definitions `[label]: target` (used by the changelog's
/// compare links — without this, a relative reference-style target would be
/// the gate's one false-negative path).
fn links(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut in_fence = false;
    for (index, raw_line) in text.lines().enumerate() {
        if raw_line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let line = strip_inline_code(raw_line);
        if let Some(rest) = line.strip_prefix('[')
            && let Some((_, target)) = rest.split_once("]: ")
        {
            found.push((index + 1, target.trim().to_owned()));
            continue;
        }
        let mut rest = line.as_str();
        while let Some(open) = rest.find("](") {
            let after = &rest[open + 2..];
            let Some(close) = after.find(')') else { break };
            found.push((index + 1, after[..close].to_owned()));
            rest = &after[close + 1..];
        }
    }
    found
}

/// Removes `` `code spans` `` from one line (unterminated spans drop the rest
/// of the line — over-stripping is safe, silently checking an example is not).
fn strip_inline_code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_code = false;
    for ch in line.chars() {
        if ch == '`' {
            in_code = !in_code;
        } else if !in_code {
            out.push(ch);
        }
    }
    out
}

/// `None` when `target` resolves; otherwise the human-readable problem.
fn check_target(root: &Path, source: &Path, target: &str) -> Option<String> {
    if target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
    {
        return None;
    }
    let (path_part, anchor) = match target.split_once('#') {
        Some((path, anchor)) => (path, Some(anchor)),
        None => (target, None),
    };
    let file = if path_part.is_empty() {
        source.to_path_buf() // same-file anchor: #heading
    } else {
        let base = source.parent().unwrap_or(root);
        base.join(path_part)
    };
    if !file.exists() {
        return Some("target does not exist".to_owned());
    }
    if let Some(anchor) = anchor {
        if file.extension().is_none_or(|ext| ext != "md") {
            return Some("anchor on a non-Markdown target".to_owned());
        }
        let Ok(target_text) = std::fs::read_to_string(&file) else {
            return Some("anchor target unreadable".to_owned());
        };
        if !heading_slugs(&target_text)
            .iter()
            .any(|slug| slug == anchor)
        {
            return Some(format!("no heading slugifies to #{anchor}"));
        }
    }
    None
}

/// GitHub-style slugs for every heading in a Markdown document: formatting
/// stripped, lowercased, punctuation removed, spaces to hyphens, duplicate
/// slugs suffixed `-1`, `-2`, … in document order.
fn heading_slugs(text: &str) -> Vec<String> {
    let mut slugs: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || !line.starts_with('#') {
            continue;
        }
        let heading = line.trim_start_matches('#').trim();
        let plain: String = strip_inline_code_markers(heading);
        let mut slug = String::new();
        for ch in plain.chars() {
            if ch.is_alphanumeric() || ch == '_' {
                slug.extend(ch.to_lowercase());
            } else if ch == ' ' || ch == '-' {
                slug.push('-');
            }
            // Everything else (punctuation) is dropped.
        }
        let base = slug.clone();
        let mut duplicate = 0;
        while slugs.contains(&slug) {
            duplicate += 1;
            slug = format!("{base}-{duplicate}");
        }
        slugs.push(slug);
    }
    slugs
}

/// Drops backticks and link syntax from a heading, keeping the visible text
/// (`### The [`Tap`] layer` slugifies from "The Tap layer").
fn strip_inline_code_markers(heading: &str) -> String {
    let mut out = String::with_capacity(heading.len());
    let mut chars = heading.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '`' | '*' | '[' | ']' => {}
            '(' => {
                // A link's URL part directly after `]`: skip to the closing
                // paren. Headings here never contain bare parens before one.
                if out.ends_with(|c: char| c.is_alphanumeric()) {
                    out.push('(');
                } else {
                    for inner in chars.by_ref() {
                        if inner == ')' {
                            break;
                        }
                    }
                }
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn extracts_links_outside_code_only() {
        // Line numbers: the fence occupies lines 2-4, so the second live link
        // sits on line 5.
        let doc =
            "see [a](x.md) here\n```\n[not](checked.md)\n```\nand `[nor](this.md)` but [b](y.md#z)";
        let found = links(doc);
        assert_eq!(
            found,
            vec![(1, "x.md".to_owned()), (5, "y.md#z".to_owned())]
        );
    }

    #[test]
    fn extracts_reference_definitions_too() {
        // A relative reference-style definition must be checked like any
        // inline link — it was the gate's one false-negative path.
        let doc =
            "[Unreleased]: https://example.com/compare\n[broken]: ../no-such.md\nprose [x][broken]";
        let found = links(doc);
        assert_eq!(
            found,
            vec![
                (1, "https://example.com/compare".to_owned()),
                (2, "../no-such.md".to_owned()),
            ]
        );
    }

    #[test]
    fn slugifies_headings_the_github_way() {
        let doc = "# Hello, World!\n## `code` in heading\n## Hello, World!\n```\n# not a heading\n```\n### a_b c-d\n";
        assert_eq!(
            heading_slugs(doc),
            vec![
                "hello-world".to_owned(),
                "code-in-heading".to_owned(),
                "hello-world-1".to_owned(),
                "a_b-c-d".to_owned(),
            ]
        );
    }

    #[test]
    fn check_target_distinguishes_the_failure_modes() {
        let root = crate::workspace_root();
        let readme = root.join("README.md");
        assert_eq!(check_target(&root, &readme, "https://example.com"), None);
        assert_eq!(check_target(&root, &readme, "LICENSE"), None);
        assert_eq!(check_target(&root, &readme, "corpus"), None);
        assert!(
            check_target(&root, &readme, "no-such-file.md")
                .unwrap()
                .contains("does not exist")
        );
        assert!(
            check_target(&root, &readme, "LICENSE#anchor")
                .unwrap()
                .contains("non-Markdown")
        );
        assert!(
            check_target(&root, &readme, "CHANGELOG.md#no-such-heading-here")
                .unwrap()
                .contains("slugifies")
        );
    }
}
