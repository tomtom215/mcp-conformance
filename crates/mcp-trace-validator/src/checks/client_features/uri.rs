// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What "a valid URL" can mean to a validator: RFC 3986's generic syntax.
//!
//! `client/elicitation` says only that "The `url` parameter MUST contain a valid
//! URL", and names no grammar. The one a recording can be held to without
//! guessing is RFC 3986 §3, the syntax every URL scheme shares:
//!
//! ```text
//! URI         = scheme ":" hier-part [ "?" query ] [ "#" fragment ]
//! hier-part   = "//" authority path-abempty / path-absolute / path-rootless / path-empty
//! authority   = [ userinfo "@" ] host [ ":" port ]
//! ```
//!
//! A scheme is required — the URL is something a user navigates to, so a
//! relative reference resolves against nothing — and the fragment is allowed,
//! because a URL may carry one. Characters outside the grammar are refused,
//! including non-ASCII: RFC 3986 admits them only percent-encoded, and an
//! internationalized host or path is an IRI (RFC 3987), not a URL.
//!
//! One scheme-specific rule is added, because the generic grammar admits it and
//! the scheme's own definition does not: an `http` or `https` URL must have an
//! authority with a non-empty host (RFC 9110 §4.2.1–4.2.2, "A sender MUST NOT
//! generate an "http" URI with an empty host identifier"). Nothing else about
//! any scheme is judged — whether `mailto:` or `urn:` addresses make sense for
//! the user to open is a question for the client's safe-URL rules, which bind
//! the client and are judged nowhere in a trace.
//!
//! Written here rather than taken from a crate: the grammar is small and exact,
//! and the URL crates parse the WHATWG URL Standard, which repairs what RFC 3986
//! rejects (spaces, backslashes, non-ASCII hosts) — the opposite of what a check
//! wants.

/// Why `text` is not an RFC 3986 URI, or `Ok` when it is one.
pub(super) fn check(text: &str) -> Result<(), String> {
    let Some((scheme, rest)) = text.split_once(':') else {
        return Err("has no scheme (no `:`)".to_owned());
    };
    if !is_scheme(scheme) {
        return Err(format!(
            "has the scheme {scheme:?}, which is not `ALPHA *( ALPHA / DIGIT / \"+\" / \"-\" / \".\" )`"
        ));
    }
    let (before_fragment, fragment) = split_off(rest, '#');
    if let Some(fragment) = fragment {
        all_of(fragment, is_query_char).map_err(|why| format!("has a fragment that {why}"))?;
    }
    let (hier_part, query) = split_off(before_fragment, '?');
    if let Some(query) = query {
        all_of(query, is_query_char).map_err(|why| format!("has a query that {why}"))?;
    }
    let host_required = scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
    if let Some(after_slashes) = hier_part.strip_prefix("//") {
        let (authority, path) = after_slashes
            .find('/')
            .map_or((after_slashes, ""), |at| after_slashes.split_at(at));
        let empty_host = check_authority(authority)?;
        if host_required && empty_host {
            return Err(format!("is an {scheme} URL with an empty host"));
        }
        all_of(path, is_path_char).map_err(|why| format!("has a path that {why}"))
    } else if host_required {
        Err(format!("is an {scheme} URL with no authority (`//host`)"))
    } else {
        all_of(hier_part, is_path_char).map_err(|why| format!("has a path that {why}"))
    }
}

/// `text` split at the first `separator`, the part after it excluded.
fn split_off(text: &str, separator: char) -> (&str, Option<&str>) {
    text.split_once(separator)
        .map_or((text, None), |(before, after)| (before, Some(after)))
}

/// `scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`.
fn is_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Checks `authority = [ userinfo "@" ] host [ ":" port ]`, returning whether
/// the host is empty — which only a `reg-name` can be.
fn check_authority(authority: &str) -> Result<bool, String> {
    let (userinfo, host_port) = match authority.split_once('@') {
        Some((userinfo, host_port)) => (Some(userinfo), host_port),
        None => (None, authority),
    };
    if let Some(userinfo) = userinfo {
        all_of(userinfo, |c| {
            is_unreserved(c) || is_sub_delim(c) || c == ':'
        })
        .map_err(|why| format!("has userinfo that {why}"))?;
    }
    let (empty_host, port) = if let Some(literal) = host_port.strip_prefix('[') {
        let Some((inside, after)) = literal.split_once(']') else {
            return Err("has an IP literal with no closing `]`".to_owned());
        };
        if !is_ip_literal(inside) {
            return Err(format!(
                "has the IP literal [{inside}], which is neither an IPv6 address nor IPvFuture"
            ));
        }
        let port = match after {
            "" => None,
            _ => Some(after.strip_prefix(':').ok_or_else(|| {
                format!("has {after:?} after its IP literal, where only `:port` may follow")
            })?),
        };
        (false, port)
    } else {
        let (host, port) = split_off(host_port, ':');
        all_of(host, |c| is_unreserved(c) || is_sub_delim(c))
            .map_err(|why| format!("has a host that {why}"))?;
        (host.is_empty(), port)
    };
    if let Some(port) = port
        && !port.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(format!("has the port {port:?}, which is not `*DIGIT`"));
    }
    Ok(empty_host)
}

