// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The OAuth flow against a loopback authorization server: what the host
//! sends at each step (PKCE `S256`, `state`, the RFC 8707 `resource`, the
//! chosen scopes, the client it presents itself as), the authorization
//! budget, and that a failure at any step leaves the flow usable.

#![cfg(feature = "oauth")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use mcp_reference_host::oauth::consent::{Consent, ConsentError, ConsentFuture, Headless};
use mcp_reference_host::oauth::context::Identity;
use mcp_reference_host::oauth::flow::{Flow, FlowError};
use mcp_reference_host::oauth::{
    AUTHORIZATION_BUDGET, CLIENT_METADATA_URL, CLIENT_NAME, REDIRECT_URI,
};
use rmcp::transport::auth::AuthError;
use support::{Answer, Site, serve};

async fn flow(site: &Site, identity: Identity) -> Flow {
    let consent = Headless::new(REDIRECT_URI).unwrap();
    Flow::new(&site.mcp(), identity, Box::new(consent))
        .await
        .unwrap()
}

fn preregistered(secret: Option<&str>) -> Identity {
    Identity::Preregistered {
        client_id: "known-client".to_owned(),
        client_secret: secret.map(str::to_owned),
    }
}

fn machine() -> Identity {
    Identity::ClientCredentials {
        client_id: "machine".to_owned(),
        client_secret: "machine-secret".to_owned(),
    }
}

#[tokio::test]
async fn a_challenge_becomes_a_token_through_pkce_state_and_resource() {
    let site = serve();
    let flow = flow(&site, Identity::Discovered).await;
    assert_eq!(flow.token().await.unwrap(), None);
    assert_eq!(flow.spent().await, 0);

    flow.authorize(&site.challenge(), None).await.unwrap();

    assert_eq!(flow.token().await.unwrap().as_deref(), Some("token-1"));
    assert_eq!(flow.spent().await, 1);
    // Registered dynamically, under the host's name and redirect URI.
    let registrations = site.registrations.lock().unwrap().clone();
    assert_eq!(registrations.len(), 1);
    assert_eq!(registrations[0]["client_name"], CLIENT_NAME);
    assert_eq!(registrations[0]["redirect_uris"][0], REDIRECT_URI);
    let asked = &site.authorizations()[0];
    assert_eq!(asked["client_id"], "registered-client");
    assert_eq!(asked["response_type"], "code");
    assert_eq!(asked["redirect_uri"], REDIRECT_URI);
    assert_eq!(asked["code_challenge_method"], "S256");
    assert!(asked["code_challenge"].len() >= 43, "{asked:?}");
    assert_ne!(asked["state"], "");
    // RFC 8707: the resource is the MCP server, as its metadata names it.
    assert_eq!(asked["resource"], site.mcp());
    // The challenge's scope wins over the resource's `scopes_supported`.
    assert_eq!(asked["scope"], "mcp:basic");
    let exchanged = &site.token_requests()[0].form;
    assert_eq!(exchanged["grant_type"], "authorization_code");
    assert_eq!(exchanged["code"], "code-1");
    assert_eq!(exchanged["redirect_uri"], REDIRECT_URI);
    assert_eq!(exchanged["resource"], site.mcp());
    assert!(exchanged["code_verifier"].len() >= 43, "{exchanged:?}");
}

#[tokio::test]
async fn without_a_challenge_scope_the_resource_scopes_are_asked_for() {
    let site = serve();
    *site.challenge_scope.lock().unwrap() = None;
    let flow = flow(&site, Identity::Discovered).await;
    flow.authorize(&site.challenge(), None).await.unwrap();
    assert_eq!(site.authorizations()[0]["scope"], "mcp:listed");
}

#[tokio::test]
async fn a_preregistered_client_is_presented_and_never_registered() {
    let site = serve();
    let flow = flow(&site, preregistered(Some("known-secret"))).await;
    flow.authorize(&site.challenge(), None).await.unwrap();

    assert_eq!(site.registrations(), 0);
    assert_eq!(site.authorizations()[0]["client_id"], "known-client");
    let exchanged = &site.token_requests()[0];
    // The secret reaches the token endpoint one way or the other.
    let in_body = exchanged.form.get("client_secret").map(String::as_str) == Some("known-secret");
    assert!(
        in_body || exchanged.authorization.is_some(),
        "{exchanged:?}"
    );
}

