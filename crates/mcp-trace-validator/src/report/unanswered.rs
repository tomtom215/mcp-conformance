// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The requests a trace ends without answering: a note on the report, never a
//! verdict.
//!
//! JSON-RPC expects a response to every request, and a server that exits with
//! a request pending, or never answers one, has left something unsaid. No
//! judgeable clause of either revision states it (`BASE-012` is excluded), so
//! the report does not invent a finding: it says what went unanswered and how
//! the trace ended, and leaves the verdict and exit status alone.

use core::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Party;

/// The most unanswered requests a report lists; the rest are counted.
pub const LISTED_UNANSWERED: usize = 10;

/// How a trace ended: what the recorder saw last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum TraceEnding {
    /// The last event is an orderly `transport-close`.
    TransportClose,
    /// The last event is a `transport-abort`: the transport failed or was torn
    /// down.
    TransportAbort,
    /// The trace simply stops: its last event is a message or an HTTP event.
    Stopped,
}

impl TraceEnding {
    /// The ending as the note's parenthesis reads it.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::TransportClose => "the transport closed",
            Self::TransportAbort => "the transport aborted",
            Self::Stopped => "the recording simply stopped",
        }
    }
}

/// One request the trace ended without answering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UnansweredRequest {
    /// The request's `method`.
    pub method: String,
    /// The request's `id`, as sent.
    pub id: Value,
    /// The event `seq` of the request.
    pub seq: u64,
    /// Who sent it — and so who was owed the answer.
    pub sender: Party,
}

impl UnansweredRequest {
    /// `client "tools/list" id 2 at seq 3`.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "{} {:?} id {} at seq {}",
            self.sender,
            self.method,
            serde_json::to_string(&self.id).unwrap_or_default(),
            self.seq
        )
    }
}

/// The requests that had no response when the trace ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UnansweredRequests {
    /// How many requests went unanswered.
    pub count: u32,
    /// How the trace ended.
    pub ending: TraceEnding,
    /// The first [`LISTED_UNANSWERED`] of them, in `seq` order.
    pub requests: Vec<UnansweredRequest>,
}

impl UnansweredRequests {
    /// The note's first sentence: `2 requests had no response when the trace
    /// ended (the transport closed)`.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} {} had no response when the trace ended ({})",
            self.count,
            if self.count == 1 {
                "request"
            } else {
                "requests"
            },
            self.ending.describe()
        )
    }

    /// How many unanswered requests the list leaves out.
    #[must_use]
    pub fn unlisted(&self) -> u32 {
        self.count
            .saturating_sub(u32::try_from(self.requests.len()).unwrap_or(u32::MAX))
    }

    /// The note on one line: the summary, then each listed request, then the
    /// count of the rest. What `JUnit` and SARIF carry.
    #[must_use]
    pub fn one_line(&self) -> String {
        let mut line = self.summary();
        line.push_str(": ");
        let listed: Vec<String> = self
            .requests
            .iter()
            .map(UnansweredRequest::describe)
            .collect();
        line.push_str(&listed.join("; "));
        let rest = self.unlisted();
        if rest > 0 {
            let _ = fmt::Write::write_fmt(&mut line, format_args!("; and {rest} more"));
        }
        line
    }
}

/// Why the note is a note: the sentence every format prints under it.
pub const UNANSWERED_WHY: &str = "JSON-RPC expects every request answered; no clause of this \
                                  revision judges it, so the verdict is unchanged.";