/// `IP-literal = "[" ( IPv6address / IPvFuture ) "]"`, brackets removed.
fn is_ip_literal(inside: &str) -> bool {
    is_ipv6(inside) || is_ip_future(inside)
}

/// `IPvFuture = "v" 1*HEXDIG "." 1*( unreserved / sub-delims / ":" )`.
fn is_ip_future(text: &str) -> bool {
    let Some(rest) = text.strip_prefix(['v', 'V']) else {
        return false;
    };
    let Some((version, address)) = rest.split_once('.') else {
        return false;
    };
    !version.is_empty()
        && version.bytes().all(|b| b.is_ascii_hexdigit())
        && !address.is_empty()
        && address
            .chars()
            .all(|c| is_unreserved(c) || is_sub_delim(c) || c == ':')
}

/// RFC 3986 `IPv6address`: eight 16-bit pieces, the last two of which may be
/// written as an IPv4 address, with at most one `::` standing for one or more
/// zero pieces.
fn is_ipv6(text: &str) -> bool {
    let (head, tail, compressed) = match text.split_once("::") {
        Some((head, tail)) => (head, tail, true),
        None => (text, "", false),
    };
    if compressed && tail.contains("::") {
        return false;
    }
    let pieces = |part: &str, may_end_in_ipv4: bool| -> Option<usize> {
        if part.is_empty() {
            return Some(0);
        }
        let groups: Vec<&str> = part.split(':').collect();
        let mut count = 0;
        for (index, group) in groups.iter().enumerate() {
            let last = index + 1 == groups.len();
            if last && may_end_in_ipv4 && group.contains('.') {
                if !is_ipv4(group) {
                    return None;
                }
                count += 2;
            } else if (1..=4).contains(&group.len()) && group.bytes().all(|b| b.is_ascii_hexdigit())
            {
                count += 1;
            } else {
                return None;
            }
        }
        Some(count)
    };
    // The IPv4 form may only end the address, so it is allowed at the end of
    // the tail — or of the head when nothing was compressed.
    let Some(head_count) = pieces(head, !compressed) else {
        return false;
    };
    let Some(tail_count) = pieces(tail, compressed) else {
        return false;
    };
    let total = head_count + tail_count;
    if compressed { total <= 7 } else { total == 8 }
}

/// `IPv4address = dec-octet "." dec-octet "." dec-octet "." dec-octet`, where a
/// `dec-octet` is 0–255 with no leading zero.
fn is_ipv4(text: &str) -> bool {
    let octets: Vec<&str> = text.split('.').collect();
    octets.len() == 4
        && octets.iter().all(|octet| {
            !octet.is_empty()
                && octet.len() <= 3
                && octet.bytes().all(|b| b.is_ascii_digit())
                && (octet.len() == 1 || !octet.starts_with('0'))
                && octet.parse::<u16>().is_ok_and(|value| value <= 255)
        })
}

/// Whether every character of `text` is one `allowed` admits or part of a
/// well-formed percent-encoding, with the reason when not.
fn all_of(text: &str, allowed: impl Fn(char) -> bool) -> Result<(), String> {
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let digits: String = chars.by_ref().take(2).collect();
            if digits.len() != 2 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("has `%{digits}`, which is not `%` HEXDIG HEXDIG"));
            }
        } else if !allowed(c) {
            return Err(format!(
                "contains {c:?}, which RFC 3986 does not allow there"
            ));
        }
    }
    Ok(())
}

/// `unreserved = ALPHA / DIGIT / "-" / "." / "_" / "~"`.
const fn is_unreserved(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~')
}

/// `sub-delims = "!" / "$" / "&" / "'" / "(" / ")" / "*" / "+" / "," / ";" / "="`.
const fn is_sub_delim(c: char) -> bool {
    matches!(
        c,
        '!' | '$' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' | ';' | '='
    )
}

/// `pchar = unreserved / pct-encoded / sub-delims / ":" / "@"`, plus the `/`
/// that separates path segments.
const fn is_path_char(c: char) -> bool {
    is_unreserved(c) || is_sub_delim(c) || matches!(c, ':' | '@' | '/')
}

