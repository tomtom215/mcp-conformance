// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! One session's spans, and which one each finding goes on.

use std::time::SystemTime;

use super::{Peer, Span, span};
use crate::live::{Exchange, LiveFinding};

/// A span not yet sent, with the `seq`s of its exchange.
#[derive(Debug)]
struct Unsent {
    span: Span,
    request: u64,
    response: Option<u64>,
}

impl Unsent {
    fn holds(&self, seq: Option<u64>) -> bool {
        seq.is_some_and(|seq| self.request == seq || self.response == Some(seq))
    }
}

/// One session's spans: one trace id, a fresh span id per exchange.
///
/// The latest span is held back until the next pass (or the end), so a
/// finding the pass after its exchange shows — or the final judgment shows —
/// still has an unsent span to go on. A finding goes on the unsent span of the
/// exchange holding its `seq`, else on the latest unsent span; with none
/// (no exchange has completed yet) it is counted, not sent.
pub struct Spans {
    trace_id: [u8; 16],
    peer: Option<Peer>,
    ids: Box<dyn FnMut() -> [u8; 8] + Send>,
    held: Option<Unsent>,
    unattached: u64,
}

impl std::fmt::Debug for Spans {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spans")
            .field("peer", &self.peer)
            .field("held", &self.held)
            .field("unattached", &self.unattached)
            .finish_non_exhaustive()
    }
}

impl Spans {
    /// A session's spans under `trace_id`, each span's id from `ids`; `peer`
    /// is the server of an HTTP session.
    #[must_use]
    pub fn new(
        trace_id: [u8; 16],
        peer: Option<Peer>,
        ids: Box<dyn FnMut() -> [u8; 8] + Send>,
    ) -> Self {
        Self {
            trace_id,
            peer,
            ids,
            held: None,
            unattached: 0,
        }
    }

    /// A judging pass: the exchanges completed since the last one, and the new
    /// findings it showed at `at`. Returns the spans ready to send — all but
    /// the latest.
    pub fn pass(
        &mut self,
        exchanges: &[Exchange],
        findings: &[LiveFinding],
        at: SystemTime,
    ) -> Vec<Span> {
        let mut unsent = self.unsent(exchanges, findings, at);
        self.held = unsent.pop();
        unsent.into_iter().map(|unsent| unsent.span).collect()
    }

    /// The end of the session: as [`Spans::pass`], and the held span too.
    pub fn finish(
        &mut self,
        exchanges: &[Exchange],
        findings: &[LiveFinding],
        at: SystemTime,
    ) -> Vec<Span> {
        self.unsent(exchanges, findings, at)
            .into_iter()
            .map(|unsent| unsent.span)
            .collect()
    }

    /// Findings no span could carry.
    #[must_use]
    pub const fn unattached(&self) -> u64 {
        self.unattached
    }

    fn unsent(
        &mut self,
        exchanges: &[Exchange],
        findings: &[LiveFinding],
        at: SystemTime,
    ) -> Vec<Unsent> {
        let mut unsent: Vec<Unsent> = self.held.take().into_iter().collect();
        for exchange in exchanges {
            let span_id = self.span_id();
            unsent.push(Unsent {
                span: span(exchange, self.trace_id, span_id, self.peer.as_ref()),
                request: exchange.request_seq,
                response: exchange.response_seq,
            });
        }
        for finding in findings {
            let place = unsent
                .iter()
                .position(|candidate| candidate.holds(finding.seq))
                .or_else(|| unsent.len().checked_sub(1));
            match place {
                Some(index) => unsent[index].span.add_finding(finding, at),
                None => self.unattached += 1,
            }
        }
        unsent
    }

    /// A span id that is not all zeros (OTLP's "invalid").
    fn span_id(&mut self) -> [u8; 8] {
        loop {
            let id = (self.ids)();
            if id != [0; 8] {
                return id;
            }
        }
    }
}
