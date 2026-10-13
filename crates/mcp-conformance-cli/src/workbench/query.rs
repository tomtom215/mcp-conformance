// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The search box's query language.
//!
//! A query is a list of terms separated by spaces; a session matches when it
//! matches every term.
//!
//! | Term | Matches a session that… |
//! |------|-------------------------|
//! | `method:tools/call` | sent or received a message with that method |
//! | `tool:echo` | called, or listed, the tool `echo` |
//! | `resource:test://static-text` | read, subscribed to, or listed that resource URI |
//! | `prompt:greeting` | got, or listed, that prompt |
//! | `session:3f2a…` | carried that `Mcp-Session-Id`, or is the library entry of that id |
//! | `code:-32602` | holds a JSON-RPC error response with that code |
//! | `req:BASE-003` | has a finding against that requirement |
//! | `server:everything` | was recorded against a server of that name |
//! | `tag:nightly` | carries that tag |
//! | `verdict:fail` | has that verdict (`pass`, `warn`, `fail`, `unsupported`, `unjudged`) |
//! | `timeout`, `"two words"` | contains the text in a finding, a name, a tag or any value above |
//!
//! Field values compare without regard to ASCII case. A value ending in `*`
//! matches by prefix (`method:resources/*`, `req:TRAN-*`). A term starting
//! with `-` excludes the sessions it matches (`-verdict:pass`). A value with
//! spaces is quoted (`tool:"my tool"`). A word before a colon that is not a
//! field name (`https://…`) is searched as text, and the response says so.

use core::fmt;

/// What a term looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Field {
    Method,
    Tool,
    Resource,
    Prompt,
    Session,
    Code,
    Req,
    Server,
    Tag,
    Verdict,
    /// Free text.
    Text,
}

impl Field {
    /// The field a `name:` prefix names.
    fn named(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "method" => Self::Method,
            "tool" => Self::Tool,
            "resource" | "uri" => Self::Resource,
            "prompt" => Self::Prompt,
            "session" => Self::Session,
            "code" => Self::Code,
            "req" | "requirement" => Self::Req,
            "server" => Self::Server,
            "tag" => Self::Tag,
            "verdict" => Self::Verdict,
            _ => return None,
        })
    }

    /// The field's name in a query.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Method => "method",
            Self::Tool => "tool",
            Self::Resource => "resource",
            Self::Prompt => "prompt",
            Self::Session => "session",
            Self::Code => "code",
            Self::Req => "req",
            Self::Server => "server",
            Self::Tag => "tag",
            Self::Verdict => "verdict",
            Self::Text => "text",
        }
    }
}

/// One term of a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Term {
    pub(crate) field: Field,
    /// The value, lowercased, without a trailing `*`.
    pub(crate) value: String,
    /// Whether the value ended in `*`.
    pub(crate) prefix: bool,
    /// Whether the term excludes what it matches.
    pub(crate) negated: bool,
}

impl Term {
    /// Whether `candidate` (already lowercased) is what this term asks for.
    pub(crate) fn accepts(&self, candidate: &str) -> bool {
        if self.field == Field::Text {
            candidate.contains(&self.value)
        } else if self.prefix {
            candidate.starts_with(&self.value)
        } else {
            candidate == self.value
        }
    }
}

/// A parsed query, and what the parser wants the reader to know.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Query {
    pub(crate) terms: Vec<Term>,
    pub(crate) notes: Vec<String>,
}

