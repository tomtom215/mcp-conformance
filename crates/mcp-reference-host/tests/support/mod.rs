// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A loopback authorization server and protected MCP endpoint for the `oauth`
//! tests: just enough OAuth to be discovered, registered with, asked for a
//! code and asked for a token, and a log of every request so a test can say
//! what the client sent.
//!
//! One origin plays both parts (the protected resource names itself as its
//! authorization server), which is what the official suite's simplest
//! scenarios do. Tokens are `token-1`, `token-2`, … so a test can tell a
//! fresh one from the one before.

// Each test binary uses a different part of this.
#![allow(dead_code, unreachable_pub, clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::{RawQuery, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use mcp_everything_server::http::router;
use mcp_everything_server::policy::HttpSecurityPolicy;
use mcp_everything_server::server::ServedRevision;
use serde_json::{Value, json};

/// Decoded `name=value` pairs: a query string or a form body.
pub type Params = BTreeMap<String, String>;

/// How `/authorize` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// `302` to the request's `redirect_uri` with a code and its `state`.
    Redirect,
    /// The same, with a `state` the client never issued.
    ForgedState,
    /// `302` to somewhere that is not the client's redirect URI.
    Elsewhere,
    /// `200`: a consent page, as a real server would show a browser.
    Page,
    /// `302` with no `Location`.
    NoLocation,
}

/// One request to `/token`.
#[derive(Debug, Clone)]
pub struct TokenRequest {
    /// The form body.
    pub form: Params,
    /// The `Authorization` header, if the client authenticated that way.
    pub authorization: Option<String>,
}

/// What the server is told to do and what it saw.
#[derive(Debug)]
pub struct Site {
    /// `http://127.0.0.1:<port>`.
    pub base: String,
    /// How `/authorize` answers; change it mid-test at will.
    pub answer: Mutex<Answer>,
    /// The `scope` the MCP endpoint's `401` challenge names, if any.
    pub challenge_scope: Mutex<Option<String>>,
    /// Whether the metadata offers a `registration_endpoint`.
    pub registration: Mutex<bool>,
    /// Whether the metadata says Client ID Metadata Documents are accepted.
    pub cimd: Mutex<bool>,
    /// Bodies posted to `/register`.
    pub registrations: Mutex<Vec<Value>>,
    /// Queries sent to `/authorize`.
    pub authorizations: Mutex<Vec<Params>>,
    /// Requests to `/token`.
    pub token_requests: Mutex<Vec<TokenRequest>>,
    /// Access tokens issued so far.
    pub issued: Mutex<Vec<String>>,
}

impl Site {
    /// The MCP endpoint.
    pub fn mcp(&self) -> String {
        format!("{}/mcp", self.base)
    }

    /// The `WWW-Authenticate` value the MCP endpoint's `401` carries.
    pub fn challenge(&self) -> String {
        let metadata = format!("{}/.well-known/oauth-protected-resource/mcp", self.base);
        let scope = self.challenge_scope.lock().unwrap().clone();
        let scope = scope.map(|scope| format!(", scope=\"{scope}\""));
        format!(
            "Bearer resource_metadata=\"{metadata}\"{}",
            scope.unwrap_or_default()
        )
    }

    /// Sets how `/authorize` answers from now on.
    pub fn answer(&self, answer: Answer) {
        *self.answer.lock().unwrap() = answer;
    }

    /// The queries `/authorize` has received.
    pub fn authorizations(&self) -> Vec<Params> {
        self.authorizations.lock().unwrap().clone()
    }

    /// The requests `/token` has received.
    pub fn token_requests(&self) -> Vec<TokenRequest> {
        self.token_requests.lock().unwrap().clone()
    }

    /// How many clients registered.
    pub fn registrations(&self) -> usize {
        self.registrations.lock().unwrap().len()
    }
}

/// Starts the server on its own thread and runtime, so it outlives any one
/// test's executor and can serve a spawned host binary.
pub fn serve() -> Arc<Site> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let site = Arc::new(Site {
                base: format!("http://{}", listener.local_addr().expect("addr")),
                answer: Mutex::new(Answer::Redirect),
                challenge_scope: Mutex::new(Some("mcp:basic".to_owned())),
                registration: Mutex::new(true),
                cimd: Mutex::new(false),
                registrations: Mutex::default(),
                authorizations: Mutex::default(),
                token_requests: Mutex::default(),
                issued: Mutex::default(),
            });
            sender.send(Arc::clone(&site)).expect("send site");
            let _ = axum::serve(listener, app(site)).await;
        });
    });
    receiver.recv().expect("server starts")
}

