// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Default-secure HTTP transport policy: `Host` / `Origin` allowlisting.
//!
//! The `2025-11-25` transports specification requires: "Servers **MUST** validate the
//! `Origin` header on all incoming connections to prevent DNS rebinding attacks", with
//! 403 on failure — and the ecosystem's defining vulnerability in this class is
//! CVE-2026-42559 (rmcp < 1.4.0 accepted any `Host`). This module is that lesson as a
//! type:
//!
//! - [`HttpSecurityPolicy::default`] allows only loopback (`localhost`, `127.0.0.1`,
//!   `::1`) — matching the upstream fix's default.
//! - Validation **fails closed**: anything unparseable is denied.
//! - The only way to disable validation is [`HttpSecurityPolicy::dangerously_allow_any_host`],
//!   which is named the way it is so it cannot slip through review quietly.
//!
//! The policy is pure string logic with no I/O, so the eventual server's security
//! posture is testable (and mutation-tested) without sockets.

/// Allowlist policy for the `Host` and `Origin` headers of incoming HTTP requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpSecurityPolicy {
    /// Lowercased hostnames (or IP literals, unbracketed) that are acceptable.
    allowed_hosts: Vec<String>,
    /// When `true`, host validation is disabled entirely.
    allow_any_host: bool,
}

impl Default for HttpSecurityPolicy {
    /// Loopback-only, mirroring both the specification's local-deployment guidance and
    /// the rmcp 1.4.0 fix's default allowlist.
    fn default() -> Self {
        Self {
            allowed_hosts: vec![
                "localhost".to_owned(),
                "127.0.0.1".to_owned(),
                "::1".to_owned(),
            ],
            allow_any_host: false,
        }
    }
}

impl HttpSecurityPolicy {
    /// Replaces the allowlist with the given hostnames (case-insensitive; IPv6
    /// literals without brackets, e.g. `"::1"`).
    #[must_use]
    pub fn with_allowed_hosts<I, S>(hosts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allowed_hosts: hosts
                .into_iter()
                .map(|host| host.into().to_ascii_lowercase())
                .collect(),
            allow_any_host: false,
        }
    }

    /// Disables `Host`/`Origin` validation entirely.
    ///
    /// This reopens the DNS-rebinding class the default exists to close. It is
    /// acceptable only behind a reverse proxy that already enforces host policy, and
    /// the name is deliberately unpleasant to type and impossible to miss in review.
    #[must_use]
    pub const fn dangerously_allow_any_host(mut self) -> Self {
        self.allow_any_host = true;
        self
    }

    /// Whether a raw `Host` header value is acceptable.
    ///
    /// Accepts `host`, `host:port`, `[v6]`, and `[v6]:port` forms. Fails closed on
    /// anything else, including bare (unbracketed) IPv6-with-port ambiguity, empty
    /// values, embedded whitespace, and non-numeric or out-of-range ports.
    #[must_use]
    pub fn host_header_allowed(&self, value: &str) -> bool {
        if self.allow_any_host {
            return true;
        }
        extract_host(value).is_some_and(|host| self.host_allowed(&host))
    }

    /// Whether an `Origin` header value is acceptable.
    ///
    /// Accepts `http://` and `https://` origins whose host passes the allowlist.
    /// Everything else fails closed — including the literal `null` origin (sent by
    /// sandboxed or opaque-origin contexts), non-HTTP schemes, and values with paths,
    /// userinfo, or whitespace.
    #[must_use]
    pub fn origin_allowed(&self, value: &str) -> bool {
        if self.allow_any_host {
            return true;
        }
        let value = value.trim_ascii();
        let Some(authority) = value
            .strip_prefix("http://")
            .or_else(|| value.strip_prefix("https://"))
        else {
            return false;
        };
        if authority.contains('/') || authority.contains('@') {
            return false;
        }
        extract_host(authority).is_some_and(|host| self.host_allowed(&host))
    }

    fn host_allowed(&self, host: &str) -> bool {
        self.allowed_hosts.iter().any(|allowed| allowed == host)
    }
}

