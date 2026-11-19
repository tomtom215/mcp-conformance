// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The request-metadata headers of the `2026-07-28` Streamable HTTP transport:
//! `Mcp-Name`, and the `Mcp-Param-*` headers a tool designates with
//! `x-mcp-header`.
//!
//! Written from the specification's "Request Metadata" section and from nothing
//! in the validator, on purpose: the validator judges what this client sends
//! (`TRAN-077` to `TRAN-093`), and two readings of the text that disagree show
//! up as a finding held against the client, where one shared implementation
//! would agree with itself.

use base64::Engine as _;
use serde_json::Value;

/// The largest integer a designated parameter may carry (2^53 - 1).
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// `value` as a header value: itself when it is plain ASCII a header can
/// carry unchanged, otherwise in the Base64 sentinel form.
pub(crate) fn encode(value: &str) -> String {
    let plain = value
        .bytes()
        .all(|byte| matches!(byte, 0x20..=0x7E | b'\t'));
    let padded = value.starts_with([' ', '\t']) || value.ends_with([' ', '\t']);
    let sentinel = value.starts_with("=?base64?") && value.ends_with("?=");
    if plain && !padded && !sentinel {
        value.to_owned()
    } else {
        format!(
            "=?base64?{}?=",
            base64::engine::general_purpose::STANDARD.encode(value)
        )
    }
}

/// One parameter a tool designates for mirroring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Designated {
    /// The `x-mcp-header` value: the header is `Mcp-Param-{name}`.
    pub(crate) name: String,
    /// The chain of `properties` keys leading to the parameter.
    pub(crate) path: Vec<String>,
}

/// The parameters `input_schema` designates.
///
/// # Errors
///
/// The reason the tool definition is invalid, in which case a client on this
/// transport must leave the tool out (`TRAN-083`, `TRAN-084`).
pub(crate) fn designated(input_schema: &Value) -> Result<Vec<Designated>, String> {
    let mut found = Vec::new();
    reachable(input_schema, &mut Vec::new(), &mut found)?;
    if annotations(input_schema, false) != found.len() {
        return Err(
            "an `x-mcp-header` annotation sits where no chain of `properties` keys reaches it"
                .to_owned(),
        );
    }
    for (index, one) in found.iter().enumerate() {
        if found[..index]
            .iter()
            .any(|other| other.name.eq_ignore_ascii_case(&one.name))
        {
            return Err(format!(
                "`x-mcp-header` {:?} is used more than once",
                one.name
            ));
        }
    }
    Ok(found)
}

/// Collects the annotations on properties reachable through `properties` alone.
fn reachable(
    schema: &Value,
    path: &mut Vec<String>,
    found: &mut Vec<Designated>,
) -> Result<(), String> {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Ok(());
    };
    for (key, property) in properties {
        path.push(key.clone());
        if let Some(annotation) = property.get("x-mcp-header") {
            let name = annotation
                .as_str()
                .filter(|name| !name.is_empty() && name.bytes().all(is_tchar))
                .ok_or_else(|| {
                    format!("`x-mcp-header` on `{key}` is not an HTTP field-name token")
                })?;
            if !matches!(
                property.get("type").and_then(Value::as_str),
                Some("string" | "integer" | "boolean")
            ) {
                return Err(format!(
                    "`x-mcp-header` on `{key}`, which is not a string, integer or boolean"
                ));
            }
            found.push(Designated {
                name: name.to_owned(),
                path: path.clone(),
            });
        }
        reachable(property, path, found)?;
        path.pop();
    }
    Ok(())
}

/// RFC 9110's `tchar`.
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

/// How many `x-mcp-header` keywords `schema` holds anywhere. The members of a
/// `properties` object are property names, not keywords, so a parameter that is
/// itself called `x-mcp-header` is not counted.
fn annotations(schema: &Value, names: bool) -> usize {
    match schema {
        Value::Object(members) => members
            .iter()
            .map(|(key, member)| {
                let here = usize::from(!names && key == "x-mcp-header");
                here + annotations(member, !names && key == "properties")
            })
            .sum(),
        Value::Array(items) => items.iter().map(|item| annotations(item, false)).sum(),
        _ => 0,
    }
}

/// The `Mcp-Param-*` headers for a call with `arguments`: one per designated
/// parameter that has a value there.
///
/// # Errors
///
/// A designated integer is outside the range the transport allows (`TRAN-081`).
pub(crate) fn mirrored(
    designated: &[Designated],
    arguments: &Value,
) -> Result<Vec<(String, String)>, String> {
    let mut headers = Vec::new();
    for one in designated {
        let present = one
            .path
            .iter()
            .try_fold(arguments, |at, key| at.as_object()?.get(key));
        let text = match present {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Bool(flag)) => flag.to_string(),
            Some(Value::Number(number)) => match number.as_i64() {
                Some(integer) if integer.abs() <= MAX_SAFE_INTEGER => integer.to_string(),
                _ => {
                    return Err(format!(
                        "argument `{}` is not an integer a header may carry",
                        one.path.join(".")
                    ));
                }
            },
            _ => continue,
        };
        headers.push((format!("Mcp-Param-{}", one.name), encode(&text)));
    }
    Ok(headers)
}

#[cfg(test)]
mod tests;
