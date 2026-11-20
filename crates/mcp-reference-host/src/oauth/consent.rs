// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The headless stand-in for the browser.
//!
//! In a real authorization the user's browser opens the authorization URL, the
//! user agrees, and the authorization server redirects the browser to the
//! client's redirect URI with a code. Here nobody is asked: [`Headless`]
//! requests the URL itself, does not follow the redirect, and hands the
//! `Location` back as the callback.
//!
//! That is approval on nobody's behalf, so it is fenced twice. The
//! authorization endpoint must be a loopback address ([`is_loopback`]) — the
//! suite's authorization server is, and no real one is — and the redirect
//! must go to the host's own redirect URI ([`callback`]), so a response
//! pointing somewhere else is refused rather than parsed for a code.

use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;

use reqwest::Url;
use reqwest::header::LOCATION;

/// Why an authorization URL produced no callback.
///
/// No variant carries the URL, its query or the `Location`: the first holds
/// the `state` and the PKCE challenge, the second the code.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConsentError {
    /// The authorization URL is not an absolute `http`/`https` URL.
    NotAUrl,
    /// The authorization endpoint is not a loopback address; the host named
    /// is carried so the refusal can say which.
    NotLoopback(String),
    /// The request to the authorization endpoint failed.
    Unreachable,
    /// The authorization endpoint answered with this status, not a redirect.
    NotARedirect(u16),
    /// The redirect carried no readable `Location`.
    NoLocation,
    /// The redirect does not go to the host's redirect URI.
    ForeignRedirect,
}

impl std::fmt::Display for ConsentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAUrl => formatter.write_str("the authorization URL is not an http(s) URL"),
            Self::NotLoopback(host) => write!(
                formatter,
                "refusing to approve an authorization at {host}: --oauth-conformance \
                 answers for the user, so it only does so at a loopback address"
            ),
            Self::Unreachable => {
                formatter.write_str("the authorization endpoint could not be reached")
            }
            Self::NotARedirect(status) => write!(
                formatter,
                "the authorization endpoint answered {status}, not a redirect"
            ),
            Self::NoLocation => {
                formatter.write_str("the authorization redirect carried no Location")
            }
            Self::ForeignRedirect => formatter
                .write_str("the authorization redirect does not go to this client's redirect URI"),
        }
    }
}

impl std::error::Error for ConsentError {}

/// The future [`Consent::approve`] returns.
pub type ConsentFuture<'a> =
    Pin<Box<dyn Future<Output = Result<String, ConsentError>> + Send + 'a>>;

/// Whatever turns an authorization URL into the callback URL the
/// authorization server redirected to.
///
/// A trait so the flow's tests can stand in an authorization server that
/// redirects wrongly, or not at all, without a socket.
pub trait Consent: Send + Sync {
    /// Visits `authorization_url` and returns the callback URL, query included.
    fn approve<'a>(&'a self, authorization_url: &'a str) -> ConsentFuture<'a>;
}

/// Whether `url` names this machine: `localhost` or a loopback IP literal.
///
/// By name and literal only. Nothing is resolved, so a public name that
/// happens to resolve to `127.0.0.1` is not loopback here.
#[must_use]
pub fn is_loopback(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    // An IPv6 literal is bracketed in a URL and bare to the address parser.
    let literal = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'));
    (literal.unwrap_or(host).parse::<IpAddr>()).is_ok_and(|address| address.is_loopback())
}

/// The authorization URL as a request target, if the host may visit it.
///
/// # Errors
///
/// It is not an absolute `http`/`https` URL, or its host is not loopback.
pub fn target(authorization_url: &str) -> Result<Url, ConsentError> {
    let url = Url::parse(authorization_url).map_err(|_| ConsentError::NotAUrl)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ConsentError::NotAUrl);
    }
    if !is_loopback(&url) {
        let host = url.host_str().unwrap_or_default().to_owned();
        return Err(ConsentError::NotLoopback(host));
    }
    Ok(url)
}

/// `location`, if it is `redirect_uri` with a query appended.
///
/// Scheme, host, port and path must all be the redirect URI's: the code is
/// only read from a redirect to the address the client registered.
///
/// # Errors
///
/// `location` does not parse, or points anywhere else.
pub fn callback(location: &str, redirect_uri: &str) -> Result<String, ConsentError> {
    let location = Url::parse(location).map_err(|_| ConsentError::NoLocation)?;
    let expected = Url::parse(redirect_uri).map_err(|_| ConsentError::ForeignRedirect)?;
    let same = location.scheme() == expected.scheme()
        && location.host() == expected.host()
        && location.port_or_known_default() == expected.port_or_known_default()
        && location.path() == expected.path();
    if same {
        Ok(location.into())
    } else {
        Err(ConsentError::ForeignRedirect)
    }
}

/// The real thing: one `GET`, redirect not followed.
#[derive(Debug, Clone)]
pub struct Headless {
    http: reqwest::Client,
    redirect_uri: String,
}

impl Headless {
    /// A visitor expecting redirects to `redirect_uri`.
    ///
    /// # Errors
    ///
    /// The HTTP client cannot be built.
    pub fn new(redirect_uri: &str) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            // The redirect is the answer, not somewhere to go: nothing
            // listens on the redirect URI.
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            http,
            redirect_uri: redirect_uri.to_owned(),
        })
    }
}

