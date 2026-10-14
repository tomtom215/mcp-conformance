// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use super::*;

/// A fresh directory under the system temp dir, removed by the caller.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mcp-conformance-suite-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_checks(dir: &Path, scenario_dir: &str, checks: &str) {
    let path = dir.join(scenario_dir);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("checks.json"), checks).unwrap();
}

#[test]
fn the_timestamp_suffix_is_dropped_and_nothing_else() {
    assert_eq!(
        scenario_name("server-ping-2026-10-13T11-01-48-651Z"),
        "server-ping"
    );
    assert_eq!(
        scenario_name("initialize-2026-01-02T03-04-05-006Z"),
        "initialize"
    );
    // Not a timestamp: kept whole.
    assert_eq!(scenario_name("ping"), "ping");
    assert_eq!(
        scenario_name("ping-2026-10-13T11-01-48-651"),
        "ping-2026-10-13T11-01-48-651"
    );
    assert_eq!(
        scenario_name("x-2026-1a-13T11-01-48-651Z"),
        "x-2026-1a-13T11-01-48-651Z"
    );
    assert_eq!(scenario_name(""), "");
}

#[test]
fn a_server_run_reads_back_by_scenario_with_its_prefix_removed() {
    let dir = scratch("server");
    write_checks(
        &dir,
        "server-server-initialize-2026-10-13T11-01-48-651Z",
        r#"[{"id":"server-initialize","name":"ServerInitialize","status":"SUCCESS"}]"#,
    );
    write_checks(
        &dir,
        "server-ping-2026-10-13T11-01-49-001Z",
        r#"[{"id":"ping","status":"FAILURE","errorMessage":"no pong"},{"id":"info","status":"INFO"}]"#,
    );
    // Not a scenario: no checks.json.
    std::fs::create_dir_all(dir.join("tap")).unwrap();
    let scenarios = read_results(&dir).unwrap();
    let names: Vec<&str> = scenarios.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["ping", "server-initialize"]);
    assert_eq!(
        scenarios[0].checks[0].error_message.as_deref(),
        Some("no pong")
    );
    assert!(scenarios[0].failed());
    assert!(!scenarios[1].failed());
    let summary = Summary::of(&scenarios);
    assert_eq!(summary.scenarios, 2);
    assert_eq!(summary.checks, 3);
    assert_eq!(
        summary.checks_by_status,
        BTreeMap::from([
            ("FAILURE".to_owned(), 1),
            ("INFO".to_owned(), 1),
            ("SUCCESS".to_owned(), 1)
        ])
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_client_run_keeps_its_names_and_finds_nested_auth_scenarios() {
    let dir = scratch("client");
    write_checks(
        &dir,
        "initialize-2026-10-13T11-01-48-651Z",
        r#"[{"id":"mcp-client-initialization","status":"SUCCESS"}]"#,
    );
    write_checks(
        &dir,
        "auth/metadata-default-2026-10-13T11-01-50-000Z",
        r#"[{"id":"x","status":"WARNING"}]"#,
    );
    let scenarios = read_results(&dir).unwrap();
    let names: Vec<&str> = scenarios.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["auth/metadata-default", "initialize"]);
    assert!(scenarios[0].failed(), "a warning fails the suite's run");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn unreadable_results_are_named() {
    let dir = scratch("bad");
    write_checks(&dir, "ping-2026-10-13T11-01-48-651Z", "{not an array}");
    let error = read_results(&dir).unwrap_err();
    assert!(error.contains("checks.json is not valid"), "{error}");
    std::fs::remove_dir_all(&dir).unwrap();
    let error = read_results(&dir).unwrap_err();
    assert!(error.starts_with("cannot read "), "{error}");
}

#[test]
fn an_empty_results_directory_has_no_scenarios() {
    let dir = scratch("empty");
    assert_eq!(read_results(&dir).unwrap(), Vec::new());
    assert_eq!(Summary::of(&[]), Summary::default());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_pins_are_exact_versions() {
    // An exact version is what lets npx refuse to substitute another; a range
    // or a tag here would make every run's suite a matter of the day.
    assert!(
        VERSION.split('.').count() == 3 && VERSION.bytes().all(|b| b.is_ascii_digit() || b == b'.'),
        "{VERSION}"
    );
    assert!(
        SPEC_VERSION
            .parse::<mcp_conformance_core::revision::ProtocolRevision>()
            .is_ok()
    );
    assert!(PACKAGE.starts_with("@modelcontextprotocol/"));
}
