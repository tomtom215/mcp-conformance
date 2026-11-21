// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use super::{Kind, Skipped, Summary};

fn skipped(kind: &'static str, name: &str, reason: &str) -> Skipped {
    Skipped {
        kind,
        name: name.to_owned(),
        reason: reason.to_owned(),
    }
}

#[test]
#[allow(clippy::too_many_lines)] // one literal summary and its rendering
fn the_account_says_what_was_exercised_and_groups_what_was_not_by_reason() {
    let summary = Summary {
        subject: "python server.py".to_owned(),
        transport: "stdio",
        revision: Some("2025-11-25".to_owned()),
        negotiation: "opened with `initialize`".to_owned(),
        server: Some("demo 1.0".to_owned()),
        requests: 9,
        tools: Kind {
            declared: true,
            listed: 4,
            exercised: 2,
            errors: 1,
        },
        resources: Kind {
            declared: true,
            listed: 1,
            exercised: 1,
            errors: 0,
        },
        prompts: Kind::default(),
        skipped: vec![
            skipped("tool", "write", "not read-only"),
            skipped("resource template", "t://{id}", "needs values"),
            skipped("tool", "delete", "not read-only"),
            skipped("tool", "odd", "no arguments"),
            skipped("prompt", "p", "requires arguments"),
        ],
        probes: vec![super::Probe {
            what: "prompts/get for a prompt the server does not list".to_owned(),
            answer: "refused with an error".to_owned(),
        }],
        needed_input: vec!["tools/call ask".to_owned()],
        server_requests: 2,
        unanswered: vec!["tools/call slow: no answer".to_owned()],
        stopped_by: Some("the request budget (--max-requests)".to_owned()),
        trace: "/tmp/t.jsonl".to_owned(),
    };
    assert_eq!(
        summary.render(),
        "mcp-conformance check: python server.py (stdio)
  revision   2025-11-25 — opened with `initialize`
  server     demo 1.0
  tools      4 listed, 2 called (1 answered with an error)
  resources  1 listed, 1 read
  prompts    not declared by the server, so not asked for
  not called:
    write, delete — not read-only
    odd — no arguments
  not read:
    t://{id} — needs values
  not fetched:
    p — requires arguments
  probes     1 sent: requests made wrong on purpose, to see how the server refuses them (--no-probes leaves them out)
    prompts/get for a prompt the server does not list — refused with an error
  asked the client for input, which this client does not give: tools/call ask
  the server sent this client 2 request(s), though it declared no capabilities
  unanswered tools/call slow: no answer
  stopped early: the request budget (--max-requests)
  requests   9 sent
  trace      /tmp/t.jsonl
"
    );
}

#[test]
fn a_run_that_established_nothing_says_so_and_adds_no_empty_sections() {
    let summary = Summary {
        subject: "x".to_owned(),
        transport: "streamable-http",
        negotiation: "nothing answered".to_owned(),
        trace: "t".to_owned(),
        ..Summary::default()
    };
    assert_eq!(
        summary.render(),
        "mcp-conformance check: x (streamable-http)
  revision   none established — nothing answered
  tools      not declared by the server, so not asked for
  resources  not declared by the server, so not asked for
  prompts    not declared by the server, so not asked for
  requests   0 sent
  trace      t
"
    );
}

#[test]
fn long_lists_of_names_are_counted_and_control_characters_never_reach_the_terminal() {
    let mut summary = Summary::default();
    for index in 0..11 {
        summary
            .skipped
            .push(skipped("tool", &format!("t{index}"), "r"));
    }
    summary.server = Some("evil\u{1b}[2J".to_owned());
    summary.skipped.push(skipped("prompt", "p\u{7}", "r"));
    let text = summary.render();
    assert!(
        text.contains("t0, t1, t2, t3, t4, t5, t6, t7 and 3 more — r"),
        "{text}"
    );
    assert!(
        !text.contains('\u{1b}') && !text.contains('\u{7}'),
        "{text:?}"
    );
}

#[test]
fn the_json_form_carries_the_same_facts() {
    let summary = Summary {
        transport: "stdio",
        tools: Kind {
            declared: true,
            listed: 1,
            exercised: 0,
            errors: 0,
        },
        skipped: vec![skipped("tool", "w", "not read-only")],
        ..Summary::default()
    };
    let json = serde_json::to_value(&summary).unwrap_or_default();
    assert_eq!(json["tools"]["listed"], 1);
    assert_eq!(json["skipped"][0]["reason"], "not read-only");
    assert_eq!(json["revision"], serde_json::Value::Null);
}

#[test]
fn exactly_as_many_names_as_a_line_shows_are_all_shown_and_none_counted() {
    let mut summary = Summary::default();
    for index in 0..8 {
        summary
            .skipped
            .push(skipped("tool", &format!("t{index}"), "r"));
    }
    let text = summary.render();
    assert!(
        text.contains("t0, t1, t2, t3, t4, t5, t6, t7 — r"),
        "{text}"
    );
    assert!(!text.contains("more"), "{text}");
}
