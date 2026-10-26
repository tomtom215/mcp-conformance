// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Who a finding is held against, and a report viewed for one party.
//!
//! `mcp-conformance test` drives a server with the official suite's client, and
//! that client breaks clauses of its own — it calls `logging/setLevel` on a
//! server that never declared `logging`, or opens a session and closes it
//! without `notifications/initialized`. Every such finding is real, and none of
//! it is the server author's to fix. So each finding names the party it is
//! held against, and a report can be *viewed* for one party: the rows and
//! totals stay the trace's facts, each finding keeps its party, and only the
//! verdict is recomputed from the findings held against the party asked for —
//! plus those held against nobody, which a reader is told are counted rather
//! than quietly dropped.

use core::fmt;

use mcp_conformance_core::requirement::Actor;
use mcp_conformance_core::trace::Direction;
use serde::{Deserialize, Serialize};

use super::{Finding, Outcome, Report, Verdict};

/// One end of a session: who sent a message, and who a finding is held against.
///
/// A clause binds the client, the server or both ([`Actor`]); a finding is
/// held against exactly one of them, or against neither when the trace cannot
/// say ([`Finding::party`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Party {
    /// The client.
    Client,
    /// The server.
    Server,
}

impl Party {
    /// The party that sent a message travelling in `direction`.
    #[must_use]
    pub const fn sender(direction: Direction) -> Self {
        match direction {
            Direction::ClientToServer => Self::Client,
            Direction::ServerToClient => Self::Server,
        }
    }

    /// The party a clause binding `actor` holds a finding against, when the
    /// actor alone decides it: `None` for a clause binding both parties,
    /// which the event at the finding's `seq` has to settle.
    #[must_use]
    pub const fn of_actor(actor: Actor) -> Option<Self> {
        match actor {
            Actor::Client => Some(Self::Client),
            Actor::Server => Some(Self::Server),
            // `Both`, and any actor a future registry adds: nothing to say
            // from the clause alone.
            _ => None,
        }
    }

    /// The report token: `client` or `server`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Server => "server",
        }
    }

    /// The other end.
    #[must_use]
    pub const fn other(self) -> Self {
        match self {
            Self::Client => Self::Server,
            Self::Server => Self::Client,
        }
    }
}

impl fmt::Display for Party {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Finding {
    /// Whether this finding counts in a report viewed for `view`: held
    /// against that party, or against nobody the trace could name.
    #[must_use]
    pub fn counts_for(&self, view: Party) -> bool {
        self.party.is_none_or(|party| party == view)
    }
}

/// How the findings of a report fall when it is viewed for one party.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PartyTally {
    /// Findings held against the viewed party.
    pub counted: u32,
    /// Findings held against the other party, which the view sets aside.
    pub set_aside: u32,
    /// Findings held against nobody, which the view counts.
    pub unattributed: u32,
}

impl PartyTally {
    /// The tally of `findings`, the wire schema's included, viewed for `party`.
    pub(crate) fn of<'f>(findings: impl Iterator<Item = &'f Finding>, party: Party) -> Self {
        let mut tally = Self::default();
        for finding in findings {
            match finding.party {
                Some(held) if held == party => tally.counted += 1,
                Some(_) => tally.set_aside += 1,
                None => tally.unattributed += 1,
            }
        }
        tally
    }

    /// What the view did, for the verdict line: `2 client findings set aside`,
    /// `1 unattributed finding counted`, joined; empty when every finding is
    /// the viewed party's own.
    #[must_use]
    pub fn describe(&self, party: Party) -> String {
        let mut clauses = Vec::new();
        if self.set_aside > 0 {
            clauses.push(format!(
                "{} {} {} set aside",
                self.set_aside,
                party.other(),
                plural(self.set_aside)
            ));
        }
        if self.unattributed > 0 {
            clauses.push(format!(
                "{} unattributed {} counted",
                self.unattributed,
                plural(self.unattributed)
            ));
        }
        clauses.join(", ")
    }
}

const fn plural(count: u32) -> &'static str {
    if count == 1 { "finding" } else { "findings" }
}

/// Whether a `fail` or `warn` row still fails or warns when viewed for
/// `party`: one of its findings counts.
fn row_counts(findings: &[Finding], party: Party) -> bool {
    findings.iter().any(|finding| finding.counts_for(party))
}

impl Report {
    /// The verdict counting only the findings held against `party`, and those
    /// held against nobody: unsupported over fail over warnings over pass, as
    /// [`Self::verdict`] ranks them, where a `fail` or `warn` row — and a failed
    /// wire schema — counts only through a finding that counts.
    #[must_use]
    pub fn verdict_for(&self, party: Party) -> Verdict {
        if self.totals.unsupported > 0 {
            return Verdict::Unsupported;
        }
        let schema_failed = self.wire_schema.as_ref().is_some_and(|schema| {
            schema.outcome == Outcome::Fail && row_counts(&schema.findings, party)
        });
        let mut failed = schema_failed;
        let mut warned = false;
        for row in &self.requirements {
            match row.outcome {
                Outcome::Fail if row_counts(&row.findings, party) => failed = true,
                Outcome::Warn if row_counts(&row.findings, party) => warned = true,
                _ => {}
            }
        }
        if failed {
            Verdict::Fail
        } else if warned {
            Verdict::PassWithWarnings
        } else {
            Verdict::Pass
        }
    }

    /// How this report's findings — the rows' and the wire schema's — fall when
    /// viewed for `party`.
    #[must_use]
    pub fn party_tally(&self, party: Party) -> PartyTally {
        let rows = self.requirements.iter().flat_map(|row| &row.findings);
        let schema = self.wire_schema.iter().flat_map(|schema| &schema.findings);
        PartyTally::of(rows.chain(schema), party)
    }
}
