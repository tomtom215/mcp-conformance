// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! One authorization, from a challenge to a stored token, under a budget.
//!
//! [`Flow`] owns rmcp's [`AuthorizationManager`] and is the only thing that
//! calls it. Two requests can be refused at once (the transport posts while
//! its event stream is opening), so everything here runs under one lock, and
//! a caller says which token its refused request carried: if the token has
//! changed by the time the caller holds the lock, somebody else already
//! authorized and the caller only has to send again.

use rmcp::transport::auth::{
    AuthError, AuthorizationCallback, AuthorizationManager, AuthorizationRequest,
    AuthorizationSession, ClientCredentialsConfig,
};
use tokio::sync::Mutex;

use super::consent::{Consent, ConsentError};
use super::context::Identity;
use super::{AUTHORIZATION_BUDGET, CLIENT_METADATA_URL, CLIENT_NAME, REDIRECT_URI};

/// Why an authorization did not end with a token.
#[derive(Debug)]
#[non_exhaustive]
pub enum FlowError {
    /// The session has made every authorization request it is allowed.
    BudgetSpent,
    /// The identity has no user to ask again: a `client_credentials` client
    /// cannot escalate through an authorization request.
    NoUser,
    /// The headless visit to the authorization endpoint failed.
    Consent(ConsentError),
    /// rmcp's OAuth client refused or failed.
    Auth(AuthError),
}

impl From<AuthError> for FlowError {
    fn from(error: AuthError) -> Self {
        Self::Auth(error)
    }
}

impl From<ConsentError> for FlowError {
    fn from(error: ConsentError) -> Self {
        Self::Consent(error)
    }
}

/// What the lock guards.
struct State {
    /// `None` only while a session has it; every path puts it back.
    manager: Option<AuthorizationManager>,
    /// Authorization requests the session may still make.
    remaining: u32,
}

impl State {
    /// The manager, or the error for the state no path should leave behind.
    fn manager(&self) -> Result<&AuthorizationManager, AuthError> {
        self.manager.as_ref().ok_or_else(lost)
    }

    /// Takes one authorization request from the budget.
    fn spend(&mut self) -> Result<(), FlowError> {
        self.remaining = (self.remaining.checked_sub(1)).ok_or(FlowError::BudgetSpent)?;
        Ok(())
    }

