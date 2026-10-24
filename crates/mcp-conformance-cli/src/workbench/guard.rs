// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What every request passes before the workbench reads it, and what every
//! response carries ([05-security-model.md], "The workbench").
//!
//! The workbench is a web server on the user's machine showing recordings
//! that may hold credentials. The threats are a web page in the user's
//! browser — reaching it by DNS rebinding (a hostile name resolving to
//! `127.0.0.1`) or by a cross-site request — and anything else that can
//! reach the port. In order:
//!
//! 1. **`Host`** must be the address the workbench is bound to, by IP or (on
//!    loopback) as `localhost`, with its port: a rebound name is refused
//!    before anything else is read.
//! 2. **`Origin`**, when present, must be this workbench's own origin; a
//!    state-changing request (anything but `GET` and `HEAD`) must carry it,
//!    and a JSON body must say it is JSON — so neither a form nor a
//!    cross-site `fetch` can change the library.
//! 3. **`Sec-Fetch-Site`**, for the API. Browsers do not separate cookies
//!    by port, and a page on another port of this host is *same-site*, not
//!    cross-site: its `<img>` or link sends a `GET` with the workbench's
//!    cookie and no `Origin`, which the two checks above let through. So
//!    `/api/…` answers only when the header is absent (`curl`),
//!    `same-origin` (the workbench's own page) or `none` (a navigation the
//!    user makes — the printed link); the page and its assets, which show
//!    nothing the API does not answer, are not held to it.
//! 4. **The token.** The launch URL carries a random single-use token; the
//!    first request with it gets the session secret as a cookie
//!    (`HttpOnly`, `SameSite=Strict`) and a redirect to a URL without it.
//!    Every other request needs that cookie. Secrets are 32 bytes from the
//!    operating system's random source, compared in constant time.
//!
//! Every response — a refusal included — carries a Content Security Policy
//! with no inline script or style and no connection beyond the workbench's
//! own origin, `nosniff`, `no-referrer`, `no-store`, frame denial and
//! same-origin isolation, and no CORS header at all.
//!
//! [05-security-model.md]: https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/05-security-model.md

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::Response;
use ring::rand::{SecureRandom as _, SystemRandom};

/// The policy every workbench response carries.
pub(crate) const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; \
     style-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; \
     frame-ancestors 'none'";

/// The policy of a downloaded HTML report. It is sent as an attachment, so
/// a browser saves it rather than showing it; should one show it in place,
/// the sandbox and `default-src 'none'` let it run and load nothing. (Opened
/// from disk, the page's own `<meta>` policy — its stylesheet's digest and
/// nothing else — is what applies.)
pub(crate) const REPORT_POLICY: &str =
    "sandbox; default-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// The secrets of one launch.
#[derive(Debug)]
pub(crate) struct Keys {
    /// In the printed URL; good for one exchange.
    launch: String,
    launched: AtomicBool,
    /// In the cookie.
    session: String,
    /// The `Host` values accepted.
    hosts: Vec<String>,
    /// The origin accepted.
    origins: Vec<String>,
    /// The cookie's name: per port, so two workbenches do not overwrite
    /// each other's cookie (cookies are shared across a host's ports).
    cookie: String,
}

/// A refusal: the status, and a plain-text reason.
pub(crate) type Refusal = (StatusCode, &'static str);

/// 32 random bytes as 64 lowercase hex digits.
pub(crate) fn secret() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "the operating system's random source failed".to_owned())?;
    Ok(super::entry::hex(&bytes))
}

/// Whether `a` and `b` are equal, in time that depends only on their
/// lengths.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0_u8, |difference, (x, y)| difference | (x ^ y))
            == 0
}

/// The value of query parameter `name` in `uri`, when present.
pub(crate) fn query<'a>(uri: &'a Uri, name: &str) -> Option<&'a str> {
    uri.query()?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

impl Keys {
    /// The keys for a workbench bound to `address`.
    pub(crate) fn new(address: SocketAddr) -> Result<Self, String> {
        let port = address.port();
        let ip = match address {
            SocketAddr::V4(v4) => v4.ip().to_string(),
            SocketAddr::V6(v6) => format!("[{}]", v6.ip()),
        };
        let mut hosts = vec![format!("{ip}:{port}")];
        if address.ip().is_loopback() {
            hosts.push(format!("localhost:{port}"));
        }
        let origins = hosts.iter().map(|host| format!("http://{host}")).collect();
        Ok(Self {
            launch: secret()?,
            launched: AtomicBool::new(false),
            session: secret()?,
            hosts,
            origins,
            cookie: format!("mcp-workbench-{port}"),
        })
    }

