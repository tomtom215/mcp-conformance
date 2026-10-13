// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A self-contained HTML page of validation reports.
//!
//! It is the file attached to a bug report against an SDK
//! ([ADR-0021](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/decisions/0021-a-workbench-people-keep-open.md),
//! decision 2): one file, readable offline and forever.
//!
//! | Part | What it shows |
//! |------|---------------|
//! | verdict | the verdict and every outcome's count, per judged revision, and the wire-schema outcome |
//! | clauses to act on | each failing, warning or unsupported clause: ID, level, the quote, its published link, and every finding with its `seq` |
//! | message timeline | every event — direction, kind, method and id, which request a response answers, HTTP exchanges — with each finding at its event, linked both ways |
//! | accounting | the passed, not-observed, not-applicable and excluded clauses, collapsed, each with its reason |
//!
//! **Nothing on the page loads anything.** There is no script, and the
//! stylesheet is inline, admitted by a Content Security Policy that names its
//! SHA-256 digest (computed at render time) and refuses everything else —
//! fonts, images, frames, connections, other styles. The interactions need no
//! script: collapsible sections are `<details>`, a finding and its event link
//! to each other by fragment, and the "only events with findings" switch is a
//! checkbox a CSS sibling selector reads. Links to the specification are plain
//! `<a>` elements, followed only when clicked.
//!
//! **Everything from the trace is text.** Method names, ids, headers, payloads
//! and findings are escaped ([`escape::Text`]); only `http(s)` URLs become
//! links. A trace holding `<script>` renders the word, not the element.
//!
//! **Deterministic.** No clock, no environment: the same reports and events
//! give the same bytes on every platform. The page names the tool's version,
//! so a release changes every page by that line.

use core::fmt::Write as _;

use mcp_conformance_core::trace::{TraceEvent, TransportKind};

use crate::report::{Report, Unjudged, Verdict};

mod clauses;
mod digest;
mod escape;
mod summary;
mod timeline;

use escape::Text;

/// The stylesheet, inline in every page. Its digest is the only style source
/// the page's Content Security Policy admits.
const STYLE: &str = include_str!("html/style.css");

/// One trace as the page shows it.
#[derive(Debug, Clone, Copy)]
pub struct Trace<'a> {
    /// The trace as the caller named it (its path); `None` when it has no name
    /// (read from stdin).
    pub name: Option<&'a str>,
    /// The trace's events, in document order.
    pub events: &'a [TraceEvent],
}

/// Renders one trace's reports — one per judged revision — as a
/// self-contained HTML page.
///
/// ```
/// use mcp_trace_validator::{html, judge};
///
/// let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"<b>c</b>","version":"0"}}}}
/// {"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}
/// {"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#;
/// let judgment = judge::judge(trace)?;
/// let page = html::render(
///     &judgment.reports,
///     html::Trace { name: Some("session.jsonl"), events: &judgment.events },
/// );
/// assert!(page.starts_with("<!DOCTYPE html>"));
/// assert!(page.contains("Content-Security-Policy"));
/// assert!(page.contains("&lt;b&gt;c&lt;/b&gt;"), "trace text is escaped");
/// assert!(!page.contains("<script"), "and the page has no script");
/// # Ok::<(), judge::JudgeError>(())
/// ```
#[must_use]
pub fn render(reports: &[Report], trace: Trace<'_>) -> String {
    render_run(&[(trace, reports)], &[])
}