#[tokio::test]
async fn a_preregistered_public_client_sends_no_secret() {
    let site = serve();
    let flow = flow(&site, preregistered(None)).await;
    flow.authorize(&site.challenge(), None).await.unwrap();

    assert_eq!(site.registrations(), 0);
    let exchanged = &site.token_requests()[0];
    assert_eq!(exchanged.form["client_id"], "known-client");
    assert_eq!(exchanged.form.get("client_secret"), None);
    assert_eq!(exchanged.authorization, None);
}

#[tokio::test]
async fn client_credentials_asks_the_token_endpoint_and_nobody_else() {
    let site = serve();
    let flow = flow(&site, machine()).await;
    flow.authorize(&site.challenge(), None).await.unwrap();

    assert_eq!(flow.token().await.unwrap().as_deref(), Some("token-1"));
    assert_eq!(site.registrations(), 0);
    assert_eq!(site.authorizations().len(), 0);
    let exchanged = &site.token_requests()[0].form;
    assert_eq!(exchanged["grant_type"], "client_credentials");
    assert_eq!(exchanged["client_id"], "machine");
    assert_eq!(exchanged["client_secret"], "machine-secret");
    assert_eq!(exchanged["resource"], site.mcp());
    assert_eq!(exchanged.get("scope"), None);
}

#[tokio::test]
async fn client_credentials_cannot_escalate() {
    let site = serve();
    let flow = flow(&site, machine()).await;
    flow.authorize(&site.challenge(), None).await.unwrap();

    let refused = flow.step_up("mcp:write", Some("token-1")).await;
    assert!(matches!(refused, Err(FlowError::NoUser)), "{refused:?}");
    // Refused before anything was spent or sent.
    assert_eq!(flow.spent().await, 1);
    assert_eq!(site.token_requests().len(), 1);
}

#[tokio::test]
async fn a_token_somebody_else_obtained_is_used_and_not_paid_for_again() {
    let site = serve();
    let flow = flow(&site, Identity::Discovered).await;
    let challenge = site.challenge();
    flow.authorize(&challenge, None).await.unwrap();

    // A second request was refused while carrying no token: by the time it
    // asks, there is one.
    flow.authorize(&challenge, None).await.unwrap();
    // Likewise one that carried a token since replaced.
    flow.authorize(&challenge, Some("token-0")).await.unwrap();
    flow.step_up("mcp:write", Some("token-0")).await.unwrap();
    flow.step_up("mcp:write", None).await.unwrap();
    assert_eq!(flow.spent().await, 1);
    assert_eq!(site.authorizations().len(), 1);

    // A request refused while carrying the current token does pay.
    flow.authorize(&challenge, Some("token-1")).await.unwrap();
    assert_eq!(flow.spent().await, 2);
    assert_eq!(flow.token().await.unwrap().as_deref(), Some("token-2"));
}

#[tokio::test]
async fn a_refusal_that_carried_a_token_the_flow_never_held_still_authorizes() {
    // No token is held, so nothing can have superseded the one sent.
    let site = serve();
    let flow = flow(&site, Identity::Discovered).await;
    flow.authorize(&site.challenge(), Some("from-elsewhere"))
        .await
        .unwrap();
    assert_eq!(flow.spent().await, 1);
    assert_eq!(flow.token().await.unwrap().as_deref(), Some("token-1"));
}

