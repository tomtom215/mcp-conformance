// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Which tracked files `REUSE.toml` gives a licence to.
//!
//! A file that cannot carry a comment cannot carry the two-line header, so
//! its licence is stated for it in `REUSE.toml` (REUSE 3.3). The `spdx` gate
//! asks this module whether a headerless file is named there; `reuse lint`
//! is the fuller check, run at release, and this one needs no tool installed.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a private module the rest of the crate
// reads; this follows the rustc lint, as the sibling modules do.
#![allow(clippy::redundant_pub_crate)]

/// Every path pattern of every `[[annotations]]` table in `text`.
///
/// Read without a TOML parser: a pattern is a quoted string inside a
/// `path = [ … ]` or `path = "…"` value, which is all the file uses.
pub(crate) fn patterns(text: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    let mut in_path = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let value = match line.strip_prefix("path") {
            Some(rest) if rest.trim_start().starts_with('=') => {
                // An array left open goes on over the lines that follow.
                in_path = rest.contains('[') && !rest.contains(']');
                rest
            }
            _ if in_path => {
                in_path = !line.contains(']');
                line
            }
            _ => continue,
        };
        patterns.extend(value.split('"').skip(1).step_by(2).map(str::to_owned));
    }
    patterns
}

/// Whether `path` matches `pattern` as REUSE reads one: `*` is any run of
/// characters within a directory, `**` any run at all, the rest literal.
pub(crate) fn matches(pattern: &str, path: &str) -> bool {
    fn at(pattern: &[u8], path: &[u8]) -> bool {
        match pattern {
            [] => path.is_empty(),
            [b'*', b'*', rest @ ..] => (0..=path.len()).any(|skip| at(rest, &path[skip..])),
            [b'*', rest @ ..] => (0..=path.len())
                .take_while(|&skip| skip == 0 || path[skip - 1] != b'/')
                .any(|skip| at(rest, &path[skip..])),
            [first, rest @ ..] => path.first() == Some(first) && at(rest, &path[1..]),
        }
    }
    at(pattern.as_bytes(), path.as_bytes())
}

/// Whether any of `patterns` names `path`.
pub(crate) fn covers(patterns: &[String], path: &str) -> bool {
    patterns.iter().any(|pattern| matches(pattern, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_are_the_quoted_strings_of_each_path_value() {
        let text = "version = 1\n# path = [\"commented.out\"]\n[[annotations]]\npath = [\n    \
                    \"**.json\",\n    \"a/*.png\", \"b\",\n]\nSPDX-License-Identifier = \"MIT\"\n\
                    [[annotations]]\npath = \"one/file\"\nprecedence = \"override\"\n\
                    [[annotations]]\npath = [\"x\", \"y\"]\nSPDX-FileCopyrightText = \"z\"\n";
        assert_eq!(
            patterns(text),
            ["**.json", "a/*.png", "b", "one/file", "x", "y"]
        );
        assert!(patterns("pathological = [\"no\"]\n").is_empty());
    }

    #[test]
    fn a_single_star_stays_in_its_directory_and_a_double_one_does_not() {
        for (pattern, path, expected) in [
            ("**.json", "a/b/c.json", true),
            ("**.json", "c.json", true),
            ("**.json", "c.jsonl", false),
            ("a/*.json", "a/c.json", true),
            ("a/*.json", "a/b/c.json", false),
            ("a/**", "a/b/c", true),
            ("a/**", "a/", true),
            ("a/**", "ab/c", false),
            ("**/Cargo.lock", "fuzz/Cargo.lock", true),
            ("**/Cargo.lock", "Cargo.lock", false),
            ("Cargo.lock", "Cargo.lock", true),
            ("Cargo.lock", "x/Cargo.lock", false),
            ("a*c", "abc", true),
            ("a*c", "ac", true),
            ("a*c", "a/c", false),
            ("", "", true),
            ("", "a", false),
        ] {
            assert_eq!(matches(pattern, path), expected, "{pattern} against {path}");
        }
    }

    #[test]
    fn the_committed_file_names_what_cannot_carry_a_header_and_not_what_can() {
        let text = std::fs::read_to_string(crate::workspace_root().join("REUSE.toml"))
            .expect("REUSE.toml at the workspace root");
        let patterns = patterns(&text);
        for named in [
            "corpus/good/stdio-full-session.jsonl",
            "crates/mcp-conformance-core/registry/2025-11-25/base.json",
            "crates/mcp-conformance-core/spec-schema/2025-11-25.schema.json",
            "Cargo.lock",
            "fuzz/Cargo.lock",
            "interop/go/go.sum",
            "fuzz/corpus/trace_parse/seed-violation",
        ] {
            assert!(covers(&patterns, named), "{named} should be named");
        }
        for headed in ["README.md", "xtask/src/main.rs", "Cargo.toml", "action.yml"] {
            assert!(
                !covers(&patterns, headed),
                "{headed} carries its own header"
            );
        }
    }
}
