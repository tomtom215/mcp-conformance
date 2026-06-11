// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Streamable HTTP serving behind the default-secure policy.
//!
//! Two enforcement layers, deliberately redundant: the axum middleware here
//! rejects bad `Host`/`Origin` with 403 before any MCP processing
//! ([`crate::policy::HttpSecurityPolicy`], fail-closed, mutation-tested),
//! and rmcp's own transport-level `allowed_hosts` check — kept in sync from
//! the same policy — backstops it. The `dns-rebinding-protection` scenario
//! and TRAN-002/007/008's tests exercise the outer layer.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;

use crate::policy::HttpSecurityPolicy;
use crate::server::EverythingServer;

/// Path the MCP endpoint is served under, matching the ecosystem default.
pub const MCP_PATH: &str = "/mcp";

/// Builds the complete HTTP application: policy middleware wrapping the
/// streamable HTTP MCP service at [`MCP_PATH`].
pub fn router(policy: HttpSecurityPolicy) -> Router {
    mcp_router(&policy).layer(middleware::from_fn_with_state(
        Arc::new(policy),
        enforce_policy,
    ))
}

/// [`router`], with the session trace tap installed inside the policy layer:
/// only policy-admitted traffic can form sessions, so only sessions are
/// recorded (the tap module documents this boundary).
#[cfg(feature = "tap")]
pub fn router_tapped(policy: HttpSecurityPolicy, tap: Arc<crate::tap::Tap>) -> Router {
    mcp_router(&policy)
        .layer(middleware::from_fn_with_state(tap, crate::tap::tap_layer))
        .layer(middleware::from_fn_with_state(
            Arc::new(policy),
            enforce_policy,
        ))
}

/// The MCP service mounted at [`MCP_PATH`], with rmcp's transport-level host
/// check mirrored from `policy`.
fn mcp_router(policy: &HttpSecurityPolicy) -> Router {
    // Mirror the policy into rmcp's transport-level host check so the two
    // layers cannot disagree: an empty list is rmcp's "allow all", matching
    // the policy's explicit dangerous opt-out. (Field assignment because the
    // config struct is non-exhaustive.)
    let mut config = StreamableHttpServerConfig::default();
    config.allowed_hosts = if policy.allows_any_host() {
        Vec::new()
    } else {
        policy.allowed_hosts().to_vec()
    };
    let service = StreamableHttpService::new(
        || Ok(EverythingServer::new()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    Router::new().nest_service(MCP_PATH, service)
}

/// The 403 gate: `Host` must be present and allowed; `Origin`, when present,
/// must be allowed. Anything else never reaches the MCP service.
async fn enforce_policy(
    State(policy): State<Arc<HttpSecurityPolicy>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if headers_allowed(&policy, request.headers()) {
        next.run(request).await
    } else {
        forbidden()
    }
}

/// Pure policy evaluation over the request headers (split out so the
/// decision logic is unit-testable without axum plumbing).
fn headers_allowed(policy: &HttpSecurityPolicy, headers: &HeaderMap) -> bool {
    // HTTP/1.1 requires Host; absence is denied (fail closed). Non-UTF-8
    // header values are denied the same way.
    let host_ok = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| policy.host_header_allowed(host));
    // Origin is absent for non-browser clients — that is acceptable; when
    // present it must pass.
    let origin_ok = headers.get(header::ORIGIN).is_none_or(|value| {
        value
            .to_str()
            .ok()
            .is_some_and(|origin| policy.origin_allowed(origin))
    });
    host_ok && origin_ok
}

/// The rejection the `2025-11-25` transports specification requires for
/// failed `Origin` validation, with a JSON-RPC-shaped body for diagnostics.
fn forbidden() -> Response {
    Response::builder()
        .status(StatusCode::FORBIDDEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32000,"message":"Forbidden: Host or Origin header rejected by security policy"}}"#,
        ))
        .unwrap_or_else(|_| StatusCode::FORBIDDEN.into_response())
}

use axum::response::IntoResponse as _;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                axum::http::HeaderName::try_from(*name).unwrap(),
                axum::http::HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    #[test]
    fn loopback_host_passes_and_rebinding_host_is_denied() {
        let policy = HttpSecurityPolicy::default();
        assert!(headers_allowed(
            &policy,
            &headers(&[("host", "localhost:8080")])
        ));
        assert!(headers_allowed(&policy, &headers(&[("host", "127.0.0.1")])));
        assert!(!headers_allowed(
            &policy,
            &headers(&[("host", "evil.example")])
        ));
    }

    #[test]
    fn missing_host_is_denied_fail_closed() {
        let policy = HttpSecurityPolicy::default();
        assert!(!headers_allowed(&policy, &HeaderMap::new()));
    }

    #[test]
    fn origin_when_present_must_pass_but_absence_is_fine() {
        let policy = HttpSecurityPolicy::default();
        assert!(headers_allowed(
            &policy,
            &headers(&[("host", "localhost"), ("origin", "http://localhost:6274")])
        ));
        assert!(!headers_allowed(
            &policy,
            &headers(&[("host", "localhost"), ("origin", "http://evil.example")])
        ));
        assert!(headers_allowed(&policy, &headers(&[("host", "localhost")])));
    }

    #[test]
    fn forbidden_response_is_403_json() {
        let response = forbidden();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }
}