#[tokio::test]
async fn an_escalation_asks_for_the_union_and_replaces_the_token() {
    let site = serve();
    let flow = flow(&site, Identity::Discovered).await;
    flow.authorize(&site.challenge(), None).await.unwrap();

    flow.step_up("mcp:write", Some("token-1")).await.unwrap();

    assert_eq!(flow.spent().await, 2);
    assert_eq!(flow.token().await.unwrap().as_deref(), Some("token-2"));
    let asked = &site.authorizations()[1];
    let scopes: Vec<&str> = asked["scope"].split(' ').collect();
    assert_eq!(scopes, ["mcp:basic", "mcp:write"]);
    assert_eq!(asked["code_challenge_method"], "S256");
    assert_eq!(asked["resource"], site.mcp());
    // Not registered a second time: the same client escalates.
    assert_eq!(site.registrations(), 1);
    assert_eq!(asked["client_id"], "registered-client");
    assert_eq!(site.token_requests()[1].form["code"], "code-2");
}

#[tokio::test]
async fn the_budget_is_three_authorization_requests_and_then_none() {
    let site = serve();
    let flow = flow(&site, Identity::Discovered).await;
    flow.authorize(&site.challenge(), None).await.unwrap();
    flow.step_up("mcp:a", Some("token-1")).await.unwrap();
    flow.step_up("mcp:b", Some("token-2")).await.unwrap();
    assert_eq!(flow.spent().await, AUTHORIZATION_BUDGET);
    assert_eq!(AUTHORIZATION_BUDGET, 3);

    let escalation = flow.step_up("mcp:c", Some("token-3")).await;
    assert!(
        matches!(escalation, Err(FlowError::BudgetSpent)),
        "{escalation:?}"
    );
    let sign_in = flow.authorize(&site.challenge(), Some("token-3")).await;
    assert!(
        matches!(sign_in, Err(FlowError::BudgetSpent)),
        "{sign_in:?}"
    );
    // Neither refusal reached the authorization server, and the token stands.
    assert_eq!(site.authorizations().len(), 3);
    assert_eq!(flow.spent().await, 3);
    assert_eq!(flow.token().await.unwrap().as_deref(), Some("token-3"));
}

#[tokio::test]
async fn a_forged_state_is_refused_and_the_flow_survives_it() {
    let site = serve();
    let flow = flow(&site, Identity::Discovered).await;
    site.answer(Answer::ForgedState);

    let forged = flow.authorize(&site.challenge(), None).await;
    assert!(matches!(forged, Err(FlowError::Auth(_))), "{forged:?}");
    // The code that came with the forged state was never offered for a token.
    assert_eq!(site.token_requests().len(), 0);
    assert_eq!(flow.token().await.unwrap(), None);

    // The attempt was paid for, and the next one works.
    site.answer(Answer::Redirect);
    flow.authorize(&site.challenge(), None).await.unwrap();
    assert_eq!(flow.spent().await, 2);
    assert_eq!(flow.token().await.unwrap().as_deref(), Some("token-1"));
}

#[tokio::test]
async fn a_forged_state_on_escalation_keeps_the_token_already_held() {
    let site = serve();
    let flow = flow(&site, Identity::Discovered).await;
    flow.authorize(&site.challenge(), None).await.unwrap();
    site.answer(Answer::ForgedState);

    let forged = flow.step_up("mcp:write", Some("token-1")).await;
    assert!(matches!(forged, Err(FlowError::Auth(_))), "{forged:?}");
    assert_eq!(site.token_requests().len(), 1);
    assert_eq!(flow.token().await.unwrap().as_deref(), Some("token-1"));
}

#[tokio::test]
async fn each_way_the_authorization_endpoint_can_fail_to_redirect_is_named() {
    let site = serve();
    for (answer, expected) in [
        (Answer::Page, ConsentError::NotARedirect(200)),
        (Answer::NoLocation, ConsentError::NoLocation),
        (Answer::Elsewhere, ConsentError::ForeignRedirect),
    ] {
        site.answer(answer);
        let flow = flow(&site, Identity::Discovered).await;
        match flow.authorize(&site.challenge(), None).await {
            Err(FlowError::Consent(error)) => assert_eq!(error, expected),
            other => panic!("{answer:?}: {other:?}"),
        }
        assert_eq!(site.token_requests().len(), 0, "{answer:?}");
        assert_eq!(flow.token().await.unwrap(), None);
    }
}

