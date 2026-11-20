// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The suite's per-scenario context: which client the host is to be.
//!
//! The runner sets `MCP_CONFORMANCE_CONTEXT` to a JSON object for the scenarios
//! that hand the client something it could not discover — a client the
//! authorization server already knows, with its secret. The value holds
//! secrets, so it is parsed into [`Identity`] and never echoed.

use serde_json::Value;

/// Who the host presents itself as to the authorization server.
#[derive(Clone, PartialEq, Eq)]
pub enum Identity {
    /// Nothing was handed over: present the Client ID Metadata Document URL
    /// where the server accepts one, otherwise register dynamically.
    Discovered,
    /// A client registered out of band, for the authorization-code flow.
    Preregistered {
        /// The `client_id` the authorization server already knows.
        client_id: String,
        /// Its secret, when it is a confidential client.
        client_secret: Option<String>,
    },
    /// A client that authenticates as itself, with no user: the
    /// `client_credentials` grant (SEP-1046).
    ClientCredentials {
        /// The `client_id` the authorization server already knows.
        client_id: String,
        /// Its secret.
        client_secret: String,
    },
}

impl std::fmt::Debug for Identity {
    /// The variant and nothing else: a secret must not reach a log by way of
    /// `{:?}`, and the `client_id` beside it is not worth an exception.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Discovered => "Discovered",
            Self::Preregistered { .. } => "Preregistered(<redacted>)",
            Self::ClientCredentials { .. } => "ClientCredentials(<redacted>)",
        })
    }
}

/// The scenario whose handed-over client uses the `client_credentials` grant.
const CLIENT_CREDENTIALS_SCENARIO: &str = "auth/client-credentials-basic";

/// Why a context could not be used. Carries no part of the context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContextError {
    /// The value is not a JSON object.
    NotAnObject,
    /// The `client_credentials` scenario came without a `client_secret`.
    MissingSecret,
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NotAnObject => "MCP_CONFORMANCE_CONTEXT is not a JSON object",
            Self::MissingSecret => {
                "MCP_CONFORMANCE_CONTEXT names a client_credentials client without a client_secret"
            }
        })
    }
}

impl std::error::Error for ContextError {}

/// The identity `context` (the raw `MCP_CONFORMANCE_CONTEXT`, if set) hands
/// over.
///
/// No context, or one without a `client_id`, is [`Identity::Discovered`]: most
/// scenarios leave registration to the client. The grant follows the
/// context's own `name`, the scenario the runner says it is running.
///
/// # Errors
///
/// The value is not a JSON object, or it names the `client_credentials`
/// scenario without a secret to authenticate with.
pub fn identity(context: Option<&str>) -> Result<Identity, ContextError> {
    let Some(context) = context else {
        return Ok(Identity::Discovered);
    };
    let value: Value = serde_json::from_str(context).map_err(|_| ContextError::NotAnObject)?;
    let Some(object) = value.as_object() else {
        return Err(ContextError::NotAnObject);
    };
    let text = |key: &str| object.get(key).and_then(Value::as_str).map(str::to_owned);
    let Some(client_id) = text("client_id") else {
        return Ok(Identity::Discovered);
    };
    let client_secret = text("client_secret");
    if text("name").as_deref() == Some(CLIENT_CREDENTIALS_SCENARIO) {
        let client_secret = client_secret.ok_or(ContextError::MissingSecret)?;
        return Ok(Identity::ClientCredentials {
            client_id,
            client_secret,
        });
    }
    Ok(Identity::Preregistered {
        client_id,
        client_secret,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn no_context_or_one_without_a_client_leaves_registration_to_the_host() {
        assert_eq!(identity(None), Ok(Identity::Discovered));
        assert_eq!(identity(Some("{}")), Ok(Identity::Discovered));
        let named = r#"{"name":"auth/metadata-default"}"#;
        assert_eq!(identity(Some(named)), Ok(Identity::Discovered));
        // A secret with no client to go with it identifies nobody.
        let orphan = r#"{"client_secret":"s"}"#;
        assert_eq!(identity(Some(orphan)), Ok(Identity::Discovered));
        // A `client_id` that is not a string is not a `client_id`.
        assert_eq!(
            identity(Some(r#"{"client_id":7}"#)),
            Ok(Identity::Discovered)
        );
    }

    #[test]
    fn a_handed_over_client_is_preregistered_with_or_without_a_secret() {
        let context = r#"{"name":"auth/pre-registration","client_id":"c","client_secret":"s"}"#;
        assert_eq!(
            identity(Some(context)),
            Ok(Identity::Preregistered {
                client_id: "c".to_owned(),
                client_secret: Some("s".to_owned()),
            })
        );
        assert_eq!(
            identity(Some(r#"{"client_id":"c"}"#)),
            Ok(Identity::Preregistered {
                client_id: "c".to_owned(),
                client_secret: None,
            })
        );
    }

    #[test]
    fn the_client_credentials_scenario_selects_that_grant_and_needs_its_secret() {
        let context =
            r#"{"name":"auth/client-credentials-basic","client_id":"c","client_secret":"s"}"#;
        assert_eq!(
            identity(Some(context)),
            Ok(Identity::ClientCredentials {
                client_id: "c".to_owned(),
                client_secret: "s".to_owned(),
            })
        );
        let bare = r#"{"name":"auth/client-credentials-basic","client_id":"c"}"#;
        assert_eq!(identity(Some(bare)), Err(ContextError::MissingSecret));
        // Without a client the scenario name alone selects nothing.
        let nameless = r#"{"name":"auth/client-credentials-basic"}"#;
        assert_eq!(identity(Some(nameless)), Ok(Identity::Discovered));
    }

    #[test]
    fn anything_but_an_object_is_refused() {
        for context in ["", "not json", "[]", "\"client_id\"", "7", "null"] {
            assert_eq!(
                identity(Some(context)),
                Err(ContextError::NotAnObject),
                "{context}"
            );
        }
    }

    #[test]
    fn neither_the_identity_nor_the_error_repeats_a_secret() {
        let secret = "hunter2-secret";
        let preregistered = Identity::Preregistered {
            client_id: "the-client".to_owned(),
            client_secret: Some(secret.to_owned()),
        };
        let machine = Identity::ClientCredentials {
            client_id: "the-client".to_owned(),
            client_secret: secret.to_owned(),
        };
        assert_eq!(format!("{:?}", Identity::Discovered), "Discovered");
        assert_eq!(format!("{preregistered:?}"), "Preregistered(<redacted>)");
        assert_eq!(format!("{machine:?}"), "ClientCredentials(<redacted>)");
        assert_eq!(
            ContextError::NotAnObject.to_string(),
            "MCP_CONFORMANCE_CONTEXT is not a JSON object"
        );
        assert_eq!(
            ContextError::MissingSecret.to_string(),
            "MCP_CONFORMANCE_CONTEXT names a client_credentials client without a client_secret"
        );
    }
}
