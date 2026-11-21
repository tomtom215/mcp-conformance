// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The authorizing HTTP client over a scripted inner client: which refusals
//! it answers, with which token it sends again, when it stops, and that each
//! of the five trait methods reaches its own counterpart unchanged.

#![cfg(feature = "oauth")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "support/scripted.rs"]
mod scripted;
mod support;

use std::collections::HashMap;
use std::sync::Arc;

use futures::StreamExt as _;
use http::{HeaderName, HeaderValue};
use mcp_reference_host::oauth::REDIRECT_URI;
use mcp_reference_host::oauth::client::Authorizing;
use mcp_reference_host::oauth::consent::Headless;
use mcp_reference_host::oauth::context::Identity;
use mcp_reference_host::oauth::flow::Flow;
use rmcp::model::{ClientJsonRpcMessage, ClientNotification, InitializedNotification};
use rmcp::transport::auth::AuthError;
use rmcp::transport::streamable_http_client::{
    StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
};
use scripted::{Call, Failure, Headers, Reply, Scripted};
use support::{Answer, Site, serve};

async fn client(site: &Site, inner: Scripted, identity: Identity) -> Authorizing<Scripted> {
    let consent = Headless::new(REDIRECT_URI).unwrap();
    let flow = Flow::new(&site.mcp(), identity, Box::new(consent))
        .await
        .unwrap();
    Authorizing::new(inner, Arc::new(flow))
}

fn message() -> ClientJsonRpcMessage {
    ClientJsonRpcMessage::notification(ClientNotification::InitializedNotification(
        InitializedNotification::default(),
    ))
}

fn headers() -> Headers {
    HashMap::from([(
        HeaderName::from_static("x-test"),
        HeaderValue::from_static("1"),
    )])
}

async fn post(client: &Authorizing<Scripted>) -> Result<StreamableHttpPostResponse, Failure> {
    // The caller's own idea of a token is ignored: the flow's is the truth.
    let stale = Some("callers-token".to_owned());
    client
        .post_message("uri".into(), message(), Some("s".into()), stale, headers())
        .await
}

#[tokio::test]
async fn an_accepted_request_goes_out_once_with_no_token_and_no_authorization() {
    let site = serve();
    let inner = Scripted::new(&site, &[], Reply::Accept);
    let client = client(&site, inner.clone(), Identity::Discovered).await;

    let sent = post(&client).await.unwrap();

    assert!(matches!(sent, StreamableHttpPostResponse::Accepted));
    assert_eq!(inner.tokens(), [None]);
    assert_eq!(site.authorizations().len(), 0);
}

#[tokio::test]
async fn a_401_is_answered_by_authorizing_and_sending_again_with_the_token() {
    let site = serve();
    let inner = Scripted::new(&site, &[Reply::Unauthorized], Reply::Accept);
    let client = client(&site, inner.clone(), Identity::Discovered).await;

    post(&client).await.unwrap();

    assert_eq!(inner.tokens(), [None, Some("token-1".to_owned())]);
    // The challenge's scope was the one asked for: the header reached the flow.
    assert_eq!(site.authorizations()[0]["scope"], "mcp:basic");
    // The next request starts with the token already held.
    post(&client).await.unwrap();
    assert_eq!(inner.tokens()[2].as_deref(), Some("token-1"));
    assert_eq!(site.authorizations().len(), 1);
}

#[tokio::test]
async fn a_403_naming_a_scope_is_answered_by_escalating_and_sending_again() {
    let site = serve();
    let script = [Reply::Unauthorized, Reply::Forbidden(Some("mcp:write"))];
    let inner = Scripted::new(&site, &script, Reply::Accept);
    let client = client(&site, inner.clone(), Identity::Discovered).await;

    post(&client).await.unwrap();

    let tokens = inner.tokens();
    assert_eq!(
        tokens,
        [None, Some("token-1".to_owned()), Some("token-2".to_owned())]
    );
    assert_eq!(site.authorizations()[1]["scope"], "mcp:basic mcp:write");
}

#[tokio::test]
async fn a_403_naming_no_scope_is_the_outcome_and_nothing_is_asked_for() {
    let site = serve();
    let script = [Reply::Unauthorized, Reply::Forbidden(None)];
    let inner = Scripted::new(&site, &script, Reply::Accept);
    let client = client(&site, inner.clone(), Identity::Discovered).await;

    let refused = post(&client).await;

    assert!(
        matches!(refused, Err(StreamableHttpError::InsufficientScope(_))),
        "{refused:?}"
    );
    assert_eq!(inner.calls().len(), 2);
    assert_eq!(site.authorizations().len(), 1);
}

