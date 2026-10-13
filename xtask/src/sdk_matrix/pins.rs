// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The package managers' pins agree with `interop/sdk-pins.env`.
//!
//! Each SDK is pinned twice by necessity: once in `sdk-pins.env`, which the
//! matrix page publishes and which names the upstream commit its conformance
//! server is taken from, and once in the lockfile its package manager
//! installs from. `setup.sh` refuses to run when the two disagree, but it runs
//! only on the schedule; this runs on every change, so a bump of one without
//! the other — a dependency bot's version bump, say, which moves the lockfile
//! and not the commit, the file hashes or the recorded corpus — is refused
//! where it is made rather than on the next Monday.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Each SDK's pin as its package manager's files must spell it: the file,
/// the [`super::PINS`] key, and the text with the version spliced in.
const SPELLINGS: [(&str, &str, &str); 6] = [
    (
        "interop/package.json",
        "TS_SDK_VERSION",
        "\"@modelcontextprotocol/sdk\": \"{}\"",
    ),
    ("interop/requirements.in", "PY_SDK_VERSION", "mcp=={}"),
    ("interop/requirements.txt", "PY_SDK_VERSION", "mcp=={} "),
    (
        "interop/go/go.mod",
        "GO_SDK_VERSION",
        "github.com/modelcontextprotocol/go-sdk {}",
    ),
    (
        "interop/rust/Cargo.toml",
        "RUST_SDK_VERSION",
        "rmcp = { version = \"={}\"",
    ),
    (
        "interop/rust/Cargo.lock",
        "RUST_SDK_VERSION",
        "name = \"rmcp\"\nversion = \"{}\"",
    ),
];

/// `true` when every package manager pins what `pins` says.
pub(super) fn check(root: &Path, pins: &BTreeMap<String, String>) -> bool {
    let mut ok = true;
    for (file, key, spelling) in SPELLINGS {
        let Ok(text) = fs::read_to_string(root.join(file)) else {
            eprintln!("xtask: sdk-matrix — cannot read {file}");
            ok = false;
            continue;
        };
        if let Some(problem) = disagreement(file, &text, key, spelling, pins) {
            eprintln!("xtask: sdk-matrix — {problem}");
            ok = false;
        }
    }
    ok
}

/// Why `text` does not pin what `pins[key]` says, if it does not.
fn disagreement(
    file: &str,
    text: &str,
    key: &str,
    spelling: &str,
    pins: &BTreeMap<String, String>,
) -> Option<String> {
    let Some(version) = pins.get(key) else {
        return Some(format!("{} names no {key}", super::PINS));
    };
    let expected = spelling.replace("{}", version);
    (!text.contains(&expected)).then(|| {
        format!(
            "{file} does not pin `{}`, which {} names as {key}: bump both together, with \
             the upstream commit and file hashes, and re-record the matrix",
            expected.replace('\n', " "),
            super::PINS
        )
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn a_lockfile_that_moved_alone_is_refused() {
        let pins = BTreeMap::from([("GO_SDK_VERSION".to_owned(), "v1.8.0".to_owned())]);
        let spelling = "github.com/modelcontextprotocol/go-sdk {}";
        let agrees = "require github.com/modelcontextprotocol/go-sdk v1.8.0 // indirect";
        let moved = "require github.com/modelcontextprotocol/go-sdk v1.9.0 // indirect";
        assert_eq!(
            disagreement("go.mod", agrees, "GO_SDK_VERSION", spelling, &pins),
            None
        );
        let problem = disagreement("go.mod", moved, "GO_SDK_VERSION", spelling, &pins);
        assert!(problem.is_some_and(|problem| problem.contains("v1.8.0")));
    }

    #[test]
    fn the_committed_pins_agree() {
        let root = crate::workspace_root();
        let pins = super::super::load_pins(&root).expect("the pins file parses");
        assert!(check(&root, &pins));
    }
}
