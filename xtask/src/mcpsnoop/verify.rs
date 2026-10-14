// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The consistency the page's honesty rests on, checked before it renders.

use std::collections::BTreeSet;
use std::path::Path;

use super::classify::compare;
use super::{Inputs, PINNED, render};

const CLASSES: [&str; 5] = [
    "mcpsnoop-defect",
    "our-defect",
    "granularity",
    "revision-attribution",
    "spec-ambiguity",
];
const RIGHT: [&str; 4] = ["ours", "mcpsnoop", "both", "undecided"];
const RESOLUTIONS: [&str; 4] = ["fixed: ", "open: ", "upstream: ", "none: "];

/// Every divergence the results show, as `(kind, subject, trace)`.
pub(crate) fn divergences(inputs: &Inputs) -> BTreeSet<(String, String, String)> {
    let mut out = BTreeSet::new();
    for trace in &inputs.results.traces {
        for (kind, subject) in compare(trace, &inputs.rules.rules).divergences() {
            out.insert((kind.to_owned(), subject.to_owned(), trace.trace.clone()));
        }
    }
    out
}

/// The rendered page, or every reason it cannot be trusted.
pub(crate) fn verify(root: &Path, inputs: &Inputs) -> Result<String, Vec<String>> {
    let mut problems = Vec::new();
    for (file, version) in [
        ("results.json", &inputs.results.mcpsnoop),
        ("rules.json", &inputs.rules.mcpsnoop),
    ] {
        if version != PINNED {
            problems.push(format!(
                "{file} is for mcpsnoop {version}; the pin is {PINNED}"
            ));
        }
    }
    // The page's account of the conversion rests on these; a run where the
    // conversion stopped matching mcpsnoop's own recordings cannot be published.
    if inputs.results.native.is_empty() {
        problems.push("results.json holds no native recording check".to_owned());
    }
    for check in inputs
        .results
        .native
        .iter()
        .filter(|check| !check.same_as_native)
    {
        problems.push(format!(
            "native/{}: the converted trace did not draw mcpsnoop's own findings",
            check.session
        ));
    }
    check_rules(inputs, &mut problems);
    for trace in &inputs.results.traces {
        for message in compare(trace, &inputs.rules.rules).unclaimed {
            problems.push(format!(
                "{}: no rule in rules.json claims the finding {message:?}",
                trace.trace
            ));
        }
    }
    check_triage(root, inputs, &mut problems);
    if problems.is_empty() {
        Ok(render::page(inputs))
    } else {
        Err(problems)
    }
}

fn check_rules(inputs: &Inputs, problems: &mut Vec<String>) {
    let revisions = inputs.quotes.revisions();
    let mut seen = BTreeSet::new();
    for rule in &inputs.rules.rules {
        if !seen.insert(rule.id.as_str()) {
            problems.push(format!("rules.json names rule {:?} twice", rule.id));
        }
        if !matches!(rule.context.as_deref(), None | Some("http" | "pairing")) {
            problems.push(format!(
                "rule {}: unknown context {:?}",
                rule.id, rule.context
            ));
        }
        let mut all = BTreeSet::new();
        for (revision, ids) in &rule.maps {
            if !revisions.contains(revision.as_str()) {
                problems.push(format!(
                    "rule {}: no registry for revision {revision}",
                    rule.id
                ));
            }
            for id in ids {
                all.insert(id.as_str());
                if inputs.quotes.get(revision, id).is_none() {
                    problems.push(format!(
                        "rule {}: {id} is not a {revision} requirement",
                        rule.id
                    ));
                }
            }
        }
        for id in &rule.partial {
            if !all.contains(id.as_str()) {
                problems.push(format!("rule {}: partial {id} is not one it maps", rule.id));
            }
        }
    }
}