#[tokio::test]
async fn a_server_that_always_says_403_is_asked_three_times_and_then_believed() {
    let site = serve();
    let inner = Scripted::new(
        &site,
        &[Reply::Unauthorized],
        Reply::Forbidden(Some("mcp:admin")),
    );
    let client = client(&site, inner.clone(), Identity::Discovered).await;

    let refused = post(&client).await;

    // The refusal itself comes back, not an error about the budget.
    match refused {
        Err(StreamableHttpError::InsufficientScope(error)) => {
            assert_eq!(error.get_required_scope(), Some("mcp:admin"));
        }
        other => panic!("{other:?}"),
    }
    // One sign-in and two escalations: three authorization requests, four sends.
    assert_eq!(site.authorizations().len(), 3);
    assert_eq!(inner.calls().len(), 4);
}

#[tokio::test]
async fn a_server_that_always_says_401_is_asked_three_times_and_then_believed() {
    let site = serve();
    let inner = Scripted::new(&site, &[], Reply::Unauthorized);
    let client = client(&site, inner.clone(), Identity::Discovered).await;

    let refused = post(&client).await;

    assert!(
        matches!(refused, Err(StreamableHttpError::AuthRequired(_))),
        "{refused:?}"
    );
    assert_eq!(site.authorizations().len(), 3);
    assert_eq!(inner.calls().len(), 4);
}

#[tokio::test]
async fn a_client_credentials_client_refused_for_scope_is_not_sent_to_a_browser() {
    let site = serve();
    let script = [Reply::Unauthorized, Reply::Forbidden(Some("mcp:write"))];
    let inner = Scripted::new(&site, &script, Reply::Accept);
    let identity = Identity::ClientCredentials {
        client_id: "machine".to_owned(),
        client_secret: "machine-secret".to_owned(),
    };
    let client = client(&site, inner.clone(), identity).await;

    let refused = post(&client).await;

    assert!(
        matches!(refused, Err(StreamableHttpError::InsufficientScope(_))),
        "{refused:?}"
    );
    assert_eq!(site.authorizations().len(), 0);
    assert_eq!(inner.calls().len(), 2);
}

#[tokio::test]
async fn any_other_failure_passes_through_untouched() {
    let site = serve();
    let inner = Scripted::new(&site, &[Reply::Closed], Reply::Accept);
    let client = client(&site, inner.clone(), Identity::Discovered).await;

    let failed = post(&client).await;

    assert!(
        matches!(failed, Err(StreamableHttpError::TransportChannelClosed)),
        "{failed:?}"
    );
    assert_eq!(inner.calls().len(), 1);
    assert_eq!(site.authorizations().len(), 0);
}

#[tokio::test]
async fn a_failed_authorization_is_reported_as_what_failed() {
    let site = serve();
    let inner = Scripted::new(&site, &[], Reply::Unauthorized);
    let client = client(&site, inner.clone(), Identity::Discovered).await;

    // The authorization endpoint shows a page: the consent step's own words.
    site.answer(Answer::Page);
    match post(&client).await {
        Err(StreamableHttpError::Auth(AuthError::AuthorizationFailed(message))) => {
            assert_eq!(
                message,
                "the authorization endpoint answered 200, not a redirect"
            );
        }
        other => panic!("{other:?}"),
    }
    // The token endpoint is never offered a forged callback: rmcp's own error.
    site.answer(Answer::ForgedState);
    let forged = post(&client).await;
    assert!(
        matches!(&forged, Err(StreamableHttpError::Auth(error))
            if !matches!(error, AuthError::AuthorizationFailed(text) if text.contains("redirect"))),
        "{forged:?}"
    );
    // Each failure ended its request after one send.
    assert_eq!(inner.calls().len(), 2);
}