/// Renders a run over several traces as one page: the run's verdict, a section
/// per judged trace laid out as [`render`] lays out one, and each trace that
/// could not be judged with its reason.
///
/// One judged trace and nothing unjudged is exactly [`render`]'s page.
///
/// ```
/// use mcp_trace_validator::html;
/// use mcp_trace_validator::report::Unjudged;
///
/// let unjudged = [Unjudged::new("bad.jsonl", "malformed trace: line 4")];
/// let page = html::render_run(&[], &unjudged);
/// assert!(page.contains("incomplete"));
/// assert!(page.contains("malformed trace: line 4"));
/// ```
#[must_use]
pub fn render_run(traces: &[(Trace<'_>, &[Report])], unjudged: &[Unjudged]) -> String {
    let mut out = String::with_capacity(64 * 1024);
    if let ([(trace, reports)], []) = (traces, unjudged) {
        let scope = Scope::new(*trace, reports, "", 2);
        head(&mut out, &page_title(trace_verdict(reports), &scope.name()));
        out.push_str("<body>\n");
        summary::masthead(&mut out, &scope, "MCP trace validation report");
        out.push_str("<main id=\"main\" class=\"wrap\">\n");
        trace_body(&mut out, &scope);
    } else {
        let overall = run_verdict(traces, unjudged);
        let name = format!("{} traces", traces.len() + unjudged.len());
        head(&mut out, &page_title(overall, &name));
        out.push_str("<body>\n");
        summary::run_masthead(&mut out, traces.len() + unjudged.len());
        out.push_str("<main id=\"main\" class=\"wrap\">\n");
        summary::run_summary(&mut out, traces, unjudged, overall);
        for (index, (trace, reports)) in traces.iter().enumerate() {
            let scope = Scope::new(*trace, reports, &format!("t{index}-"), 3);
            let _ = writeln!(
                out,
                "<section class=\"trace\" id=\"t{index}\" aria-labelledby=\"t{index}-title\">"
            );
            summary::trace_heading(&mut out, &scope, index);
            trace_body(&mut out, &scope);
            out.push_str("</section>\n");
        }
    }
    out.push_str("</main>\n");
    footer(&mut out);
    out.push_str("</body>\n</html>\n");
    out
}

/// One trace's verdict, sections and accounting, under its header.
fn trace_body(out: &mut String, scope: &Scope<'_>) {
    summary::verdict(out, scope);
    summary::mismatch_notes(out, scope);
    summary::contents(out, scope);
    clauses::attention(out, scope);
    timeline::write(out, scope);
    clauses::accounting(out, scope);
}

/// The document head: the policy, the stylesheet it admits, and the title.
fn head(out: &mut String, title: &str) {
    // `default-src 'none'` refuses every fetch; the stylesheet's digest is the
    // one exception. `base-uri` and `form-action` are not covered by
    // `default-src`, so they are refused by name.
    let policy = format!(
        "default-src 'none'; style-src {}; base-uri 'none'; form-action 'none'",
        digest::csp_source(STYLE)
    );
    let _ = write!(
        out,
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{policy}\">\n\
         <meta name=\"referrer\" content=\"no-referrer\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta name=\"color-scheme\" content=\"light dark\">\n\
         <meta name=\"generator\" content=\"{}\">\n<title>{}</title>\n<style>{STYLE}</style>\n</head>\n",
        generator(),
        Text(title)
    );
}

fn footer(out: &mut String) {
    let _ = write!(
        out,
        "<footer class=\"wrap foot\">\n<p>Generated by {}.</p>\n<p>The same trace gives this \
         page byte for byte: nothing on it came from a clock, the network or the machine that \
         rendered it. It loads nothing — no script, font or image — and its links to the \
         specification open only when followed.</p>\n</footer>\n",
        Text(&generator())
    );
}

/// The tool and version that rendered the page.
fn generator() -> String {
    format!("mcp-trace-validator {}", env!("CARGO_PKG_VERSION"))
}

fn page_title(verdict: Verdict, name: &str) -> String {
    format!(
        "{} — {name} — MCP trace validation",
        summary::verdict_word(verdict)
    )
}

/// The verdict over one trace's reports: the most severe.
fn trace_verdict(reports: &[Report]) -> Verdict {
    Verdict::most_severe(reports.iter().map(Report::verdict)).unwrap_or(Verdict::Pass)
}

/// The run's verdict: the most severe of the traces', or incomplete when one
/// could not be judged — the rule every other format applies.
fn run_verdict(traces: &[(Trace<'_>, &[Report])], unjudged: &[Unjudged]) -> Verdict {
    let incomplete = unjudged.first().map(|_| Verdict::Incomplete);
    Verdict::most_severe(
        traces
            .iter()
            .map(|(_, reports)| trace_verdict(reports))
            .chain(incomplete),
    )
    .unwrap_or(Verdict::Incomplete)
}

/// One trace being rendered: its reports, and the names its anchors take.
///
/// Anchors are readable (`#BASE-003`, `#e12`) so a link into a report can be
/// written by hand. A page holding several revisions qualifies clause anchors
/// with the revision (`#2026-07-28-BASE-045`); a page holding several traces
/// prefixes every anchor with the trace's position (`#t1-e12`).
struct Scope<'a> {
    trace: Trace<'a>,
    reports: &'a [Report],
    prefix: String,
    /// The heading level of the trace's sections (`2` under the page's `h1`).
    level: u8,
}

impl<'a> Scope<'a> {
    fn new(trace: Trace<'a>, reports: &'a [Report], prefix: &str, level: u8) -> Self {
        Self {
            trace,
            reports,
            prefix: prefix.to_owned(),
            level,
        }
    }

    /// The trace's name as shown.
    fn name(&self) -> String {
        self.trace
            .name
            .map_or_else(|| "standard input".to_owned(), ToOwned::to_owned)
    }

    /// Whether the page shows several revisions of this trace.
    const fn multi(&self) -> bool {
        self.reports.len() > 1
    }

    /// An anchor in this trace's namespace.
    fn id(&self, name: &str) -> String {
        format!("{}{}", self.prefix, escape::anchor(name))
    }

    /// The anchor of the event `seq`.
    fn event(&self, seq: u64) -> String {
        self.id(&format!("e{seq}"))
    }

    /// The anchor of the clause `id` under `revision`'s report.
    fn clause(&self, revision: &str, id: &str) -> String {
        if self.multi() {
            self.id(&format!("{revision}-{id}"))
        } else {
            self.id(id)
        }
    }

    /// How the transport reads: the one the events used, or "mixed".
    fn transport(&self) -> &'static str {
        let mut kinds = self.trace.events.iter().map(|event| event.transport);
        match kinds.next() {
            None => "no transport",
            Some(first) if kinds.all(|kind| kind == first) => match first {
                TransportKind::Stdio => "stdio",
                TransportKind::StreamableHttp => "Streamable HTTP",
                _ => "another transport",
            },
            Some(_) => "mixed transports",
        }
    }
}

#[cfg(test)]
mod tests;