/// Extracts the lowercased host from a `host[:port]` / `[v6][:port]` authority string.
/// Returns `None` — deny — for anything malformed.
fn extract_host(value: &str) -> Option<String> {
    let value = value.trim_ascii();
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return None;
    }
    let (host, port) = if let Some(rest) = value.strip_prefix('[') {
        // Bracketed IPv6 literal: `[::1]` or `[::1]:8080`.
        let (host, after) = rest.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(port) => Some(port),
            None if after.is_empty() => None,
            None => return None,
        };
        (host, port)
    } else if let Some((host, port)) = value.split_once(':') {
        // A second colon means an unbracketed IPv6 literal — ambiguous as
        // host-with-port, so denied.
        if port.contains(':') {
            return None;
        }
        (host, Some(port))
    } else {
        (value, None)
    };
    if host.is_empty() {
        return None;
    }
    if let Some(port) = port {
        if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let numeric: u32 = port.parse().ok()?;
        if numeric > 65_535 {
            return None;
        }
    }
    Some(host.to_ascii_lowercase())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn default_allows_loopback_host_forms() {
        let policy = HttpSecurityPolicy::default();
        for value in [
            "localhost",
            "localhost:8080",
            "LOCALHOST:8080",
            "127.0.0.1",
            "127.0.0.1:3000",
            "[::1]",
            "[::1]:3000",
            " localhost ",
        ] {
            assert!(policy.host_header_allowed(value), "should allow {value:?}");
        }
    }

    #[test]
    fn default_denies_rebinding_and_malformed_hosts() {
        let policy = HttpSecurityPolicy::default();
        for value in [
            "evil.example",
            "localhost.evil.example",
            "127.0.0.1.evil.example",
            "localhost:8080:80",
            "::1",             // unbracketed IPv6 is ambiguous — denied
            "[::1]x",          // junk after the bracket
            "[::1]:",          // empty port
            "[::1]:http",      // non-numeric port
            "localhost:99999", // port out of range
            "",
            "   ",
            "local host",
            "[]",
            "[]:80",
        ] {
            assert!(!policy.host_header_allowed(value), "should deny {value:?}");
        }
    }

    #[test]
    fn origin_validation_accepts_loopback_http_origins_only() {
        let policy = HttpSecurityPolicy::default();
        for value in [
            "http://localhost",
            "http://localhost:6274",
            "https://127.0.0.1:8443",
            "https://[::1]:8443",
        ] {
            assert!(policy.origin_allowed(value), "should allow {value:?}");
        }
        for value in [
            "http://evil.example",
            "https://localhost.evil.example",
            "null",
            "file://localhost",
            "ws://localhost",
            "http://localhost/path",
            "http://user@localhost",
            "localhost",
            "",
        ] {
            assert!(!policy.origin_allowed(value), "should deny {value:?}");
        }
    }

    #[test]
    fn custom_allowlist_replaces_default() {
        let policy = HttpSecurityPolicy::with_allowed_hosts(["mcp.internal.example"]);
        assert!(policy.host_header_allowed("MCP.Internal.Example:443"));
        assert!(!policy.host_header_allowed("localhost"));
        assert!(policy.origin_allowed("https://mcp.internal.example"));
        assert!(!policy.origin_allowed("https://localhost"));
    }

    #[test]
    fn dangerous_opt_out_is_total_and_explicit() {
        let policy = HttpSecurityPolicy::default().dangerously_allow_any_host();
        assert!(policy.host_header_allowed("evil.example"));
        assert!(policy.origin_allowed("http://evil.example"));
        // Even nonsense passes — the opt-out means "no validation", not "lenient
        // validation", and pretending otherwise would be false comfort.
        assert!(policy.host_header_allowed(""));
    }

    proptest! {
        #[test]
        fn never_panics_on_arbitrary_host_headers(value in ".*") {
            let policy = HttpSecurityPolicy::default();
            let _ = policy.host_header_allowed(&value);
            let _ = policy.origin_allowed(&value);
        }

        #[test]
        fn allowed_hosts_accept_their_own_name_with_valid_ports(port in 0u32..=65_535) {
            let policy = HttpSecurityPolicy::default();
            // Bound first: prop_assert! stringifies its expression into a format
            // string, so an inline `{port}` capture would be misparsed there.
            let value = format!("localhost:{port}");
            prop_assert!(policy.host_header_allowed(&value));
        }

        #[test]
        fn unrelated_hosts_are_denied_regardless_of_port(port in 0u32..=65_535) {
            let policy = HttpSecurityPolicy::default();
            let value = format!("evil.example:{port}");
            prop_assert!(!policy.host_header_allowed(&value));
        }
    }

    #[test]
    fn origin_userinfo_and_path_bans_hold_even_for_matching_allowlists() {
        // Defense-in-depth: the '/' and '@' bans must deny independently of the
        // host comparison, even when a (misguided) custom allowlist would match the
        // raw authority string.
        let at_policy = HttpSecurityPolicy::with_allowed_hosts(["user@example"]);
        assert!(!at_policy.origin_allowed("http://user@example"));
        let slash_policy = HttpSecurityPolicy::with_allowed_hosts(["example/x"]);
        assert!(!slash_policy.origin_allowed("http://example/x"));
        // Same for the whitespace ban in host extraction.
        let space_policy = HttpSecurityPolicy::with_allowed_hosts(["local host"]);
        assert!(!space_policy.host_header_allowed("local host"));
    }

    #[test]
    fn port_digit_check_is_not_redundant_with_integer_parsing() {
        // Rust's u32 parsing accepts a leading '+'; without the digits check,
        // "localhost:+80" would slip through as port 80.
        let policy = HttpSecurityPolicy::default();
        assert!(!policy.host_header_allowed("localhost:+80"));
    }

    #[test]
    fn port_range_boundary_is_inclusive_at_65535() {
        let policy = HttpSecurityPolicy::default();
        assert!(policy.host_header_allowed("localhost:65535"));
        assert!(!policy.host_header_allowed("localhost:65536"));
    }
}