#[tokio::test]
async fn a_server_that_offers_no_registration_fails_and_the_flow_survives_it() {
    let site = serve();
    *site.registration.lock().unwrap() = false;
    let flow = flow(&site, Identity::Discovered).await;

    let failed = flow.authorize(&site.challenge(), None).await;
    assert!(
        matches!(
            failed,
            Err(FlowError::Auth(AuthError::RegistrationFailed(_)))
        ),
        "{failed:?}"
    );
    assert_eq!(site.authorizations().len(), 0);

    // The manager was handed back: the same flow authorizes once it can.
    *site.registration.lock().unwrap() = true;
    flow.authorize(&site.challenge(), None).await.unwrap();
    assert_eq!(flow.token().await.unwrap().as_deref(), Some("token-1"));
}

#[tokio::test]
async fn failed_discovery_hands_the_manager_back() {
    // The challenge names metadata on a port nothing listens on, and so does
    // every fallback: the MCP server itself is that dead port.
    let dead = "http://127.0.0.1:1/mcp";
    let consent = Headless::new(REDIRECT_URI).unwrap();
    let flow = Flow::new(dead, Identity::Discovered, Box::new(consent))
        .await
        .unwrap();
    let challenge =
        "Bearer resource_metadata=\"http://127.0.0.1:1/.well-known/oauth-protected-resource/mcp\"";

    let failed = flow.authorize(challenge, None).await;
    assert!(matches!(failed, Err(FlowError::Auth(_))), "{failed:?}");
    // Still usable: asking for the token works, and so does a second attempt.
    assert_eq!(flow.token().await.unwrap(), None);
    let again = flow.authorize(challenge, None).await;
    assert!(matches!(again, Err(FlowError::Auth(_))), "{again:?}");
    assert_eq!(flow.spent().await, 2);
}

/// A browser that goes somewhere the authorization server never sent it.
struct Scripted(Result<String, ConsentError>);

impl Consent for Scripted {
    fn approve<'a>(&'a self, _authorization_url: &'a str) -> ConsentFuture<'a> {
        Box::pin(async move { self.0.clone() })
    }
}

#[tokio::test]
async fn a_callback_without_a_code_is_refused() {
    let site = serve();
    let callback = format!("{REDIRECT_URI}?state=anything");
    let flow = Flow::new(
        &site.mcp(),
        Identity::Discovered,
        Box::new(Scripted(Ok(callback))),
    )
    .await
    .unwrap();
    let failed = flow.authorize(&site.challenge(), None).await;
    assert!(
        matches!(
            failed,
            Err(FlowError::Auth(AuthError::AuthorizationFailed(_)))
        ),
        "{failed:?}"
    );
    assert_eq!(site.token_requests().len(), 0);
}

#[tokio::test]
async fn a_server_that_accepts_metadata_documents_is_given_the_url_and_no_registration() {
    let site = serve();
    *site.cimd.lock().unwrap() = true;
    let flow = flow(&site, Identity::Discovered).await;
    flow.authorize(&site.challenge(), None).await.unwrap();
    assert_eq!(site.authorizations()[0]["client_id"], CLIENT_METADATA_URL);
    assert_eq!(site.registrations(), 0);
    assert_eq!(
        site.token_requests()[0].form["client_id"],
        CLIENT_METADATA_URL
    );
}

#[tokio::test]
async fn a_flow_does_not_show_what_it_holds() {
    let site = serve();
    let flow = flow(&site, preregistered(Some("known-secret"))).await;
    flow.authorize(&site.challenge(), None).await.unwrap();
    let shown = format!("{flow:?}");
    assert!(shown.starts_with("Flow"), "{shown}");
    assert!(shown.contains("Preregistered"), "{shown}");
    assert!(!shown.contains("known-secret"), "{shown}");
    assert!(!shown.contains("token-1"), "{shown}");
}

#[tokio::test]
async fn a_server_url_that_is_not_one_is_refused_up_front() {
    let consent = Headless::new(REDIRECT_URI).unwrap();
    let refused = Flow::new("not a url", Identity::Discovered, Box::new(consent)).await;
    assert!(refused.is_err());
}