/// A query that cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueryError(pub(crate) String);

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Splits `text` into words, keeping a quoted run (`"a b"`, also after a
/// `field:`) as part of one word.
fn words(text: &str) -> Result<Vec<String>, QueryError> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut started = false;
    for character in text.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    words.push(core::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if quoted {
        return Err(QueryError("a quotation mark is not closed".to_owned()));
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

/// Reads a query (see the module documentation).
pub(crate) fn parse(text: &str) -> Result<Query, QueryError> {
    let mut query = Query::default();
    for word in words(text)? {
        let (negated, rest) = match word.strip_prefix('-') {
            Some(rest) if !rest.is_empty() => (true, rest),
            _ => (false, word.as_str()),
        };
        let named = rest
            .split_once(':')
            .map(|(name, value)| (name, Field::named(name), value));
        let (field, value) = match named {
            Some((_, Some(field), value)) => (field, value),
            Some((name, None, _)) => {
                query.notes.push(format!(
                    "{name}: is not a field, so {rest} is searched as text"
                ));
                (Field::Text, rest)
            }
            None => (Field::Text, rest),
        };
        let (value, prefix) = match value.strip_suffix('*') {
            Some(stem) if field != Field::Text => (stem, true),
            _ => (value, false),
        };
        if value.is_empty() && !prefix {
            return Err(QueryError(format!(
                "{}: needs a value, e.g. {}",
                field.name(),
                example(field)
            )));
        }
        let mut value = value.to_lowercase();
        if field == Field::Verdict && value == "warn" {
            "pass-with-warnings".clone_into(&mut value);
        }
        query.terms.push(Term {
            field,
            value,
            prefix,
            negated,
        });
    }
    Ok(query)
}

const fn example(field: Field) -> &'static str {
    match field {
        Field::Method => "method:tools/call",
        Field::Tool => "tool:echo",
        Field::Resource => "resource:test://static-text",
        Field::Prompt => "prompt:greeting",
        Field::Session => "session:3f2a",
        Field::Code => "code:-32602",
        Field::Req => "req:BASE-003",
        Field::Server => "server:everything",
        Field::Tag => "tag:nightly",
        Field::Verdict => "verdict:fail",
        Field::Text => "timeout",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn term(field: Field, value: &str) -> Term {
        Term {
            field,
            value: value.to_owned(),
            prefix: false,
            negated: false,
        }
    }

    #[test]
    fn the_example_query_reads_as_three_fields_and_a_word() {
        let query = parse("method:tools/call req:BASE-003 code:-32602 timeout").unwrap();
        assert_eq!(
            query.terms,
            [
                term(Field::Method, "tools/call"),
                term(Field::Req, "base-003"),
                term(Field::Code, "-32602"),
                term(Field::Text, "timeout"),
            ]
        );
        assert_eq!(query.notes.len(), 0);
    }

    #[test]
    fn every_field_has_a_name_and_aliases_read_the_same() {
        for (text, field) in [
            ("method:m", Field::Method),
            ("TOOL:m", Field::Tool),
            ("resource:m", Field::Resource),
            ("uri:m", Field::Resource),
            ("prompt:m", Field::Prompt),
            ("session:m", Field::Session),
            ("code:m", Field::Code),
            ("req:m", Field::Req),
            ("requirement:m", Field::Req),
            ("server:m", Field::Server),
            ("tag:m", Field::Tag),
            ("verdict:m", Field::Verdict),
        ] {
            let query = parse(text).unwrap();
            assert_eq!(query.terms, [term(field, "m")], "{text}");
            assert_eq!(Field::named(field.name()), Some(field));
        }
        assert_eq!(Field::named("text"), None);
    }

    #[test]
    fn quotes_negation_and_prefixes() {
        let query = parse(r#"tool:"my tool"  -verdict:pass req:TRAN-* "two words" -"#).unwrap();
        assert_eq!(query.terms[0], term(Field::Tool, "my tool"));
        assert!(query.terms[1].negated);
        assert_eq!(query.terms[1].value, "pass");
        assert!(query.terms[2].prefix);
        assert_eq!(query.terms[2].value, "tran-");
        assert_eq!(query.terms[3], term(Field::Text, "two words"));
        // A lone `-` is a word, not a negation.
        assert_eq!(query.terms[4], term(Field::Text, "-"));
        // A `*` in free text is a character.
        assert_eq!(parse("a*").unwrap().terms[0], term(Field::Text, "a*"));
        // `method:*` matches every method.
        let any = parse("method:*").unwrap();
        assert!(any.terms[0].prefix);
        assert_eq!(any.terms[0].value, "");
        assert_eq!(
            parse("verdict:warn").unwrap().terms[0].value,
            "pass-with-warnings"
        );
    }

    #[test]
    fn an_unknown_field_is_text_and_said_so() {
        let query = parse("https://example.com/x").unwrap();
        assert_eq!(query.terms, [term(Field::Text, "https://example.com/x")]);
        assert_eq!(query.notes.len(), 1);
        assert!(query.notes[0].starts_with("https: is not a field"));
    }

    #[test]
    fn malformed_queries_are_refused_with_a_reason() {
        assert!(parse(r#"tool:"open"#).unwrap_err().0.contains("not closed"));
        let error = parse("req:").unwrap_err();
        assert!(error.to_string().contains("req:BASE-003"), "{error}");
        assert!(parse(r#""""#).unwrap_err().0.contains("needs a value"));
        assert_eq!(parse("   ").unwrap(), Query::default());
        for field in [
            Field::Method,
            Field::Tool,
            Field::Resource,
            Field::Prompt,
            Field::Session,
            Field::Code,
            Field::Server,
            Field::Tag,
            Field::Verdict,
            Field::Text,
        ] {
            assert_ne!(example(field).len(), 0);
        }
    }

    #[test]
    fn terms_accept_by_their_kind() {
        let exact = term(Field::Method, "tools/call");
        assert!(exact.accepts("tools/call"));
        assert!(!exact.accepts("tools/call2"));
        let prefix = parse("method:tools/*").unwrap().terms.remove(0);
        assert!(prefix.accepts("tools/list") && !prefix.accepts("prompts/list"));
        let text = term(Field::Text, "time");
        assert!(text.accepts("a timeout") && !text.accepts("tim"));
    }
}