    /// The URL to open: the bound address and the launch token.
    pub(crate) fn launch_url(&self) -> String {
        format!("{}/?token={}", self.origins[0], self.launch)
    }

    /// The cookie a browser holds after launching.
    #[cfg(test)]
    pub(crate) fn cookie(&self) -> String {
        format!("{}={}", self.cookie, self.session)
    }

    /// The launch token.
    #[cfg(test)]
    pub(crate) fn launch_token(&self) -> &str {
        &self.launch
    }

    /// The `Host` and `Origin` checks, before anything else.
    pub(crate) fn check_origin(&self, method: &Method, headers: &HeaderMap) -> Result<(), Refusal> {
        let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
        if !host.is_some_and(|host| self.hosts.iter().any(|ours| ours == host)) {
            return Err((
                StatusCode::FORBIDDEN,
                "forbidden: this Host is not the workbench's address\n",
            ));
        }
        let origin = headers
            .get(header::ORIGIN)
            .map(|v| v.to_str().unwrap_or(""));
        match origin {
            Some(origin) if !self.origins.iter().any(|ours| ours == origin) => {
                return Err((StatusCode::FORBIDDEN, "forbidden: cross-origin request\n"));
            }
            None if !matches!(*method, Method::GET | Method::HEAD) => {
                return Err((
                    StatusCode::FORBIDDEN,
                    "forbidden: a change needs the workbench's Origin\n",
                ));
            }
            _ => {}
        }
        if headers
            .get("sec-fetch-site")
            .is_some_and(|site| site.as_bytes() == b"cross-site")
        {
            return Err((StatusCode::FORBIDDEN, "forbidden: cross-site request\n"));
        }
        Ok(())
    }

    /// The API's `Sec-Fetch-Site` check: answered only to the workbench's
    /// own page (`same-origin`), to a navigation the user makes (`none`) or
    /// to a client that sends no such header. A page on another port of
    /// this host is `same-site`, and is refused like `cross-site`.
    pub(crate) fn check_api_fetch_site(headers: &HeaderMap) -> Result<(), Refusal> {
        match headers.get("sec-fetch-site").map(HeaderValue::as_bytes) {
            None | Some(b"same-origin" | b"none") => Ok(()),
            Some(_) => Err((
                StatusCode::FORBIDDEN,
                "forbidden: the API answers only the workbench's own page\n",
            )),
        }
    }

    /// Whether the request carries the session cookie.
    pub(crate) fn authenticated(&self, headers: &HeaderMap) -> bool {
        headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .any(|(name, value)| name == self.cookie && same(value, &self.session))
    }

    /// Exchanges the launch token for the cookie: `Some` with the
    /// `Set-Cookie` value when `uri` carries the unused launch token.
    pub(crate) fn launch(&self, uri: &Uri) -> Option<Result<String, Refusal>> {
        let token = query(uri, "token")?;
        if !same(token, &self.launch) {
            return Some(Err((
                StatusCode::FORBIDDEN,
                "forbidden: this is not the launch link\n",
            )));
        }
        if self.launched.swap(true, Ordering::SeqCst) {
            return Some(Err((
                StatusCode::FORBIDDEN,
                "forbidden: this launch link has been used; restart `mcp-conformance serve` for a new one\n",
            )));
        }
        Some(Ok(format!(
            "{}={}; Path=/; HttpOnly; SameSite=Strict",
            self.cookie, self.session
        )))
    }
}

/// Adds the headers every response carries.
pub(crate) fn harden(response: &mut Response, policy: &'static str) {
    let headers = response.headers_mut();
    for (name, value) in [
        (header::CONTENT_SECURITY_POLICY, policy),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::CACHE_CONTROL, "no-store"),
        (header::X_FRAME_OPTIONS, "DENY"),
        (
            header::HeaderName::from_static("cross-origin-opener-policy"),
            "same-origin",
        ),
        (
            header::HeaderName::from_static("cross-origin-resource-policy"),
            "same-origin",
        ),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
}

/// A plain-text response.
pub(crate) fn plain(status: StatusCode, text: &'static str) -> Response {
    let mut response = Response::new(axum::body::Body::from(text));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

#[cfg(test)]
mod tests;