/// `query = fragment = *( pchar / "/" / "?" )`.
const fn is_query_char(c: char) -> bool {
    is_path_char(c) || c == '?'
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn accepts_the_uris_rfc_3986_and_the_specification_use() {
        for uri in [
            "https://mcp.example.com/ui/set_api_key",
            "https://mcp.example.com/connect?elicitationId=550e8400-e29b-41d4-a716-446655440000",
            "https://example.com/authorize?a=1&b=%2F#frag/ment?x",
            "HTTPS://user:pa%20ss@Example.COM:8443/a/b;c=d/@:x",
            "http://localhost:3000",
            "http://127.0.0.1/",
            "http://[::1]:8080/",
            "http://[2001:db8::7]/c=GB?objectClass?one",
            "http://[::ffff:192.0.2.128]/",
            "http://[1:2:3:4:5:6:7:8]/",
            "http://[1:2:3:4:5:6:1.2.3.4]/",
            "http://[v1.fe80::a+en1]/",
            "http://example.com:/",
            "mailto:John.Doe@example.com",
            "urn:oasis:names:specification:docbook:dtd:xml:4.1.2",
            "tel:+1-816-555-1212",
            "file:///etc/hosts",
            "news:comp.infosystems.www.servers.unix",
            "foo:",
            "a+b-c.d:/x",
        ] {
            assert_eq!(check(uri), Ok(()), "{uri}");
        }
    }

    #[test]
    fn refuses_what_rfc_3986_does_not_admit() {
        for (uri, reason) in [
            ("", "no scheme"),
            ("example.com/path", "no scheme"),
            ("/relative/reference", "no scheme"),
            (":no-scheme", "scheme \"\""),
            ("1http://example.com", "scheme \"1http\""),
            ("ht tp://example.com", "scheme \"ht tp\""),
            ("h_t://example.com", "scheme \"h_t\""),
            ("https://exa mple.com", "host that contains ' '"),
            ("https://example.com/a b", "path that contains ' '"),
            (
                "https://example.com/caf\u{e9}",
                "path that contains '\u{e9}'",
            ),
            ("https://ex\u{e4}mple.com/", "host that contains '\u{e4}'"),
            ("https://example.com/%zz", "`%zz`"),
            ("https://example.com/%4", "`%4`"),
            ("https://example.com/a?b=%", "query that has `%`"),
            ("https://example.com/#a#b", "fragment that contains '#'"),
            ("https://example.com/?a=<b>", "query that contains '<'"),
            ("https://example.com/a\\b", "path that contains '\\\\'"),
            ("https://a@b@example.com/", "host that contains '@'"),
            ("https://us[er@example.com/", "userinfo that contains '['"),
            ("https://example.com:80a/", "port \"80a\""),
            ("https://example.com:8:0/", "port \"8:0\""),
            ("https://[::1/", "no closing `]`"),
            ("https://[::1]x/", "after its IP literal"),
            ("https://[example.com]/", "IP literal [example.com]"),
            ("https:///path", "empty host"),
            ("https://", "empty host"),
            ("http:example.com", "no authority"),
            ("https:/path", "no authority"),
        ] {
            let Err(found) = check(uri) else {
                panic!("{uri} should be refused");
            };
            assert!(found.contains(reason), "{uri}: {found}");
        }
    }

    #[test]
    fn ipv6_literals_follow_the_grammar_exactly() {
        let literal = |inside: &str| check(&format!("http://[{inside}]/")).is_ok();
        for valid in [
            "::",
            "::1",
            "1::",
            "1:2:3:4:5:6:7::",
            "::2:3:4:5:6:7:8",
            "1:2:3:4:5:6:7:8",
            "fe80::1:2",
            "abcd:ABCD:0:00:000:0000::",
            "::1.2.3.4",
            "1:2:3:4:5:6:1.2.3.4",
            "1::255.255.255.255",
            "V7.x",
            "vF.a:b",
        ] {
            assert!(literal(valid), "{valid}");
        }
        for invalid in [
            "",
            ":",
            ":1",
            "1:",
            "1:2:3:4:5:6:7",
            "1:2:3:4:5:6:7:8:9",
            "1:2:3:4:5:6:7::8",
            "1::2::3",
            "12345::",
            "g::",
            "1.2.3.4::",
            "1:2:3:4:5:6:7:1.2.3.4",
            "::1.2.3",
            "::1.2.3.4.5",
            "::256.1.1.1",
            "::01.1.1.1",
            "::1.2.3.x",
            "::1..3.4",
            "::1234.1.1.1",
            "v.x",
            "vx.y",
            "v1.",
            "v1",
            "v1.a b",
            "w1.a",
        ] {
            assert!(!literal(invalid), "{invalid}");
        }
    }

    #[test]
    fn the_host_rule_binds_http_and_https_only() {
        // Generic syntax admits an empty authority; only the http schemes forbid it.
        assert_eq!(check("file:///x"), Ok(()));
        assert_eq!(check("foo://"), Ok(()));
        assert_eq!(check("foo:bar"), Ok(()));
        assert!(check("HTTP:///x").is_err());
        assert!(check("Https:x").is_err());
        assert_eq!(check("httpx:x"), Ok(()));
    }
}