    /// The token a request would carry now, if there is one.
    async fn token(&self) -> Result<Option<String>, AuthError> {
        match self.manager()?.get_access_token().await {
            Ok(token) => Ok(Some(token)),
            Err(AuthError::AuthorizationRequired) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Whether the token changed after a request carrying `sent` went out.
    async fn superseded(&self, sent: Option<&str>) -> Result<bool, AuthError> {
        let current = self.token().await?;
        Ok(current.is_some() && current.as_deref() != sent)
    }
}

/// The manager was not put back. Unreachable by construction; an error and
/// not a panic because this is a library.
fn lost() -> AuthError {
    AuthError::InternalError("authorization manager is in use".to_owned())
}

/// The authorization state of one session with one MCP server.
pub struct Flow {
    state: Mutex<State>,
    identity: Identity,
    consent: Box<dyn Consent>,
    /// The MCP server's URL: the RFC 8707 `resource` of a
    /// `client_credentials` token request.
    resource: String,
}

impl std::fmt::Debug for Flow {
    /// Nothing the manager holds is shown.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Flow")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

impl Flow {
    /// A flow for the MCP server at `server_url`, holding no token yet.
    ///
    /// # Errors
    ///
    /// `server_url` is not a URL, or rmcp's OAuth HTTP client cannot be built.
    pub async fn new(
        server_url: &str,
        identity: Identity,
        consent: Box<dyn Consent>,
    ) -> Result<Self, AuthError> {
        // `new`, not `new_allowing_private_addresses`: metadata may only send
        // rmcp to a loopback or private address when the server itself is at
        // one, and never to a link-local or cloud-metadata address.
        let manager = AuthorizationManager::new(server_url).await?;
        Ok(Self {
            state: Mutex::new(State {
                manager: Some(manager),
                remaining: AUTHORIZATION_BUDGET,
            }),
            identity,
            consent,
            resource: server_url.to_owned(),
        })
    }

    /// The access token to send, if one is held.
    ///
    /// # Errors
    ///
    /// The credential store or a token refresh failed.
    pub async fn token(&self) -> Result<Option<String>, AuthError> {
        self.state.lock().await.token().await
    }

    /// How many authorization requests the session has made.
    pub async fn spent(&self) -> u32 {
        AUTHORIZATION_BUDGET - self.state.lock().await.remaining
    }

    /// Obtains a token after a `401` whose `WWW-Authenticate` was `challenge`,
    /// for a request that carried `sent`.
    ///
    /// # Errors
    ///
    /// The budget is spent, discovery or registration failed, the
    /// authorization endpoint did not redirect to this client, or the token
    /// endpoint refused the exchange.
    pub async fn authorize(&self, challenge: &str, sent: Option<&str>) -> Result<(), FlowError> {
        let mut state = self.state.lock().await;
        if state.superseded(sent).await? {
            return Ok(());
        }
        state.spend()?;
        let manager = state.manager.take().ok_or_else(lost)?;
        let (manager, outcome) = match &self.identity {
            Identity::ClientCredentials {
                client_id,
                client_secret,
            } => {
                let config = ClientCredentialsConfig::ClientSecret {
                    client_id: client_id.clone(),
                    client_secret: client_secret.clone(),
                    scopes: Vec::new(),
                    resource: Some(self.resource.clone()),
                };
                client_credentials(manager, &config).await
            }
            identity => {
                let request = request_for(identity).with_challenge(challenge);
                self.authorization_code(manager, request).await
            }
        };
        state.manager = Some(manager);
        outcome
    }

    /// Obtains a token that also covers `required_scope` after a
    /// `403 insufficient_scope`, for a request that carried `sent`.
    ///
    /// # Errors
    ///
    /// As [`Flow::authorize`], and [`FlowError::NoUser`] for a
    /// `client_credentials` client.
    // The lock is held to the end on purpose: one authorization at a time,
    // and `manager` borrows from the guard the lint would have dropped.
    #[allow(clippy::significant_drop_tightening)]
    pub async fn step_up(&self, required_scope: &str, sent: Option<&str>) -> Result<(), FlowError> {
        let mut state = self.state.lock().await;
        if state.superseded(sent).await? {
            return Ok(());
        }
        if matches!(self.identity, Identity::ClientCredentials { .. }) {
            return Err(FlowError::NoUser);
        }
        state.spend()?;
        let manager = state.manager()?;
        // rmcp asks for the union of what was granted and what is now
        // required, so an escalation never drops a scope it already had.
        let authorization_url = manager.request_scope_upgrade(required_scope).await?;
        self.redeem(manager, &authorization_url).await
    }

    /// Discovery, registration, the authorization request and the exchange.
    /// Hands the manager back whatever happened.
    async fn authorization_code(
        &self,
        mut manager: AuthorizationManager,
        request: AuthorizationRequest,
    ) -> (AuthorizationManager, Result<(), FlowError>) {
        let resolution = manager
            .resolve_metadata_from_challenge(request.challenge.as_deref())
            .await;
        match resolution {
            Ok(resolution) => manager.set_metadata(resolution.metadata),
            Err(error) => return (manager, Err(error.into())),
        }
        match AuthorizationSession::new(manager, request).await {
            Ok(session) => {
                let authorization_url = session.get_authorization_url().to_owned();
                let outcome = (self.redeem(&session.auth_manager, &authorization_url)).await;
                (session.auth_manager, outcome)
            }
            Err((manager, error)) => (manager, Err(error.into())),
        }
    }

    /// Visits `authorization_url` and exchanges the code that comes back.
    ///
    /// The exchange is where rmcp checks `state` against the one it issued
    /// and sends the PKCE verifier, so a callback from any other
    /// authorization request ends here.
    async fn redeem(
        &self,
        manager: &AuthorizationManager,
        authorization_url: &str,
    ) -> Result<(), FlowError> {
        let callback_url = self.consent.approve(authorization_url).await?;
        let callback = AuthorizationCallback::from_redirect_url(&callback_url)?;
        manager
            .exchange_code_for_token_with_issuer(
                &callback.code,
                &callback.csrf_token,
                callback.issuer.as_deref(),
            )
            .await?;
        Ok(())
    }
}

/// The authorization request `identity` makes, before the challenge is added.
///
/// Scopes are left to rmcp's initial selection, which is the specification's:
/// the challenge's `scope`, else the protected resource's `scopes_supported`,
/// else none at all.
fn request_for(identity: &Identity) -> AuthorizationRequest {
    let request = AuthorizationRequest::new(REDIRECT_URI)
        .with_initial_scope_selection()
        .with_client_name(CLIENT_NAME)
        .with_client_metadata_url(CLIENT_METADATA_URL);
    match identity {
        Identity::Preregistered {
            client_id,
            client_secret,
        } => {
            let request = request.with_preregistered_client(client_id);
            match client_secret {
                Some(secret) => request.with_client_secret(secret),
                None => request,
            }
        }
        Identity::Discovered | Identity::ClientCredentials { .. } => request,
    }
}

/// The `client_credentials` grant: discovery, then the token request.
async fn client_credentials(
    mut manager: AuthorizationManager,
    config: &ClientCredentialsConfig,
) -> (AuthorizationManager, Result<(), FlowError>) {
    let outcome = async {
        let resolution = manager.resolve_metadata().await?;
        manager.set_metadata(resolution.metadata);
        manager.validate_client_credentials_metadata(config)?;
        manager.configure_client_credentials(config)?;
        manager.exchange_client_credentials(config).await?;
        Ok(())
    }
    .await;
    (manager, outcome)
}
