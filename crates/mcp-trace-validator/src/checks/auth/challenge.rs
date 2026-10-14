// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Reading a `WWW-Authenticate` field value as challenges (RFC 9110 §11.6.1).
//!
//! The grammar is `#challenge`, `challenge = auth-scheme [ 1*SP ( token68 /
//! #auth-param ) ]`, `auth-param = token BWS "=" BWS ( token / quoted-string )`.
//! Challenges and their parameters share one comma-separated list, so a token
//! after a comma starts a new challenge unless an `=` follows it. A recorder
//! joins repeated field lines with `, ` (RFC 9110 §5.3), which is the same list.
//!
//! Lenient by design: the clauses judged here read a Bearer challenge's
//! parameters, and a malformed stretch is skipped rather than refused, so a
//! value this reader cannot follow can make a clause *not observed* but never
//! invents a parameter. Parameter names compare case-insensitively (RFC 9110
//! §11.2); values are kept as sent.

/// One challenge: its scheme and parameters, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Challenge {
    /// The auth-scheme as sent.
    pub scheme: String,
    /// `(lowercased name, value)` for each auth-param, quoted values unescaped.
    pub params: Vec<(String, String)>,
}

impl Challenge {
    /// Whether this is a `Bearer` challenge (schemes are case-insensitive).
    pub(super) fn is_bearer(&self) -> bool {
        self.scheme.eq_ignore_ascii_case("bearer")
    }

    /// The first value of the parameter `name` (lowercase).
    pub(super) fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// RFC 9110 `tchar`.
const fn is_tchar(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// RFC 9110 `token68` characters, before its `=` padding.
const fn is_token68_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'+' | b'/')
}

/// The unread rest of a field value. It only ever shrinks by re-slicing, with
/// no index arithmetic, and every loop below walks a slice or an iterator, so
/// no value — however malformed — can keep a reading from ending.
struct Cursor<'a> {
    rest: &'a [u8],
}

impl<'a> Cursor<'a> {
    /// Consumes the longest prefix of bytes that `accept` takes, and returns it.
    fn take_while(&mut self, accept: impl Fn(u8) -> bool) -> &'a [u8] {
        let len = self
            .rest
            .iter()
            .position(|&byte| !accept(byte))
            .unwrap_or(self.rest.len());
        let (taken, rest) = self.rest.split_at(len);
        self.rest = rest;
        taken
    }

    fn skip_whitespace(&mut self) {
        self.take_while(|byte| matches!(byte, b' ' | b'\t'));
    }

    /// A quoted-string's content, unescaped; the cursor sits just past its
    /// opening `"`. An unterminated string runs to the end of the value.
    fn quoted(&mut self) -> String {
        let mut out = Vec::new();
        let mut bytes = self.rest.iter();
        while let Some(&byte) = bytes.next() {
            match byte {
                b'"' => break,
                b'\\' => out.extend(bytes.next()),
                other => out.push(other),
            }
        }
        self.rest = bytes.as_slice();
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Consumes a token68 after a scheme when one is there: its characters,
    /// any `=` padding, and then the end of the list element.
    fn token68(&mut self) {
        let start = self.rest;
        let body = self.take_while(is_token68_char);
        self.take_while(|byte| byte == b'=');
        self.skip_whitespace();
        // `error="x"` and `scope=a` are parameters, not a token68: a token68,
        // padded or not, ends its list element.
        if body.is_empty() || !matches!(self.rest, [] | [b',', ..]) {
            self.rest = start;
        }
    }
}

/// Every challenge in a `WWW-Authenticate` value, in order.
pub(super) fn parse(value: &str) -> Vec<Challenge> {
    let mut cursor = Cursor {
        rest: value.as_bytes(),
    };
    let mut challenges: Vec<Challenge> = Vec::new();
    loop {
        cursor.take_while(|byte| matches!(byte, b' ' | b'\t' | b','));
        if cursor.rest.is_empty() {
            return challenges;
        }
        let token = cursor.take_while(is_tchar);
        if token.is_empty() {
            // Not a token: skip to the next list element. Dropping the stray
            // byte first (it is no separator — those were just skipped) makes
            // every pass through this loop consume at least one byte.
            cursor.rest = &cursor.rest[1..];
            cursor.take_while(|byte| byte != b',');
            continue;
        }
        let token = String::from_utf8_lossy(token).into_owned();
        cursor.skip_whitespace();
        if let [b'=', after @ ..] = cursor.rest
            && let Some(current) = challenges.last_mut()
        {
            cursor.rest = after;
            cursor.skip_whitespace();
            let parameter = if let [b'"', after @ ..] = cursor.rest {
                cursor.rest = after;
                cursor.quoted()
            } else {
                let value = cursor.take_while(|byte| byte != b',' && byte != b' ' && byte != b'\t');
                String::from_utf8_lossy(value).into_owned()
            };
            current.params.push((token.to_ascii_lowercase(), parameter));
            continue;
        }
        challenges.push(Challenge {
            scheme: token,
            params: Vec::new(),
        });
        cursor.token68();
    }
}
