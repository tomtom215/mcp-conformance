// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The per-request `_meta` envelope, enforced for transports that do not
//! enforce it themselves.
//!
//! rmcp validates the envelope inside its Streamable HTTP tower layer, reading
//! HTTP headers alongside the body. stdio has no headers and no such layer: a
//! server there is handed a decoded `ClientRequest` and a
//! [`RequestContext`], and whatever it does not check, nothing does. So
//! `2026-07-28` over stdio needs this — without it the server would answer
//! requests that name no revision at all, which is precisely the state SEP-2575
//! replaced sessions to avoid.
//!
//! [`StatelessEnvelope`] wraps the handler rather than living inside it, for
//! the same reason rmcp's `NegotiatingStatelessHttpService` does: the rules are
//! about the *envelope* every request carries, not about any one method, and a
//! check repeated in nine handlers is a check that will be missing from the
//! tenth.
//!
//! It enforces these clauses and deliberately no more:
//!
//! | Clause | Condition | Answer |
//! |---|---|---|
//! | BASE-031 | `_meta` lacks `protocolVersion` or `clientCapabilities` | `-32602` |
//! | VERS-001 / TRAN-074 | the named version is not one this server serves | `-32022`, `data.supported` |
//! | LOG-010 | `_meta` names a log level outside RFC 5424's eight | `-32602` |
//! | PAGE-011 | a list request presents a cursor this server never issued | `-32602` |
//! | BASE-035 | a needed capability was not declared | `-32021` — raised by the handler, which knows what it needs |
//!
//! `initialize` passes straight through. It predates the envelope and cannot
//! carry one, and `basic/versioning` explicitly declines to say which error a
//! modern server owes it ("the exact code is implementation-defined"); the
//! handler answers it with the version refusal that tells a legacy client
//! something useful.
//!
//! **It wraps both transports, and the revision decides whether it enforces.**
//! rmcp's HTTP layer covers the envelope's required *keys*, and nothing covers
//! the last two rules on either transport — a level it cannot decode reads as
//! "no level asked", and a cursor nobody looks at is a cursor nobody rejects.
//! Both were live `MUST` failures until a probe session asked for them. At
//! `2025-11-25` every rule here is skipped: the envelope does not exist at that
//! revision and `resources/subscribe`-era cursors are a different question.

use std::borrow::Cow;

use rmcp::model::{
    ClientNotification, ClientRequest, ErrorData as McpError, LoggingLevel, ProtocolVersion,
    RequestMetaObject, ServerInfo, ServerResult,
};
use rmcp::service::{NotificationContext, RequestContext};
use rmcp::{RoleServer, Service};

use super::ServedRevision;

/// The `_meta` key a request asks for log messages with.
const LOG_LEVEL: &str = "io.modelcontextprotocol/logLevel";

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;

/// A service that admits only requests carrying a well-formed `2026-07-28`
/// envelope, wrapping one that assumes the envelope was already checked.
#[derive(Debug, Clone)]
pub struct StatelessEnvelope<S> {
    revision: ServedRevision,
    inner: S,
}

impl<S: Service<RoleServer>> StatelessEnvelope<S> {
    /// Wraps `inner`, enforcing only where `revision` defines the envelope.
    pub const fn new(revision: ServedRevision, inner: S) -> Self {
        Self { revision, inner }
    }