fn app(site: Arc<Site>) -> Router {
    let protected = router(HttpSecurityPolicy::default(), ServedRevision::V2025_11_25)
        .layer(middleware::from_fn_with_state(Arc::clone(&site), guard));
    Router::new()
        .route("/.well-known/oauth-protected-resource/mcp", get(resource))
        .route("/.well-known/oauth-authorization-server", get(metadata))
        .route("/register", post(register))
        .route("/authorize", get(authorize))
        .route("/token", post(token))
        .with_state(site)
        .merge(protected)
}

fn json_response(status: StatusCode, body: &Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Decodes `name=value&…`, percent-encoding and `+` included.
pub fn decode(pairs: &str) -> Params {
    let url = reqwest::Url::parse(&format!("http://decode.invalid/?{pairs}")).unwrap();
    url.query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

/// The MCP endpoint's bearer check: a token this server issued, or `401`.
async fn guard(State(site): State<Arc<Site>>, request: Request, next: Next) -> Response {
    let bearer = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_owned);
    let known = bearer.is_some_and(|token| site.issued.lock().unwrap().contains(&token));
    if known {
        return next.run(request).await;
    }
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::WWW_AUTHENTICATE, site.challenge())
        .body(Body::empty())
        .unwrap()
}

async fn resource(State(site): State<Arc<Site>>) -> Response {
    let body = json!({
        "resource": site.mcp(),
        "authorization_servers": [site.base],
        "scopes_supported": ["mcp:listed"],
    });
    json_response(StatusCode::OK, &body)
}

async fn metadata(State(site): State<Arc<Site>>) -> Response {
    let base = &site.base;
    let mut body = json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "client_credentials"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["client_secret_post", "none"],
    });
    if *site.registration.lock().unwrap() {
        body["registration_endpoint"] = json!(format!("{base}/register"));
    }
    if *site.cimd.lock().unwrap() {
        body["client_id_metadata_document_supported"] = json!(true);
    }
    json_response(StatusCode::OK, &body)
}

async fn register(State(site): State<Arc<Site>>, body: String) -> Response {
    let request: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let reply = json!({
        "client_id": "registered-client",
        "redirect_uris": request["redirect_uris"],
        "token_endpoint_auth_method": "none",
    });
    site.registrations.lock().unwrap().push(request);
    json_response(StatusCode::CREATED, &reply)
}

async fn authorize(State(site): State<Arc<Site>>, RawQuery(query): RawQuery) -> Response {
    let params = decode(&query.unwrap_or_default());
    let count = {
        let mut log = site.authorizations.lock().unwrap();
        log.push(params.clone());
        log.len()
    };
    let field = |name: &str| params.get(name).cloned().unwrap_or_default();
    let answer = *site.answer.lock().unwrap();
    let location = match answer {
        Answer::Page => return json_response(StatusCode::OK, &json!({"consent": "pending"})),
        Answer::NoLocation => None,
        Answer::Redirect => Some((field("redirect_uri"), field("state"))),
        Answer::ForgedState => Some((field("redirect_uri"), "forged".to_owned())),
        Answer::Elsewhere => Some((format!("{}/elsewhere", site.base), field("state"))),
    };
    let mut response = Response::builder().status(StatusCode::FOUND);
    if let Some((target, state)) = location {
        let location = format!("{target}?code=code-{count}&state={state}");
        response = response.header(header::LOCATION, location);
    }
    response.body(Body::empty()).unwrap()
}

async fn token(State(site): State<Arc<Site>>, headers: HeaderMap, body: String) -> Response {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let form = decode(&body);
    site.token_requests.lock().unwrap().push(TokenRequest {
        form,
        authorization,
    });
    let access_token = {
        let mut issued = site.issued.lock().unwrap();
        let token = format!("token-{}", issued.len() + 1);
        issued.push(token.clone());
        token
    };
    let body = json!({"access_token": access_token, "token_type": "Bearer", "expires_in": 3600});
    json_response(StatusCode::OK, &body)
}
