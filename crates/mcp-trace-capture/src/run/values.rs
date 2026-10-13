// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The two command-line values a run checks itself, shared by every command
//! line over [`super`]: `--upstream` and `--listen`.
//!
//! `--upstream` is taken as text and parsed here, not by clap: clap's error
//! repeats the value, and an upstream URL can carry a password or an API key
//! (`http://user:pa ss@host/`, `?key=…`). Errors about it say what is wrong
//! without the value.

use std::net::SocketAddr;

use axum::http::Uri;

/// Parses `--upstream` without ever repeating it.
///
/// # Errors
///
/// What is wrong with it, for the user — naming at most the host and path of a
/// URL that lacks only its scheme, never a password or a query.
pub fn upstream(text: &str) -> Result<Uri, String> {
    let authority = text
        .split_once("://")
        .map_or(text, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    if authority.contains('@') {
        return Err(
            "the URL carries credentials (user:password@), which the proxy would not send; \
             have the client send them instead, e.g. as an Authorization header"
                .to_owned(),
        );
    }
    if !text.contains("://")
        && let Ok(guess) = format!("http://{text}").parse::<Uri>()
        && guess.host().is_some()
    {
        return Err(format!(
            "the URL has no scheme; did you mean http://{}{}?",
            guess.authority().map_or("", |authority| authority.as_str()),
            guess.path().trim_end_matches('/')
        ));
    }
    text.parse().map_err(|_| {
        "not a valid URL (the value is not repeated, since it can carry credentials); a \
         space or other character a URL cannot hold must be percent-encoded"
            .to_owned()
    })
}

/// Parses `--listen`, explaining the two near misses: a port alone, and a host
/// name where an IP address belongs.
///
/// # Errors
///
/// Not an `IP:PORT`, with what was probably meant.
pub fn listen_address(text: &str) -> Result<SocketAddr, String> {
    if let Ok(address) = text.parse() {
        return Ok(address);
    }
    if text.parse::<u16>().is_ok() {
        return Err(format!(
            "an IP address and a port are needed, e.g. 127.0.0.1:{text}"
        ));
    }
    if let Some((_, port)) = text.rsplit_once(':')
        && port.parse::<u16>().is_ok()
    {
        return Err(format!(
            "the address must be an IP address, not a host name, e.g. 127.0.0.1:{port} or \
             [::1]:{port}"
        ));
    }
    Err("expected IP:PORT, e.g. 127.0.0.1:8080".to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn an_upstream_is_explained_without_being_repeated() {
        for secret in [
            "http://user:pa ss@host/",
            "http://user:secret@host/mcp",
            "user:secret@host:1/mcp",
        ] {
            let error = upstream(secret).unwrap_err();
            assert!(error.contains("credentials"), "{error}");
            assert!(
                !error.contains("secret") && !error.contains("pa ss"),
                "{error}"
            );
        }
        let error = upstream("http://h/?key=SEC RET").unwrap_err();
        assert!(!error.contains("SEC"), "{error}");
        assert!(error.contains("not a valid URL"), "{error}");
        // `@` in the query is not credentials.
        assert!(upstream("http://h/mcp?to=a@b").is_ok());
        assert!(upstream("http://h:1/mcp").is_ok());
    }

    #[test]
    fn an_upstream_without_a_scheme_gets_one_suggested() {
        assert_eq!(
            upstream("localhost:3999").unwrap_err(),
            "the URL has no scheme; did you mean http://localhost:3999?"
        );
        assert_eq!(
            upstream("localhost:3999/mcp?key=k").unwrap_err(),
            "the URL has no scheme; did you mean http://localhost:3999/mcp?"
        );
        // A path alone is a URL, if not an absolute one: `Options::new` says so.
        assert!(upstream("/mcp").is_ok());
    }

    #[test]
    fn a_listen_address_near_miss_is_explained() {
        assert_eq!(
            listen_address("127.0.0.1:1").unwrap(),
            "127.0.0.1:1".parse().unwrap()
        );
        assert_eq!(
            listen_address("18081").unwrap_err(),
            "an IP address and a port are needed, e.g. 127.0.0.1:18081"
        );
        assert!(
            listen_address("localhost:18081")
                .unwrap_err()
                .contains("not a host name, e.g. 127.0.0.1:18081 or [::1]:18081")
        );
        assert!(
            listen_address("localhost:port")
                .unwrap_err()
                .starts_with("expected IP:PORT")
        );
        assert!(
            listen_address("")
                .unwrap_err()
                .starts_with("expected IP:PORT")
        );
    }
}
