// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The real binary over the committed fixture library: `serve` as a process
//! answering over loopback, and `library`'s subcommands — above all that the
//! dashboards' data endpoint is byte for byte what `library stats --format
//! json` prints.

use std::io::{BufRead as _, BufReader};
use std::process::Stdio;

use super::*;

fn fixture_library(name: &str) -> PathBuf {
    let dir = scratch(name).join("library");
    std::fs::create_dir_all(&dir).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../visual/fixtures/library");
    for file in std::fs::read_dir(fixtures).unwrap() {
        let path = file.unwrap().path();
        std::fs::copy(&path, dir.join(path.file_name().unwrap())).unwrap();
    }
    dir
}

fn library(dir: &Path, args: &[&str]) -> Output {
    binary()
        .arg("library")
        .arg("--library")
        .arg(dir)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[tokio::test]
async fn the_dashboards_endpoint_is_what_library_stats_prints() {
    let dir = fixture_library("serve-stats");
    let printed = library(&dir, &["stats", "--format", "json"]);
    assert!(printed.status.success(), "{}", stderr(&printed));
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_mcp-conformance"))
        .arg("serve")
        .arg("--library")
        .arg(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let link = loop {
        let line = lines.next().expect("serve printed its link").unwrap();
        if let Some(link) = line.strip_prefix("mcp-conformance serve: open ") {
            break link.to_owned();
        }
    };
    let origin = link.split("/?").next().unwrap().to_owned();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    assert_eq!(
        client
            .get(format!("{origin}/api/stats"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let launched = client.get(&link).send().await.unwrap();
    assert_eq!(launched.status(), 303);
    let cookie = launched.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let served = client
        .get(format!("{origin}/api/stats"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(served.status(), 200);
    assert_eq!(served.headers()["x-content-type-options"], "nosniff");
    assert_eq!(served.text().await.unwrap(), stdout(&printed));
    // The link is spent.
    assert_eq!(client.get(&link).send().await.unwrap().status(), 403);
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn serve_refuses_a_non_loopback_address_without_the_flag() {
    let dir = scratch("serve-wide");
    let output = binary()
        .args(["serve", "--listen", "0.0.0.0:0", "--library"])
        .arg(&dir)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("--dangerously-listen-non-loopback"),
        "{}",
        stderr(&output)
    );
}

// One library through every subcommand in order: each step reads the last.
#[allow(clippy::too_many_lines)]
#[test]
fn the_library_is_administered_from_the_command_line() {
    let dir = fixture_library("library-cli");
    let list = library(&dir, &["list"]);
    assert!(list.status.success());
    let text = stdout(&list);
    assert_eq!(text.lines().count(), 11);
    assert!(
        text.contains("base-001-request-id-boolean\tfail\t2026-10-09\tcorpus-server 0.1.0"),
        "{text}"
    );
    let human = stdout(&library(&dir, &["stats"]));
    assert!(human.starts_with("11 recordings"), "{human}");

    let capture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/mcpsnoop/native/http-2026-07-28.mcpsnoop.jsonl");
    let imported = library(&dir, &["import", capture.to_str().unwrap()]);
    assert!(imported.status.success(), "{}", stderr(&imported));
    assert_eq!(stdout(&imported), "imported http-2026-07-28.mcpsnoop\n");
    let meta = std::fs::read_to_string(dir.join("http-2026-07-28.mcpsnoop.meta.json")).unwrap();
    assert!(
        meta.contains("\"importer\": \"mcpsnoop\"") && meta.contains("Mcp-Session-Id"),
        "{meta}"
    );

    let redacted = library(
        &dir,
        &[
            "redact",
            "leaky-session",
            "--meta",
            "--pointer",
            "/params/arguments/apiKey",
            "--string",
            "fixture-api-key-3",
        ],
    );
    assert!(redacted.status.success(), "{}", stderr(&redacted));
    let copy = std::fs::read_to_string(dir.join("leaky-session.redacted.jsonl")).unwrap();
    for secret in [
        "fixture-not-a-real-token-1",
        "fixture-cookie-2",
        "fixture-api-key-3",
        "fixture-meta-4",
    ] {
        assert!(!copy.contains(secret), "{secret}");
    }

    let tagged = library(
        &dir,
        &["tag", "everything-http", "--tag", "b", "--tag", "a"],
    );
    assert_eq!(stdout(&tagged), "everything-http: a, b\n");

    let archive = dir.parent().unwrap().join("export.tar");
    let exported = library(
        &dir,
        &["export", "everything-http", "-o", archive.to_str().unwrap()],
    );
    assert!(exported.status.success(), "{}", stderr(&exported));
    assert_eq!(&std::fs::read(&archive).unwrap()[257..262], b"ustar");

    let listed = library(&dir, &["prune", "--retain-days", "0"]);
    assert!(
        stdout(&listed).contains("would delete everything-http"),
        "{}",
        stdout(&listed)
    );
    assert!(
        dir.join("everything-http.jsonl").exists(),
        "listing deletes nothing"
    );
    let kept = library(&dir, &["prune", "--retain-days", "100000", "--apply"]);
    assert!(stdout(&kept).starts_with("nothing recorded"));

    let deleted = library(&dir, &["delete", "everything-http", "tool-005-name-length"]);
    assert!(deleted.status.success());
    assert!(!dir.join("everything-http.jsonl").exists());
    let missing = library(&dir, &["delete", "everything-http"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(stderr(&missing).contains("no entry everything-http"));
    let bad = library(
        &dir,
        &["redact", "base-001-request-id-boolean", "--pointer", "nope"],
    );
    assert_eq!(bad.status.code(), Some(2));
}
