// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the envelope gate.
//!
//! [`StatelessEnvelope::fault`] is exercised directly, on requests built by
//! deserializing wire JSON — the same path a real message takes — so what is
//! pinned is the rule and not rmcp's plumbing. That the plumbing delivers the
//! `_meta` at all is pinned separately, on the wire, by
//! `tests/stateless_stdio.rs`.

use rmcp::model::{ClientCapabilities, ErrorCode, Implementation};

use super::*;
use crate::server::{EverythingServer, ServedRevision};

/// A gate over a server serving the stateless revision.
fn gate() -> StatelessEnvelope<EverythingServer> {
    StatelessEnvelope::new(
        ServedRevision::V2026_07_28,
        EverythingServer::serving(ServedRevision::V2026_07_28),
    )
}

/// A client request, from the JSON a client would have sent.
fn request(method: &str) -> ClientRequest {
    serde_json::from_value(serde_json::json!({ "method": method, "params": {} }))
        .unwrap_or_else(|error| panic!("{method} is a client request: {error}"))
}

/// The removed handshake, which cannot carry an envelope.
fn initialize() -> ClientRequest {
    serde_json::from_value(serde_json::json!({
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "legacy", "version": "0.0.0" },
        },
    }))
    .expect("initialize is a client request")
}

/// A complete `2026-07-28` envelope.
fn envelope() -> RequestMetaObject {
    RequestMetaObject::with_client_context(
        ProtocolVersion::V_2026_07_28,
        Implementation::new("gate-test", "0.0.0"),
        ClientCapabilities::default(),
    )
}

/// The error code of the fault `request` draws under `meta`, if any.
fn fault_code(request: &ClientRequest, meta: &RequestMetaObject) -> Option<ErrorCode> {
    gate().fault(request, meta).map(|error| error.code)
}

#[test]
fn a_complete_envelope_passes() {
    assert_eq!(fault_code(&request("tools/list"), &envelope()), None);
}

#[test]
fn an_absent_envelope_is_invalid_params() {
    // The failure this module exists to prevent: without it, a request naming
    // no revision at all would simply be served.
    assert_eq!(
        fault_code(&request("tools/list"), &RequestMetaObject::default()),
        Some(ErrorCode::INVALID_PARAMS)
    );
}

#[test]
fn every_method_carries_the_envelope_including_discovery() {
    // `server/discover` is how a client *learns* the versions, which makes it
    // tempting to exempt — but a client must still say which revision it is
    // speaking, and rmcp's own client sends the full envelope on its probe.
    // Exempting it would also break the refusal-and-retry flow: with no
    // version to refuse, `-32022` never fires and the retry never happens.
    for method in [
        "server/discover",
        "tools/list",
        "tools/call",
        "resources/read",
        "prompts/get",
        "completion/complete",
    ] {
        assert_eq!(
            fault_code(&request(method), &RequestMetaObject::default()),
            Some(ErrorCode::INVALID_PARAMS),
            "{method} must carry the envelope"
        );
        assert_eq!(
            fault_code(&request(method), &envelope()),
            None,
            "{method} with a complete envelope must pass"
        );
    }
}

#[test]
fn each_required_field_is_required_on_its_own() {
    // Version present, capabilities absent — the case a check for "any `_meta`
    // at all" would wave through.
    let mut version_only = RequestMetaObject::new();
    version_only.set_protocol_version(ProtocolVersion::V_2026_07_28);
    assert_eq!(
        fault_code(&request("tools/list"), &version_only),
        Some(ErrorCode::INVALID_PARAMS)
    );

    // And the reverse.
    let mut capabilities_only = RequestMetaObject::new();
    capabilities_only.set_client_capabilities(ClientCapabilities::default());
    assert_eq!(
        fault_code(&request("tools/list"), &capabilities_only),
        Some(ErrorCode::INVALID_PARAMS)
    );
}

#[test]
fn the_refusal_names_the_field_that_was_missing() {
    // A client that cannot tell *which* field it omitted has to guess, and the
    // envelope has two.
    let mut version_only = RequestMetaObject::new();
    version_only.set_protocol_version(ProtocolVersion::V_2026_07_28);
    let error = gate()
        .fault(&request("tools/list"), &version_only)
        .expect("refused");
    assert!(
        error.message.contains("clientCapabilities"),
        "{}",
        error.message
    );
}

#[test]
fn a_version_this_server_does_not_serve_is_refused_by_code() {
    // Not `-32602`: the envelope is well-formed, and the client needs to learn
    // which versions exist rather than that it wrote the field wrong.
    let mut legacy = RequestMetaObject::new();
    legacy.set_protocol_version(ProtocolVersion::V_2025_11_25);
    legacy.set_client_capabilities(ClientCapabilities::default());
    assert_eq!(
        fault_code(&request("tools/list"), &legacy),
        Some(ErrorCode::UNSUPPORTED_PROTOCOL_VERSION)
    );
}

