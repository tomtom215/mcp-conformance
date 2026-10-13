// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Text into HTML, inert.
//!
//! Everything a report shows came from somewhere the page's reader does not
//! control: a method name, an id, a header, a finding quoting either, and —
//! from a custom registry — even a clause's quote and link. All of it goes
//! through [`Text`], which escapes every character HTML gives meaning to in
//! element content *and* in a quoted attribute, so one function serves both
//! and no call site has to know which it is in. Links go through [`href`],
//! which admits only `http` and `https`: a `javascript:` URL is text, never a
//! link.

use core::fmt::{self, Write as _};

/// `text`, escaped for HTML element content or a double- or single-quoted
/// attribute value.
///
/// Beyond the five markup characters, two kinds of character are shown as
/// visible `\u{…}` escapes rather than passed through: control characters
/// other than newline and tab (a recording is untrusted, and these render as
/// nothing, or as something else), and the Unicode bidirectional controls,
/// which reorder the text around them — `U+202E` can make a method name read
/// backwards, the trick behind CVE-2021-42574.
#[derive(Debug, Clone, Copy)]
pub(super) struct Text<'a>(pub(super) &'a str);

impl fmt::Display for Text<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for character in self.0.chars() {
            match character {
                '&' => f.write_str("&amp;")?,
                '<' => f.write_str("&lt;")?,
                '>' => f.write_str("&gt;")?,
                '"' => f.write_str("&quot;")?,
                '\'' => f.write_str("&#39;")?,
                '\n' | '\t' => f.write_char(character)?,
                _ if shown_escaped(character) => {
                    write!(f, "{}", character.escape_unicode())?;
                }
                _ => f.write_char(character)?,
            }
        }
        Ok(())
    }
}

/// Whether `character` is shown as its escape instead of itself.
fn shown_escaped(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
}

/// `url` when it is safe to put in an `href`: an absolute `http` or `https`
/// URL. Anything else — `javascript:`, `data:`, a relative path that would
/// resolve against the report file's own location — is `None`, and the caller
/// shows it as text.
pub(super) fn href(url: &str) -> Option<&str> {
    let starts = |scheme: &str| {
        url.as_bytes()
            .get(..scheme.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(scheme.as_bytes()))
    };
    (starts("https://") || starts("http://")).then_some(url)
}

/// `text` as an anchor name: ASCII letters, digits, `-` and `_` kept, every
/// other character replaced by `_`. Requirement IDs (`BASE-003`) and
/// revisions (`2025-11-25`) pass through unchanged, so a report's anchors are
/// the names a reader would type (`report.html#BASE-003`).
pub(super) fn anchor(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_characters_are_escaped() {
        assert_eq!(
            Text(r#"<script>alert("x")</script> & 'y'"#).to_string(),
            "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &amp; &#39;y&#39;"
        );
        assert_eq!(
            Text("plain text: ünïcode").to_string(),
            "plain text: ünïcode"
        );
    }

    #[test]
    fn newlines_and_tabs_survive_and_other_controls_are_shown() {
        assert_eq!(Text("a\nb\tc").to_string(), "a\nb\tc");
        assert_eq!(Text("a\u{1b}[2Jb").to_string(), "a\\u{1b}[2Jb");
        assert_eq!(
            Text("\u{0}\u{7f}\u{85}").to_string(),
            "\\u{0}\\u{7f}\\u{85}"
        );
    }

    #[test]
    fn bidirectional_controls_are_shown_not_obeyed() {
        for (control, shown) in [
            ('\u{200e}', "\\u{200e}"),
            ('\u{200f}', "\\u{200f}"),
            ('\u{202a}', "\\u{202a}"),
            ('\u{202e}', "\\u{202e}"),
            ('\u{2066}', "\\u{2066}"),
            ('\u{2069}', "\\u{2069}"),
        ] {
            assert_eq!(
                Text(&format!("a{control}b")).to_string(),
                format!("a{shown}b")
            );
        }
        // Their neighbours are ordinary text.
        for ordinary in ['\u{200d}', '\u{2029}', '\u{202f}', '\u{2065}', '\u{206a}'] {
            let text = format!("a{ordinary}b");
            assert_eq!(Text(&text).to_string(), text, "{ordinary:?}");
        }
    }

    #[test]
    fn only_absolute_http_urls_become_links() {
        for url in [
            "https://modelcontextprotocol.io/specification/2025-11-25/basic",
            "HTTPS://EXAMPLE.COM/",
            "http://example.com",
            // A multi-byte character straddling the scheme's length.
            "http://é.example/",
        ] {
            assert_eq!(href(url), Some(url));
        }
        for url in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "//example.com/",
            "basic/lifecycle",
            "https:",
            "http:/",
            "",
            " https://example.com",
        ] {
            assert_eq!(href(url), None, "{url:?}");
        }
    }

    #[test]
    fn anchors_keep_ids_and_revisions_and_replace_the_rest() {
        assert_eq!(anchor("BASE-003"), "BASE-003");
        assert_eq!(anchor("2025-11-25"), "2025-11-25");
        assert_eq!(anchor("a_b"), "a_b");
        assert_eq!(anchor("x\"><y z/é"), "x___y_z__");
    }
}
