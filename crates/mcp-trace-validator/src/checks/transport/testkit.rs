// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Shared fixtures for the stimulus-conditional transport check tests: a small
//! builder that numbers events as it goes, so a test reads as the exchange it
//! describes.

#![allow(clippy::unwrap_used)]

use mcp_conformance_core::trace::TraceEvent;

use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};

/// A `ping` request with the given id.
pub(super) fn ping(id: u32) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"ping"}}"#)
}

/// An empty result for the given id.
pub(super) fn pong(id: u32) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{}}}}"#)
}

/// A Streamable HTTP trace under construction.
#[derive(Default)]
pub(super) struct Session {
    lines: Vec<String>,
}

impl Session {
    fn push(&mut self, direction: &str, rest: &str) {
        let seq = self.lines.len();
        self.lines.push(format!(
            r#"{{"seq":{seq},"direction":"{direction}","transport":"streamable-http",{rest}}}"#
        ));
    }

    /// A client `http` event: `method` with `headers`, a JSON object.
    pub(super) fn request(mut self, method: &str, headers: &str) -> Self {
        self.push(
            "client-to-server",
            &format!(r#""kind":"http","method":"{method}","headers":{headers}"#),
        );
        self
    }

    /// A client message.
    pub(super) fn sends(mut self, payload: &str) -> Self {
        self.push(
            "client-to-server",
            &format!(r#""kind":"message","payload":{payload}"#),
        );
        self
    }

    /// A server `http` event: `status` with `headers`, a JSON object.
    pub(super) fn status(mut self, status: u16, headers: &str) -> Self {
        self.push(
            "server-to-client",
            &format!(r#""kind":"http","status":{status},"headers":{headers}"#),
        );
        self
    }

    /// A server message.
    pub(super) fn answers(mut self, payload: &str) -> Self {
        self.push(
            "server-to-client",
            &format!(r#""kind":"message","payload":{payload}"#),
        );
        self
    }

    /// A POST carrying `payload` under `headers`, answered `status`.
    pub(super) fn post(self, headers: &str, payload: &str, status: u16) -> Self {
        self.request("POST", headers)
            .sends(payload)
            .status(status, "{}")
    }

    /// A bodiless `method` request under `headers`, answered `status`.
    pub(super) fn bare(self, method: &str, headers: &str, status: u16) -> Self {
        self.request(method, headers).status(status, "{}")
    }

    /// The findings `check` reports on this trace, and its subject count.
    pub(super) fn run(&self, check: &str) -> (Vec<String>, u32) {
        let document = self.lines.join("\n");
        let events: Vec<TraceEvent> = parse_trace(&document, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        let outcome = checks::find(check).unwrap().run(&context);
        (
            outcome
                .findings
                .into_iter()
                .map(|finding| finding.detail)
                .collect(),
            outcome.subjects,
        )
    }

    /// The subject count alone, asserting `check` reported nothing.
    pub(super) fn clean(&self, check: &str) -> u32 {
        let (findings, subjects) = self.run(check);
        assert!(findings.is_empty(), "{findings:?}");
        subjects
    }

    /// The one finding `check` reports, asserting it is the only one.
    pub(super) fn one(&self, check: &str) -> String {
        let (mut findings, subjects) = self.run(check);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(subjects, 1, "a single finding on a single subject");
        findings.remove(0)
    }
}
