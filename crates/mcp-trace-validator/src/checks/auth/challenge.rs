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

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.at += 1;
        }
    }

    fn take_while(&mut self, accept: impl Fn(u8) -> bool) -> String {
        let start = self.at;
        while self.peek().is_some_and(&accept) {
            self.at += 1;
        }
        String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned()
    }

    /// A quoted-string's content, unescaped; the cursor sits on its `"`.
    fn quoted(&mut self) -> String {
        self.at += 1;
        let mut out = Vec::new();
        while let Some(byte) = self.peek() {
            self.at += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    if let Some(escaped) = self.peek() {
                        out.push(escaped);
                        self.at += 1;
                    }
                }
                other => out.push(other),
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Consumes a token68 after a scheme when one is there: its characters,
    /// any `=` padding, and then the end of the list element.
    fn token68(&mut self) {
        let start = self.at;
        let body = self.take_while(is_token68_char);
        while self.peek() == Some(b'=') {
            self.at += 1;
        }
        self.skip_whitespace();
        // `error="x"` and `scope=a` are parameters, not a token68: a token68,
        // padded or not, ends its list element.
        if body.is_empty() || !matches!(self.peek(), None | Some(b',')) {
            self.at = start;
        }
    }
}

/// Every challenge in a `WWW-Authenticate` value, in order.
pub(super) fn parse(value: &str) -> Vec<Challenge> {
    let mut cursor = Cursor {
        bytes: value.as_bytes(),
        at: 0,
    };
    let mut challenges: Vec<Challenge> = Vec::new();
    loop {
        while matches!(cursor.peek(), Some(b' ' | b'\t' | b',')) {
            cursor.at += 1;
        }
        if cursor.peek().is_none() {
            return challenges;
        }
        let token = cursor.take_while(is_tchar);
        if token.is_empty() {
            // Not a token: skip to the next list element.
            cursor.at += 1;
            while cursor.peek().is_some_and(|byte| byte != b',') {
                cursor.at += 1;
            }
            continue;
        }
        cursor.skip_whitespace();
        if cursor.peek() == Some(b'=')
            && let Some(current) = challenges.last_mut()
        {
            cursor.at += 1;
            cursor.skip_whitespace();
            let parameter = if cursor.peek() == Some(b'"') {
                cursor.quoted()
            } else {
                cursor.take_while(|byte| byte != b',' && byte != b' ' && byte != b'\t')
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