impl Consent for Headless {
    fn approve<'a>(&'a self, authorization_url: &'a str) -> ConsentFuture<'a> {
        Box::pin(async move {
            let url = target(authorization_url)?;
            let response =
                (self.http.get(url).send().await).map_err(|_| ConsentError::Unreachable)?;
            let status = response.status();
            if !status.is_redirection() {
                return Err(ConsentError::NotARedirect(status.as_u16()));
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(ConsentError::NoLocation)?;
            callback(location, &self.redirect_uri)
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn loopback_is_localhost_and_the_loopback_literals_and_nothing_else() {
        for loopback in [
            "http://localhost:8080/authorize",
            "http://LOCALHOST/authorize",
            "http://127.0.0.1:9/authorize",
            "http://127.8.9.10/authorize",
            "https://[::1]:9/authorize",
        ] {
            assert!(is_loopback(&url(loopback)), "{loopback}");
        }
        for elsewhere in [
            "https://auth.example.com/authorize",
            "http://localhost.example.com/authorize",
            "http://notlocalhost/authorize",
            "http://10.0.0.1/authorize",
            "http://169.254.169.254/latest",
            "http://0.0.0.0/authorize",
            "http://[::2]/authorize",
            "http://[fe80::1]/authorize",
            // No host at all.
            "data:text/plain,localhost",
        ] {
            assert!(!is_loopback(&url(elsewhere)), "{elsewhere}");
        }
    }

    #[test]
    fn a_target_is_an_http_url_on_this_machine() {
        let ok = target("http://127.0.0.1:9/authorize?state=s").unwrap();
        assert_eq!(ok.as_str(), "http://127.0.0.1:9/authorize?state=s");
        assert!(target("https://localhost/authorize").is_ok());
        assert_eq!(target("/authorize"), Err(ConsentError::NotAUrl));
        assert_eq!(target(""), Err(ConsentError::NotAUrl));
        // Loopback, but not something to send a GET to.
        assert_eq!(
            target("ftp://127.0.0.1/authorize"),
            Err(ConsentError::NotAUrl)
        );
        assert_eq!(
            target("file://localhost/etc/passwd"),
            Err(ConsentError::NotAUrl)
        );
        assert_eq!(
            target("https://auth.example.com/authorize?state=s"),
            Err(ConsentError::NotLoopback("auth.example.com".to_owned()))
        );
    }

    #[test]
    fn a_callback_is_the_redirect_uri_and_only_that() {
        let redirect = "http://localhost:3000/callback";
        assert_eq!(
            callback("http://localhost:3000/callback?code=c&state=s", redirect),
            Ok("http://localhost:3000/callback?code=c&state=s".to_owned())
        );
        // An explicit default port is the same address.
        assert!(callback("http://localhost:80/cb?code=c", "http://localhost/cb").is_ok());
        for foreign in [
            "https://localhost:3000/callback?code=c&state=s",
            "http://127.0.0.1:3000/callback?code=c&state=s",
            "http://localhost:3001/callback?code=c&state=s",
            "http://localhost:3000/callback/more?code=c&state=s",
            "http://localhost:3000/?code=c&state=s",
        ] {
            assert_eq!(
                callback(foreign, redirect),
                Err(ConsentError::ForeignRedirect),
                "{foreign}"
            );
        }
        assert_eq!(
            callback("/callback?code=c", redirect),
            Err(ConsentError::NoLocation)
        );
        assert_eq!(
            callback("http://localhost:3000/callback?code=c", "not a url"),
            Err(ConsentError::ForeignRedirect)
        );
    }

    #[test]
    fn every_refusal_says_what_happened_and_nothing_secret() {
        let cases = [
            (
                ConsentError::NotAUrl,
                "the authorization URL is not an http(s) URL",
            ),
            (
                ConsentError::Unreachable,
                "the authorization endpoint could not be reached",
            ),
            (
                ConsentError::NotARedirect(418),
                "the authorization endpoint answered 418, not a redirect",
            ),
            (
                ConsentError::NoLocation,
                "the authorization redirect carried no Location",
            ),
            (
                ConsentError::ForeignRedirect,
                "the authorization redirect does not go to this client's redirect URI",
            ),
        ];
        for (error, text) in cases {
            assert_eq!(error.to_string(), text);
        }
        let refusal = ConsentError::NotLoopback("auth.example.com".to_owned()).to_string();
        assert!(
            refusal.starts_with("refusing to approve an authorization at auth.example.com:"),
            "{refusal}"
        );
        assert!(refusal.contains("loopback"), "{refusal}");
    }

    #[tokio::test]
    async fn the_headless_visit_refuses_a_remote_endpoint_before_any_request() {
        let headless = Headless::new("http://localhost:3000/callback").unwrap();
        // `.invalid` never resolves: an attempt to connect would be
        // `Unreachable`, so `NotLoopback` shows none was made.
        let refused = headless
            .approve("https://auth.invalid/authorize?state=s")
            .await;
        assert_eq!(
            refused,
            Err(ConsentError::NotLoopback("auth.invalid".to_owned()))
        );
        assert_eq!(
            headless.approve("nonsense").await,
            Err(ConsentError::NotAUrl)
        );
    }

    #[tokio::test]
    async fn the_headless_visit_reports_an_endpoint_nobody_serves() {
        let headless = Headless::new("http://localhost:3000/callback").unwrap();
        let dead = headless.approve("http://127.0.0.1:1/authorize").await;
        assert_eq!(dead, Err(ConsentError::Unreachable));
    }
}
