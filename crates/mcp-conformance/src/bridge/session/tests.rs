// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use serde_json::json;

use super::*;

fn session() -> (Session, DuplexStream) {
    let (writer, reader) = tokio::io::duplex(1024);
    (Session::new(writer), reader)
}

fn drain(receiver: &mut mpsc::UnboundedReceiver<String>) -> Vec<String> {
    let mut lines = Vec::new();
    while let Ok(line) = receiver.try_recv() {
        lines.push(line);
    }
    lines
}

#[test]
fn ids_are_read_from_requests_and_responses_only() {
    assert_eq!(
        request_ids(&json!({"jsonrpc":"2.0","id":1,"method":"ping"})),
        ["1"]
    );
    assert_eq!(
        request_ids(&json!({"jsonrpc":"2.0","id":"a","method":"x"})),
        ["\"a\""]
    );
    assert_eq!(
        request_ids(&json!({"jsonrpc":"2.0","method":"notifications/initialized"})).len(),
        0
    );
    assert_eq!(
        request_ids(&json!({"jsonrpc":"2.0","id":1,"result":{}})).len(),
        0
    );
    assert_eq!(
        request_ids(
            &json!([{"id":1,"method":"a"},{"method":"n"},{"id":2,"method":"b"},{"id":3,"result":{}}])
        ),
        ["1", "2"]
    );
    assert_eq!(request_ids(&json!("text")).len(), 0);

    assert_eq!(
        response_id(&json!({"id":7,"result":{}})).as_deref(),
        Some("7")
    );
    assert_eq!(
        response_id(&json!({"id":"x","error":{}})).as_deref(),
        Some("\"x\"")
    );
    assert_eq!(response_id(&json!({"id":7,"method":"ping"})), None);
    assert_eq!(response_id(&json!({"id":7,"method":"m","result":{}})), None);
    assert_eq!(response_id(&json!({"id":7})), None);
    assert_eq!(response_id(&json!({"result":{}})), None);
    assert_eq!(response_id(&json!([1])), None);
}

#[test]
fn a_response_ends_its_stream_once_every_request_is_answered() {
    let (session, _reader) = session();
    let mut first = session.open_stream(vec!["1".into(), "2".into()]);
    let mut second = session.open_stream(vec!["3".into()]);
    session.route(r#"{"jsonrpc":"2.0","id":2,"result":{}}"#.into());
    session.route(r#"{"jsonrpc":"2.0","id":3,"result":{}}"#.into());
    assert_eq!(
        drain(&mut first),
        [r#"{"jsonrpc":"2.0","id":2,"result":{}}"#]
    );
    assert!(!first.is_closed(), "still waiting for 1");
    assert_eq!(
        drain(&mut second),
        [r#"{"jsonrpc":"2.0","id":3,"result":{}}"#]
    );
    assert!(second.is_closed(), "its one request is answered");
    session.route(r#"{"jsonrpc":"2.0","id":1,"error":{"code":1,"message":"m"}}"#.into());
    assert_eq!(drain(&mut first).len(), 1);
    assert!(first.is_closed());
}

#[test]
fn server_messages_go_on_the_newest_open_stream_then_standalone_then_wait() {
    let (session, _reader) = session();
    let progress = r#"{"jsonrpc":"2.0","method":"notifications/progress"}"#;
    // Nothing open: it waits, and the standalone stream gets it first.
    session.route(progress.into());
    let mut older = session.open_stream(vec!["1".into()]);
    let mut newer = session.open_stream(vec!["2".into()]);
    session.route(r#"{"jsonrpc":"2.0","id":9,"method":"sampling/createMessage"}"#.into());
    assert_eq!(drain(&mut older).len(), 0);
    assert_eq!(
        drain(&mut newer).len(),
        1,
        "the newest open stream carries it"
    );
    drop(newer);
    session.route(progress.into());
    assert_eq!(drain(&mut older), [progress], "a closed stream is skipped");
    drop(older);
    session.route(r#"{"jsonrpc":"2.0","method":"notifications/message"}"#.into());
    let mut standalone = session.open_standalone();
    assert_eq!(
        drain(&mut standalone),
        [
            progress,
            r#"{"jsonrpc":"2.0","method":"notifications/message"}"#
        ],
        "what waited arrives first, in order"
    );
    // An unmatched response is routed like any other server message.
    session.route(r#"{"jsonrpc":"2.0","id":42,"result":{}}"#.into());
    assert_eq!(drain(&mut standalone).len(), 1);
    drop(standalone);
    session.route(progress.into());
    let mut again = session.open_standalone();
    assert_eq!(
        drain(&mut again),
        [progress],
        "a gone standalone stream is replaced"
    );
}

#[test]
fn a_line_that_is_not_json_is_not_forwarded() {
    let (session, _reader) = session();
    let mut stream = session.open_stream(vec!["1".into()]);
    session.route("starting up...".into());
    assert_eq!(drain(&mut stream).len(), 0);
}

#[test]
fn waiting_messages_are_bounded() {
    let (session, _reader) = session();
    for _ in 0..PENDING_LIMIT + 5 {
        session.route(r#"{"jsonrpc":"2.0","method":"n"}"#.into());
    }
    let mut standalone = session.open_standalone();
    assert_eq!(drain(&mut standalone).len(), PENDING_LIMIT);
}

#[test]
fn when_the_server_ends_every_stream_ends_and_none_opens() {
    let (session, _reader) = session();
    let stream = session.open_stream(vec!["1".into()]);
    let standalone = session.open_standalone();
    session.route(r#"{"jsonrpc":"2.0","method":"n"}"#.into());
    session.end();
    assert!(stream.is_closed());
    assert!(standalone.is_closed());
    assert!(session.open_stream(vec!["2".into()]).is_closed());
    assert!(session.open_standalone().is_closed());
}

#[tokio::test]
async fn lines_reach_the_server_until_the_session_closes() {
    let (session, reader) = session();
    session.send(b"{\"id\":1}\n").await.unwrap();
    session.close().await;
    let error = session.send(b"x\n").await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    let mut lines = tokio::io::BufReader::new(reader).lines();
    assert_eq!(
        lines.next_line().await.unwrap().as_deref(),
        Some("{\"id\":1}")
    );
    assert_eq!(
        lines.next_line().await.unwrap(),
        None,
        "closing is end of stream"
    );
}

#[tokio::test]
async fn route_output_routes_until_end_of_output() {
    let (session, _reader) = session();
    let session = std::sync::Arc::new(session);
    let mut stream = session.open_stream(vec!["1".into()]);
    let (mut server, answers) = tokio::io::duplex(1024);
    server
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"n\"}\n{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n")
        .await
        .unwrap();
    drop(server);
    route_output(std::sync::Arc::clone(&session), answers).await;
    assert_eq!(drain(&mut stream).len(), 2);
    assert!(session.open_stream(vec!["2".into()]).is_closed(), "ended");
}
