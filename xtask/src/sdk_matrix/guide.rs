// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The worked guides and the scheduled job share their commands.
//!
//! The book's guide to recording an SDK's server does not restate the
//! commands: it includes them, by mdBook anchor, from `interop/matrix.sh` —
//! the script the scheduled job runs. Inclusion keeps the two from drifting
//! only while both ends still point at each other, so this holds the links:
//! every anchor the guide includes exists in the script, the guide includes
//! the TypeScript and Python sections (roadmap item 1.7 is those two guides),
//! and the scheduled workflow still runs `setup.sh`, `matrix.sh` and the
//! comparison.

use std::fs;
use std::path::Path;

/// The guide chapter.
const GUIDE: &str = "book/src/sdk-servers.md";

/// The script the guide includes from and the job runs.
const SCRIPT: &str = "interop/matrix.sh";

/// The workflow carrying the scheduled job.
const WORKFLOW: &str = ".github/workflows/scheduled.yml";

/// The anchors the guide must include: the recording function and the two
/// SDKs the worked guides cover.
const REQUIRED: [&str; 3] = ["record", "typescript", "python"];

/// What the scheduled job must run, verbatim.
const JOB_RUNS: [&str; 3] = [
    "./setup.sh",
    "./matrix.sh",
    "cargo xtask sdk-matrix --recaptured interop/out-matrix",
];

/// `true` when the guide, the script and the workflow still agree.
pub(super) fn check(root: &Path) -> bool {
    let read = |path: &str| fs::read_to_string(root.join(path));
    let (Ok(guide), Ok(script), Ok(workflow)) = (read(GUIDE), read(SCRIPT), read(WORKFLOW)) else {
        eprintln!("xtask: sdk-matrix — cannot read {GUIDE}, {SCRIPT} or {WORKFLOW}");
        return false;
    };
    let problems = problems(&guide, &script, &workflow);
    for problem in &problems {
        eprintln!("xtask: sdk-matrix — {problem}");
    }
    problems.is_empty()
}

/// Every way the three files disagree.
fn problems(guide: &str, script: &str, workflow: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let prefix = "{{#include ../../interop/matrix.sh:";
    let included: Vec<&str> = guide
        .match_indices(prefix)
        .filter_map(|(at, _)| guide[at + prefix.len()..].split_once("}}"))
        .map(|(anchor, _)| anchor)
        .collect();
    for anchor in &included {
        if !script.contains(&format!("# ANCHOR: {anchor}\n"))
            || !script.contains(&format!("# ANCHOR_END: {anchor}\n"))
        {
            problems.push(format!(
                "{GUIDE} includes `{anchor}`, which {SCRIPT} does not mark"
            ));
        }
    }
    for anchor in REQUIRED {
        if !included.contains(&anchor) {
            problems.push(format!(
                "{GUIDE} no longer includes `{anchor}` from {SCRIPT}, so it no longer shows \
                 the commands the scheduled job runs"
            ));
        }
    }
    for command in JOB_RUNS {
        if !workflow.contains(command) {
            problems.push(format!("{WORKFLOW} no longer runs `{command}`"));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT_TEXT: &str = "# ANCHOR: record\nx\n# ANCHOR_END: record\n\
        # ANCHOR: typescript\ny\n# ANCHOR_END: typescript\n\
        # ANCHOR: python\nz\n# ANCHOR_END: python\n";
    const WORKFLOW_TEXT: &str =
        "./setup.sh\n./matrix.sh\ncargo xtask sdk-matrix --recaptured interop/out-matrix\n";

    fn guide(anchors: &[&str]) -> String {
        let mut text = String::new();
        for anchor in anchors {
            text.push_str("{{#include ../../interop/matrix.sh:");
            text.push_str(anchor);
            text.push_str("}}\n");
        }
        text
    }

    #[test]
    fn agreeing_files_raise_nothing() {
        let guide = guide(&["record", "typescript", "python"]);
        assert_eq!(
            problems(&guide, SCRIPT_TEXT, WORKFLOW_TEXT),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_dropped_include_an_unknown_anchor_and_a_dropped_step_are_named() {
        let guide = guide(&["record", "typescript", "kotlin"]);
        let found = problems(&guide, SCRIPT_TEXT, "./setup.sh\n");
        assert!(found.iter().any(|problem| problem.contains("`kotlin`")));
        assert!(found.iter().any(|problem| problem.contains("`python`")));
        assert!(found.iter().any(|problem| problem.contains("./matrix.sh")));
    }

    #[test]
    fn the_committed_files_agree() {
        assert!(check(&crate::workspace_root()));
    }
}