#[test]
fn the_version_refusal_names_the_versions_this_server_does_serve() {
    // VERS-001's substance: the list is what lets a client retry rather than
    // give up, and rmcp's own client drives its retry off exactly this field.
    let mut legacy = RequestMetaObject::new();
    legacy.set_protocol_version(ProtocolVersion::V_2025_11_25);
    legacy.set_client_capabilities(ClientCapabilities::default());
    let error = gate()
        .fault(&request("tools/list"), &legacy)
        .expect("refused");
    assert_eq!(
        error
            .data
            .and_then(|data| data.get("supported").cloned())
            .expect("data.supported"),
        serde_json::json!(["2026-07-28"])
    );
}

#[test]
fn the_removed_handshake_passes_through_to_the_handler() {
    // `initialize` cannot carry the envelope, and `basic/versioning` declines
    // to say which error it draws. The handler answers it with the version
    // refusal; the gate must not pre-empt that with a `-32602` about a field
    // the request was never able to send.
    assert_eq!(
        fault_code(&initialize(), &RequestMetaObject::default()),
        None
    );
}

/// A service that records whether the wrapper delegated to it.
///
/// The wrapper's job is "reject a bad envelope, pass everything else
/// through", and *pass everything else through* is only observable from the
/// far side. `EverythingServer` cannot observe it — it acts on no client
/// notification and reads its own `get_info`, not the wrapper's — so the
/// observer is a stub whose whole purpose is to notice.
#[derive(Debug, Default)]
struct Recorder {
    notified: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl rmcp::Service<RoleServer> for Recorder {
    async fn handle_request(
        &self,
        _request: ClientRequest,
        _context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ServerResult, McpError> {
        Err(McpError::internal_error("unused", None))
    }

    async fn handle_notification(
        &self,
        _notification: rmcp::model::ClientNotification,
        _context: rmcp::service::NotificationContext<RoleServer>,
    ) -> Result<(), McpError> {
        self.notified
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo::new(rmcp::model::ServerCapabilities::default())
            .with_server_info(Implementation::new("recorder", "9.9.9"))
    }

    fn supported_protocol_versions(&self) -> std::borrow::Cow<'static, [ProtocolVersion]> {
        std::borrow::Cow::Owned(vec![ProtocolVersion::V_2025_06_18])
    }
}

#[test]
fn the_wrapper_reports_the_inner_service_rather_than_itself() {
    // Both are trait methods rmcp reads to decide what this service is. A
    // wrapper answering for itself would advertise a server nobody wrote — and
    // `fault` refuses versions against exactly this list, so a wrong answer
    // here is a wrong refusal on the wire.
    let wrapped = StatelessEnvelope::new(ServedRevision::V2026_07_28, Recorder::default());
    assert_eq!(
        rmcp::Service::get_info(&wrapped).server_info.name,
        "recorder"
    );
    assert_eq!(
        rmcp::Service::supported_protocol_versions(&wrapped).as_ref(),
        [ProtocolVersion::V_2025_06_18],
        "the list is the inner service's, verbatim"
    );
}

#[tokio::test]
async fn a_notification_reaches_the_inner_service() {
    // Notifications are exempt from the envelope rules, which means they must
    // arrive *unchanged* rather than be quietly dropped: BASE-030 binds
    // requests, and a wrapper that swallowed notifications would silently
    // break cancellation and every list-changed signal.
    let notified = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (server_io, mut client_io) = tokio::io::duplex(4096);
    let service = rmcp::service::serve_directly(
        StatelessEnvelope::new(
            ServedRevision::V2026_07_28,
            Recorder {
                notified: std::sync::Arc::clone(&notified),
            },
        ),
        server_io,
        None,
    );

    tokio::io::AsyncWriteExt::write_all(
        &mut client_io,
        b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{\"requestId\":1}}\n",
    )
    .await
    .expect("write the notification");
    // Closing the whole duplex is what makes this deterministic: the bytes are
    // already buffered, so the serve loop reads them and *then* sees EOF, and
    // the flag is read after the loop has finished rather than on a timer. The
    // timeout is a guard against a hang becoming a wedged CI job, not a wait.
    drop(client_io);
    tokio::time::timeout(std::time::Duration::from_secs(10), service.waiting())
        .await
        .expect("the serve loop ends when the transport closes")
        .ok();

    assert!(
        notified.load(std::sync::atomic::Ordering::Acquire),
        "the wrapper must pass notifications through"
    );
}
