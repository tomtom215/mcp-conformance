// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The feature pages' *Error Handling* clauses: which error code a server owes
//! a request it cannot serve.
//!
//! Every clause here has the same shape — "for failure case X, return code Y" —
//! and the same difficulty: whether X occurred is, in general, the server's own
//! knowledge. These checks are **stimulus-conditional**. Each one recognises a
//! request for which the *trace itself* establishes the failure case, judges
//! the answer to that request, and reports the clause as not observed in every
//! session that carries no such request. A conforming client rarely sends one;
//! a probe does, deliberately.
//!
//! Three rules keep them sound, and every check in the module applies them:
//!
//! - **The antecedent comes from the recording.** A prompt name is unknown only
//!   when the session holds a *complete* `prompts/list` enumeration that omits
//!   it and no `list_changed` notification ([`listing`]); a log level is
//!   invalid when it is none of the eight the schema enumerates; an argument is
//!   missing when the server's own listing marks it required.
//! - **A served result is the server's word that the case did not occur**,
//!   wherever the case is the server's to decide. A server may serve a prompt
//!   or a resource it does not list, so a result for an unlisted name is never
//!   judged: only a *refusal* of one is, and only for the code it carries.
//! - **An error naming another failure is never judged.** Each clause lists
//!   "Internal errors: `-32603`" beside the case it is judged on, and a trace
//!   cannot tell a server that hit one from a server that mislabelled the
//!   other; the same holds for the codes the specification gives a malformed
//!   or unacceptable *request* (`OTHER_FAILURES`).

use serde_json::Value;

use crate::context::Exchange;

pub(super) mod completion;
mod listing;
pub(super) mod logging;
pub(super) mod pagination;
pub(super) mod prompts;
pub(super) mod resources;

#[cfg(test)]
mod testkit;

/// JSON-RPC `Method not found`.
const METHOD_NOT_FOUND: i64 = -32601;

/// JSON-RPC `Invalid params`.
const INVALID_PARAMS: i64 = -32602;

/// The codes that name a failure of their own, none of which is the feature
/// page's "wrong code for this case":
///
/// - `-32603` `Internal error`, which every clause here lists beside the case
///   it is judged on;
/// - `-32700` and `-32600`, JSON-RPC's parse error and invalid request;
/// - `-32020`, `-32021` and `-32022`, which `2026-07-28` assigns to a header
///   mismatch, a missing client capability and an unsupported protocol
///   version — faults of the request's envelope, found before its parameters
///   are read;
/// - `-32042`, a URL elicitation the request needs first.
///
/// A request can be one of this module's stimuli *and* carry one of those
/// faults, and the server answering the fault is not mislabelling the stimulus.
const OTHER_FAILURES: [i64; 7] = [-32603, -32700, -32600, -32020, -32021, -32022, -32042];

/// How one request was answered.
#[derive(Clone, Copy)]
enum Answer<'a> {
    /// With a result.
    Result(&'a Value),
    /// With an error carrying this integer code, when it carries one.
    Error(Option<i64>),
}

impl<'a> Answer<'a> {
    /// The answer `exchange` ended in.
    fn of(exchange: &Exchange<'a>) -> Self {
        exchange.result.map_or_else(
            || {
                Self::Error(
                    exchange
                        .response
                        .message_payload()
                        .and_then(|payload| payload.get("error"))
                        .and_then(|error| error.get("code"))
                        .and_then(Value::as_i64),
                )
            },
            Self::Result,
        )
    }

    /// Whether this is an error naming a failure other than the one a check
    /// here looks for, and so an answer none of them judges.
    fn is_another_failure(self) -> bool {
        matches!(self, Self::Error(Some(code)) if OTHER_FAILURES.contains(&code))
    }

    /// Whether this is an error carrying exactly `code`.
    const fn is_error(self, code: i64) -> bool {
        matches!(self, Self::Error(Some(found)) if found == code)
    }

    /// The answer as a finding's detail names it.
    fn describe(self) -> String {
        match self {
            Self::Result(_) => "a result".to_owned(),
            Self::Error(Some(code)) => format!("error {code}"),
            Self::Error(None) => "an error with no integer code".to_owned(),
        }
    }
}