#[tokio::test]
async fn every_method_reaches_its_own_counterpart_with_the_token_and_its_arguments() {
    let site = serve();
    let inner = Scripted::new(&site, &[Reply::Unauthorized], Reply::Accept);
    let client = client(&site, inner.clone(), Identity::Discovered).await;
    post(&client).await.unwrap();
    let uri: Arc<str> = "http://server/mcp".into();
    let session: Arc<str> = "session-1".into();
    let stale = || Some("callers-token".to_owned());

    client
        .post_message_with_max_sse_event_size(
            uri.clone(),
            message(),
            Some(session.clone()),
            stale(),
            headers(),
            11,
        )
        .await
        .unwrap();
    client
        .delete_session(uri.clone(), session.clone(), stale(), headers())
        .await
        .unwrap();
    let expected = |method, last_event_id: Option<&str>, limit, session: Option<&str>| Call {
        method,
        uri: "http://server/mcp".to_owned(),
        session: session.map(str::to_owned),
        last_event_id: last_event_id.map(str::to_owned),
        token: Some("token-1".to_owned()),
        headers: 1,
        limit,
    };
    assert_eq!(
        inner.calls()[2..],
        [
            expected("post-limited", None, Some(11), Some("session-1")),
            expected("delete", None, None, Some("session-1")),
        ]
    );
    // The plain post, from the sign-in above, carried its session and headers.
    assert_eq!(inner.calls()[1].method, "post");
    assert_eq!(inner.calls()[1].session.as_deref(), Some("s"));
    assert_eq!(inner.calls()[1].headers, 1);
}

#[tokio::test]
async fn both_stream_methods_reach_their_own_counterparts_with_the_token() {
    let site = serve();
    let inner = Scripted::new(&site, &[Reply::Unauthorized], Reply::Accept);
    let client = client(&site, inner.clone(), Identity::Discovered).await;
    post(&client).await.unwrap();
    let uri: Arc<str> = "http://server/mcp".into();
    let session: Arc<str> = "session-1".into();
    let stale = || Some("callers-token".to_owned());

    let mut stream = client
        .get_stream(
            uri.clone(),
            Some(session.clone()),
            Some("e1".to_owned()),
            stale(),
            headers(),
        )
        .await
        .unwrap();
    assert_eq!(
        stream.next().await.unwrap().unwrap().data.as_deref(),
        Some("one")
    );
    let mut limited = client
        .get_stream_with_max_sse_event_size(uri.clone(), None, None, stale(), headers(), 22)
        .await
        .unwrap();
    assert!(limited.next().await.is_some());

    let calls = inner.calls();
    assert_eq!(calls[2].method, "get");
    assert_eq!(calls[2].uri, "http://server/mcp");
    assert_eq!(calls[2].session.as_deref(), Some("session-1"));
    assert_eq!(calls[2].last_event_id.as_deref(), Some("e1"));
    assert_eq!(calls[2].limit, None);
    assert_eq!(calls[3].method, "get-limited");
    assert_eq!(calls[3].session, None);
    assert_eq!(calls[3].last_event_id, None);
    assert_eq!(calls[3].limit, Some(22));
    for call in &calls[2..] {
        assert_eq!(call.token.as_deref(), Some("token-1"));
        assert_eq!(call.headers, 1);
    }
}

#[tokio::test]
async fn every_method_authorizes_on_a_401() {
    let site = serve();
    let uri: Arc<str> = "http://server/mcp".into();
    let session: Arc<str> = "session-1".into();
    for method in ["post-limited", "delete", "get", "get-limited"] {
        let inner = Scripted::new(&site, &[Reply::Unauthorized], Reply::Accept);
        let client = client(&site, inner.clone(), Identity::Discovered).await;
        let (uri, session) = (uri.clone(), session.clone());
        match method {
            "post-limited" => drop(
                client
                    .post_message_with_max_sse_event_size(uri, message(), None, None, headers(), 1)
                    .await
                    .unwrap(),
            ),
            "delete" => client
                .delete_session(uri, session, None, headers())
                .await
                .unwrap(),
            "get" => drop(
                client
                    .get_stream(uri, None, None, None, headers())
                    .await
                    .unwrap(),
            ),
            _ => drop(
                client
                    .get_stream_with_max_sse_event_size(uri, None, None, None, headers(), 1)
                    .await
                    .unwrap(),
            ),
        }
        let calls = inner.calls();
        assert_eq!(calls.len(), 2, "{method}");
        assert_eq!(calls[0].method, method);
        assert_eq!(calls[0].token, None, "{method}");
        assert_eq!(calls[1].method, method);
        assert!(calls[1].token.is_some(), "{method}");
    }
    assert_eq!(site.authorizations().len(), 4);
}

#[tokio::test]
async fn a_clone_shares_the_authorization() {
    let site = serve();
    let inner = Scripted::new(&site, &[Reply::Unauthorized], Reply::Accept);
    let client = client(&site, inner.clone(), Identity::Discovered).await;
    let twin = client.clone();

    post(&client).await.unwrap();
    post(&twin).await.unwrap();

    assert_eq!(inner.tokens()[2].as_deref(), Some("token-1"));
    assert_eq!(site.authorizations().len(), 1);
    assert!(format!("{twin:?}").starts_with("Authorizing"));
}
