// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `requirements`: the registry, filtered, and one requirement at a time.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::{Command, Output};

use serde_json::Value;

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .args(args)
        .output()
        .expect("binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

/// The `2025-11-25` registry's requirements as JSON, under `filters`.
fn requirements(filters: &[&str]) -> Vec<Value> {
    let mut args = vec![
        "requirements",
        "--revision",
        "2025-11-25",
        "--format",
        "json",
    ];
    args.extend_from_slice(filters);
    let output = run(&args);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let registry: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(registry["revision"], "2025-11-25");
    registry["requirements"].as_array().unwrap().clone()
}

fn id(requirement: &Value) -> &str {
    requirement["id"].as_str().unwrap()
}

#[test]
fn help_offers_only_the_formats_requirements_accepts() {
    let help = stdout(&run(&["requirements", "--help"]));
    assert!(help.contains("human") && help.contains("json"), "{help}");
    assert!(!help.contains("junit") && !help.contains("sarif"), "{help}");
    let rejected = run(&["requirements", "--format", "sarif"]);
    assert_eq!(rejected.status.code(), Some(2), "{rejected:?}");
}

#[test]
fn the_unfiltered_json_is_the_registry_document() {
    let output = run(&[
        "requirements",
        "--revision",
        "2025-11-25",
        "--format",
        "json",
    ]);
    let text = stdout(&output);
    assert!(text.starts_with("{\n  \"revision\": \"2025-11-25\",\n  \"requirements\": ["));
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../mcp-conformance-core/registry/2025-11-25"
    );
    let mut on_disk = 0;
    for entry in std::fs::read_dir(path).unwrap() {
        let area: Value =
            serde_json::from_str(&std::fs::read_to_string(entry.unwrap().path()).unwrap()).unwrap();
        on_disk += area["requirements"].as_array().map_or(0, Vec::len);
    }
    assert_eq!(requirements(&[]).len(), on_disk);
}

#[test]
fn filters_narrow_the_listing_and_partition_it() {
    let all = requirements(&[]);
    let life = requirements(&["--area", "life"]);
    assert!(!life.is_empty(), "--area life lists no requirement");
    assert!(
        life.iter()
            .all(|requirement| id(requirement).starts_with("LIFE-"))
    );
    assert_eq!(
        life.len(),
        all.iter()
            .filter(|requirement| id(requirement).starts_with("LIFE-"))
            .count()
    );
    let two_areas = requirements(&["--area", "LIFE", "--area", "PAGE"]);
    assert!(two_areas.len() > life.len());

    let should = requirements(&["--level", "should", "--level", "should-not"]);
    assert!(!should.is_empty(), "--level should lists no requirement");
    assert!(
        should
            .iter()
            .all(|requirement| requirement["level"].as_str().unwrap().starts_with("SHOULD"))
    );

    let checked = requirements(&["--checked"]);
    let excluded = requirements(&["--excluded"]);
    assert!(
        checked
            .iter()
            .all(|requirement| requirement["checks"].is_array())
    );
    assert!(
        excluded
            .iter()
            .all(|requirement| requirement["exclusion"].is_string())
    );
    assert_eq!(checked.len() + excluded.len(), all.len());
    assert!(!checked.is_empty() && !excluded.is_empty());

    // Filters combine.
    let checked_life = requirements(&["--area", "LIFE", "--checked"]);
    assert!(!checked_life.is_empty() && checked_life.len() < life.len());

    // The human listing filters the same way.
    let text = stdout(&run(&[
        "requirements",
        "--revision",
        "2025-11-25",
        "--excluded",
    ]));
    // `--excluded` adds each row's reason on the line after it.
    assert_eq!(text.lines().count(), 1 + 2 * excluded.len(), "{text}");
    assert!(!text.contains("checks:"), "{text}");

    let both = run(&["requirements", "--checked", "--excluded"]);
    assert_eq!(both.status.code(), Some(2), "{both:?}");
    let unknown = run(&["requirements", "--area", "NOPE"]);
    assert_eq!(unknown.status.code(), Some(2), "{unknown:?}");
    assert!(stderr(&unknown).contains("has no area NOPE"), "{unknown:?}");
}

#[test]
fn the_excluded_listing_gives_each_reason() {
    let all = requirements(&[]);
    let excluded = requirements(&["--excluded"]);
    let text = stdout(&run(&[
        "requirements",
        "--revision",
        "2025-11-25",
        "--excluded",
    ]));
    // `--excluded` promises the reason: each row is followed by its own.
    for requirement in &excluded {
        assert!(
            text.contains(&format!(
                "(excluded) — {}\n      why: {}\n",
                requirement["source"]["quote"].as_str().unwrap(),
                requirement["exclusion"].as_str().unwrap()
            )),
            "{text}"
        );
    }
    // Without `--excluded`, the listing stays one line per requirement.
    let unfiltered = stdout(&run(&["requirements", "--revision", "2025-11-25"]));
    assert_eq!(unfiltered.lines().count(), 1 + all.len(), "{unfiltered}");
    assert!(!unfiltered.contains("      why: "), "{unfiltered}");
}