    /// The envelope fault in `meta`, if any.
    ///
    /// Takes the `_meta` rather than the whole [`RequestContext`] so the rules
    /// are testable without a live peer — and because reading anything else
    /// off the context would be a mistake: `RequestContext::protocol_version`
    /// falls back to session state, and a fallback is exactly what must not
    /// happen here. A request that named no revision would silently inherit
    /// one and be served.
    fn fault(&self, request: &ClientRequest, meta: &RequestMetaObject) -> Option<McpError> {
        if !self.revision.is_stateless() {
            return None;
        }
        if matches!(request, ClientRequest::InitializeRequest(_)) {
            return None;
        }
        // rmcp's own list of what this revision's schema marks required, so
        // the two cannot drift apart.
        for key in RequestMetaObject::DRAFT_REQUIRED_KEYS {
            if !meta.0.contains_key(key) {
                return Some(missing(key));
            }
        }
        // Present but unreadable is missing for this purpose: either way the
        // server cannot process the request standalone, which is what the
        // clause is about.
        let (Some(version), Some(_)) = (meta.protocol_version(), meta.client_capabilities()) else {
            return Some(missing("io.modelcontextprotocol/protocolVersion"));
        };
        // This service's own answer, not the inner one's. They are the same
        // list — the impl below delegates — but going through the seam means
        // the version this gate refuses against is the version this service
        // *advertises*, with one place for the two to be defined and none for
        // them to disagree.
        let supported = self.supported_protocol_versions();
        if !supported.contains(&version) {
            return Some(McpError::unsupported_protocol_version(version, &supported));
        }
        if let Some(fault) = log_level_fault(meta) {
            return Some(fault);
        }
        cursor_fault(request)
    }
}

/// LOG-010: a level outside RFC 5424's eight draws `-32602`.
///
/// Read from the raw `_meta` value rather than through rmcp's
/// `RequestMetaObject::log_level`, which decodes into `LoggingLevel` and
/// answers `None` for *both* "absent" and "not a level". Those are opposite
/// cases: the first asks for no logs, the second is the malformed request this
/// clause exists to reject, and a server that cannot tell them apart silently
/// serves the second.
fn log_level_fault(meta: &RequestMetaObject) -> Option<McpError> {
    let asked = meta.0.get(LOG_LEVEL)?;
    if serde_json::from_value::<LoggingLevel>(asked.clone()).is_ok() {
        return None;
    }
    Some(McpError::invalid_params(
        format!("`{LOG_LEVEL}` is {asked}, which is not one of the eight RFC 5424 levels"),
        Some(serde_json::json!({ "field": LOG_LEVEL })),
    ))
}

/// PAGE-011: a cursor this server never issued draws `-32602`.
///
/// This server issues none: every catalogue it serves fits in one page, so no
/// result of its own has ever carried a `nextCursor`. That makes *any* cursor
/// presented to it one it did not issue — fabricated, modified, or carried
/// over from another server — which is exactly what the clause forbids
/// honouring. A server that paginated would compare against what it had
/// issued; this one can answer from the stronger fact that it issued nothing.
fn cursor_fault(request: &ClientRequest) -> Option<McpError> {
    let cursor = match request {
        ClientRequest::ListToolsRequest(request) => request.params.as_ref()?.cursor.as_ref(),
        ClientRequest::ListPromptsRequest(request) => request.params.as_ref()?.cursor.as_ref(),
        ClientRequest::ListResourcesRequest(request) => request.params.as_ref()?.cursor.as_ref(),
        ClientRequest::ListResourceTemplatesRequest(request) => {
            request.params.as_ref()?.cursor.as_ref()
        }
        _ => None,
    }?;
    Some(McpError::invalid_params(
        format!("cursor {cursor:?} was not issued by this server, which paginates nothing"),
        Some(serde_json::json!({ "cursor": cursor })),
    ))
}

/// The `-32602` a malformed envelope draws (BASE-031).
fn missing(field: &str) -> McpError {
    McpError::invalid_params(
        format!("request `_meta` is missing required field `{field}`"),
        Some(serde_json::json!({ "field": field })),
    )
}

impl<S: Service<RoleServer>> Service<RoleServer> for StatelessEnvelope<S> {
    async fn handle_request(
        &self,
        request: ClientRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<ServerResult, McpError> {
        match self.fault(&request, &context.meta) {
            Some(error) => Err(error),
            None => self.inner.handle_request(request, context).await,
        }
    }

    async fn handle_notification(
        &self,
        notification: ClientNotification,
        context: NotificationContext<RoleServer>,
    ) -> Result<(), McpError> {
        // Notifications are exempt, and the exemption is the specification's:
        // BASE-030 binds *requests*, which the stateless model defines as the
        // messages a server must be able to process standalone. A notification
        // draws no answer, so there is nothing to reject it with either.
        self.inner.handle_notification(notification, context).await
    }

    fn get_info(&self) -> ServerInfo {
        self.inner.get_info()
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        self.inner.supported_protocol_versions()
    }
}
