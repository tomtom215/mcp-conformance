// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The reload is not done under the library's lock.

#![allow(clippy::unwrap_used)]

use super::tests::shared;
use super::*;

/// A trace that takes a while to judge: a handshake, then `count` pings.
fn many_pings(count: u64) -> String {
    let mut text = String::new();
    let mut seq = 0_u64;
    let mut push = |direction: &str, payload: &str| {
        use std::fmt::Write as _;
        let _ = writeln!(
            text,
            r#"{{"seq":{seq},"direction":"{direction}","transport":"stdio","kind":"message","payload":{payload}}}"#
        );
        seq += 1;
    };
    push(
        "client-to-server",
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}"#,
    );
    push(
        "server-to-client",
        r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}"#,
    );
    push(
        "client-to-server",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    );
    for id in 2..count {
        push(
            "client-to-server",
            &format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"ping"}}"#),
        );
        push(
            "server-to-client",
            &format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{}}}}"#),
        );
    }
    text
}

/// The audit's A12: the library's one lock was held across the reload, so after
/// a recorder appended to a followed trace an unrelated request waited for the
/// whole trace to be judged again (0.6 to 7.7 s, measured, at 10k to 100k
/// events). The reload runs with the lock released.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reload_does_not_hold_the_librarys_lock() {
    let (shared, dir) = shared("reload-lock");
    std::fs::write(dir.join("big.jsonl"), many_pings(20_000)).unwrap();

    let reloading = tokio::spawn(blocking(Arc::clone(&shared), |wb| Ok(wb.entries.len())));
    // Let the reload start, then take the lock as an unrelated request would.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let probe = Arc::clone(&shared);
    let started = std::time::Instant::now();
    tokio::task::spawn_blocking(move || {
        drop(
            probe
                .workbench
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
    })
    .await
    .unwrap();
    let waited = started.elapsed();
    assert!(
        waited < std::time::Duration::from_millis(500),
        "an unrelated request waited {waited:?} for the reload"
    );
    assert!(
        !reloading.is_finished(),
        "the reload was still running when the lock was taken: this checks nothing on a machine that judges 40,000 events in 100 ms"
    );
    assert!(
        reloading.await.unwrap().unwrap() > 11,
        "the reload then completed"
    );
}