fn check_triage(root: &Path, inputs: &Inputs, problems: &mut Vec<String>) {
    let found = divergences(inputs);
    let mut covered: BTreeSet<(String, String, String)> = BTreeSet::new();
    let revision_of = |trace: &str| {
        inputs
            .results
            .traces
            .iter()
            .find(|result| result.trace == trace)
            .map(|result| result.revision.clone())
    };
    let triage = &inputs.triage;
    let mut cited = BTreeSet::new();
    for entry in &triage.divergences {
        let label = format!("triage {} {}", entry.kind, entry.subject);
        for trace in &entry.traces {
            let key = (entry.kind.clone(), entry.subject.clone(), trace.clone());
            if !found.contains(&key) {
                problems.push(format!("{label}: {trace} shows no such divergence (stale)"));
            }
            if !covered.insert(key) {
                problems.push(format!("{label}: {trace} is triaged twice"));
            }
        }
        let revisions: BTreeSet<String> = entry
            .traces
            .iter()
            .filter_map(|trace| revision_of(trace))
            .collect();
        check_judgement(
            &label,
            (&entry.class, &entry.right, &entry.resolution),
            &entry.quote,
            &revisions,
            inputs,
            problems,
        );
        cited.extend(upstream_id(&entry.resolution));
    }
    for (kind, subject, trace) in found.difference(&covered) {
        problems.push(format!(
            "{trace}: {kind} divergence on {subject} has no triage entry"
        ));
    }
    check_recorded(root, inputs, &mut cited, problems);
    check_issues(inputs, &cited, problems);
}

/// Every cited issue drafted, every drafted issue cited.
fn check_issues(inputs: &Inputs, cited: &BTreeSet<String>, problems: &mut Vec<String>) {
    let triage = &inputs.triage;
    let drafted: BTreeSet<&str> = triage
        .issues
        .iter()
        .map(|issue| issue.id.as_str())
        .collect();
    for id in cited {
        if !drafted.contains(id.as_str()) {
            problems.push(format!("upstream issue {id} is cited but not drafted"));
        }
    }
    for id in drafted {
        if !cited.contains(id) {
            problems.push(format!(
                "upstream issue {id} is drafted but cited by no triage"
            ));
        }
    }
}

fn check_recorded(
    root: &Path,
    inputs: &Inputs,
    cited: &mut BTreeSet<String>,
    problems: &mut Vec<String>,
) {
    let triage = &inputs.triage;
    let every: BTreeSet<String> = inputs
        .quotes
        .revisions()
        .into_iter()
        .map(str::to_owned)
        .collect();
    for recorded in &triage.recorded {
        let label = format!("recorded {}", recorded.rule);
        if !inputs
            .rules
            .rules
            .iter()
            .any(|rule| rule.id == recorded.rule)
        {
            problems.push(format!("{label}: no such rule"));
        }
        for evidence in &recorded.evidence {
            if !root.join("conformance/mcpsnoop").join(evidence).is_file() {
                problems.push(format!("{label}: evidence {evidence} is not committed"));
            }
        }
        check_judgement(
            &label,
            (&recorded.class, &recorded.right, &recorded.resolution),
            &recorded.quote,
            &every,
            inputs,
            problems,
        );
        cited.extend(upstream_id(&recorded.resolution));
    }
}

fn upstream_id(resolution: &str) -> Option<String> {
    resolution
        .strip_prefix("upstream: ")
        .map(|id| id.trim().to_owned())
}

fn check_judgement(
    label: &str,
    (class, right, resolution): (&str, &str, &str),
    quote: &[String],
    revisions: &BTreeSet<String>,
    inputs: &Inputs,
    problems: &mut Vec<String>,
) {
    if !CLASSES.contains(&class) {
        problems.push(format!("{label}: unknown class {class:?}"));
    }
    if !RIGHT.contains(&right) {
        problems.push(format!("{label}: unknown verdict {right:?}"));
    }
    if !RESOLUTIONS
        .iter()
        .any(|prefix| resolution.starts_with(prefix))
    {
        problems.push(format!(
            "{label}: resolution must start with one of {RESOLUTIONS:?}"
        ));
    }
    if quote.is_empty() {
        problems.push(format!("{label}: quotes no clause"));
    }
    for id in quote {
        if !revisions
            .iter()
            .any(|revision| inputs.quotes.get(revision, id).is_some())
        {
            problems.push(format!("{label}: {id} is in none of {revisions:?}"));
        }
    }
}
