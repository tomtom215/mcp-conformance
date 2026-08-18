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
//! **The last two rules are shared, not duplicated.** rmcp's HTTP layer covers
//! the envelope's required *keys* on that transport, but nothing anywhere
//! covered the log level or the cursor — a level rmcp cannot decode reads as
//! "no level asked", and a cursor no handler inspects is a cursor nobody
//! rejects — so both were live failures until a probe session asked for them.
//! They live in the private `rules` module, over the request's JSON, and the
//! Streamable HTTP layer calls the same function: two transports, one
//! implementation, no way for them to disagree. At `2025-11-25` every rule
//! here is skipped.

use std::borrow::Cow;

use rmcp::model::{
    ClientNotification, ClientRequest, ErrorData as McpError, ProtocolVersion, RequestMetaObject,
    ServerInfo, ServerResult,
};
use rmcp::service::{NotificationContext, RequestContext};
use rmcp::{RoleServer, Service};

use super::ServedRevision;

pub(crate) mod rules;

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
        // The last two rules read the request's JSON, because the HTTP layer
        // that also enforces them holds bytes rather than a decoded request
        // and one implementation is the only way the two transports can be
        // relied on to answer the same adversarial request the same way. One
        // `to_value` per request is what that costs here.
        rules::fault(&serde_json::to_value(request).ok()?)
    }
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