#[test]
fn an_id_shows_one_requirement_in_full() {
    let output = run(&["requirements", "--revision", "2025-11-25", "life-001"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let text = stdout(&output);
    let first = requirements(&[])
        .into_iter()
        .find(|requirement| id(requirement) == "LIFE-001")
        .unwrap();
    assert!(
        text.starts_with(&format!(
            "LIFE-001 — MUST, binding {} — revision 2025-11-25\n  spec: \"{}\"\n  see:  https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle#",
            match first["actor"].as_str().unwrap() {
                "server" => "the server",
                "client" => "the client",
                _ => "client and server",
            },
            first["source"]["quote"].as_str().unwrap()
        )),
        "{text}"
    );
    assert!(text.contains("\n  checks: "), "{text}");

    // An excluded one gives its reason; a gated one, its capability.
    let excluded = requirements(&["--excluded"]).remove(0);
    let gated = requirements(&[])
        .into_iter()
        .find(|requirement| requirement["capability"].is_string())
        .unwrap();
    let output = run(&[
        "requirements",
        "--revision",
        "2025-11-25",
        id(&excluded),
        id(&gated),
    ]);
    let text = stdout(&output);
    assert!(
        text.contains(&format!(
            "  excluded: {}\n",
            excluded["exclusion"].as_str().unwrap()
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "  capability: {} — judged only in a session that declares it\n",
            gated["capability"].as_str().unwrap()
        )),
        "{text}"
    );
    // In the order named, a blank line between them.
    assert!(text.find(id(&excluded)) < text.find(id(&gated)), "{text}");
    assert_eq!(text.matches("\n\n").count(), 1, "{text}");
}

#[test]
fn each_party_a_requirement_binds_is_named() {
    let all = requirements(&[]);
    for (actor, phrase) in [
        ("server", "binding the server"),
        ("client", "binding the client"),
        ("both", "binding client and server"),
    ] {
        let one = all
            .iter()
            .find(|requirement| requirement["actor"] == actor)
            .unwrap();
        let text = stdout(&run(&["requirements", "--revision", "2025-11-25", id(one)]));
        assert!(
            text.starts_with(&format!("{} — ", id(one))) && text.contains(phrase),
            "{actor}: {text}"
        );
    }
}

#[test]
fn an_id_as_json_is_the_registry_shape_and_an_unknown_one_is_an_error() {
    let first = requirements(&[])
        .into_iter()
        .find(|requirement| id(requirement) == "LIFE-001")
        .unwrap();
    let json = run(&[
        "requirements",
        "--revision",
        "2025-11-25",
        "--format",
        "json",
        "LIFE-001",
    ]);
    let registry: Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(registry["requirements"], serde_json::json!([first]));

    let missing = run(&["requirements", "--revision", "2025-11-25", "LIFE-999"]);
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
    assert!(
        stderr(&missing).contains("revision 2025-11-25 has no requirement LIFE-999"),
        "{missing:?}"
    );
}

#[test]
fn an_id_of_the_other_revision_says_which_revision_has_it() {
    // BASE-003 is a 2025-11-25 clause; the default lookup is the newest
    // revision, which numbers its request-id rule differently. A reader who
    // copied the ID off a 2025-11-25 report is told where it is.
    let output = run(&["requirements", "BASE-003"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let message = stderr(&output);
    assert!(
        message.contains("it is a 2025-11-25 requirement: pass --revision 2025-11-25"),
        "{message}"
    );
    let found = run(&["requirements", "--revision", "2025-11-25", "BASE-003"]);
    assert_eq!(found.status.code(), Some(0), "{found:?}");
    // An ID no revision has gets no such hint.
    let nowhere = stderr(&run(&["requirements", "BASE-999"]));
    assert!(!nowhere.contains("--revision"), "{nowhere}");
}

#[test]
fn the_readmes_requirements_and_completions_examples_run() {
    // The crate README shows these commands; each must succeed as written,
    // at the default revision (an earlier example named two `2025-11-25`
    // IDs and exited 2).
    let readme = include_str!("../README.md");
    let commands: Vec<Vec<&str>> = readme
        .lines()
        .filter_map(|line| line.strip_prefix("mcp-trace-validator "))
        .map(|line| line.split(['#', '>']).next().unwrap_or_default())
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .filter(|args| matches!(args.first(), Some(&("requirements" | "completions"))))
        .collect();
    assert!(commands.len() >= 4, "{commands:?}");
    for args in commands {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(0), "{args:?}: {output:?}");
    }
}
