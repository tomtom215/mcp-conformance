// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The requests that change the library. Each is a `POST` with a JSON body
//! (`Content-Type: application/json`, which no HTML form can send), behind
//! the same-origin check every state-changing request passes.
//!
//! | Request | Body | Effect |
//! |---------|------|--------|
//! | `POST /api/import` | `{name, format?, transport?, text}` | adds the recording, converted, as a new entry |
//! | `POST /api/sessions/<id>/tags` | `{tags: [...]}` | replaces its tags |
//! | `POST /api/sessions/<id>/redact` | `{headers?, pointers?, meta?, strings?}` | adds a redacted copy as a new entry |
//! | `POST /api/sessions/<id>/delete` | `{}` | deletes the trace and its sidecar |
//! | `POST /api/retention` | `{days, apply}` | lists — or, with `apply`, deletes — entries recorded more than `days` days ago |

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{StatusCode, header, request::Parts};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Shared, blocking, checked_id, failure, json_response};
use crate::workbench::clock;
use crate::workbench::ops::{import_document, redact_document, retention_select};
use crate::workbench::redact::{self, Rules};
use crate::workbench::state::{Failure, Workbench};

/// The largest body accepted: a recording at the toolkit's own message limit,
/// with room for the JSON escaping of it.
const BODY_LIMIT: usize = 192 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportBody {
    name: String,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    transport: Option<String>,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TagsBody {
    tags: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RedactBody {
    #[serde(default)]
    headers: Option<Vec<String>>,
    #[serde(default)]
    pointers: Vec<String>,
    #[serde(default)]
    meta: bool,
    #[serde(default)]
    strings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionBody {
    days: u32,
    #[serde(default)]
    apply: bool,
}

/// Dispatches a `POST`.
pub(super) async fn post(
    shared: Arc<Shared>,
    parts: &Parts,
    path: &[&str],
    body: Body,
) -> Response {
    let json = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|media| media.trim() == "application/json")
        });
    if !json {
        return json_response(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            &json!({"error": "a change is a JSON body (Content-Type: application/json)"}),
        );
    }
    let Ok(bytes) = to_bytes(body, BODY_LIMIT).await else {
        return json_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            &json!({"error": "the body is too large"}),
        );
    };
    let path: Vec<String> = path.iter().map(|segment| (*segment).to_owned()).collect();
    let answer = blocking(shared, move |wb| {
        let segments: Vec<&str> = path.iter().map(String::as_str).collect();
        match segments.as_slice() {
            ["api", "import"] => import_entry(wb, &parse(&bytes)?),
            ["api", "sessions", id, "tags"] => {
                let body: TagsBody = parse(&bytes)?;
                let id = checked_id(id)?;
                wb.set_tags(&id, &body.tags)?;
                Ok(wb.entry(&id)?.summary())
            }
            ["api", "sessions", id, "redact"] => redact_entry(wb, &checked_id(id)?, parse(&bytes)?),
            ["api", "sessions", id, "delete"] => {
                let id = checked_id(id)?;
                wb.delete(&id)?;
                Ok(json!({ "deleted": [id] }))
            }
            ["api", "retention"] => retention(wb, &parse(&bytes)?, &clock::now()),
            _ => Err(Failure::NotFound("no such API".to_owned())),
        }
    })
    .await;
    match answer {
        Ok(value) => json_response(StatusCode::OK, &value),
        Err(error) => failure(&error),
    }
}

fn parse<T: for<'a> Deserialize<'a>>(bytes: &[u8]) -> Result<T, Failure> {
    serde_json::from_slice(bytes)
        .map_err(|error| Failure::Invalid(format!("the request body: {error}")))
}

fn import_entry(wb: &mut Workbench, body: &ImportBody) -> Result<Value, Failure> {
    import_document(
        wb,
        &body.name,
        &body.text,
        body.format.as_deref(),
        body.transport.as_deref(),
    )
}

fn redact_entry(wb: &mut Workbench, id: &str, body: RedactBody) -> Result<Value, Failure> {
    let rules = Rules {
        headers: body.headers.unwrap_or_else(|| {
            redact::DEFAULT_HEADERS
                .iter()
                .map(|h| (*h).to_owned())
                .collect()
        }),
        pointers: body.pointers,
        meta: body.meta,
        strings: body.strings,
    };
    redact_document(wb, id, &rules)
}

fn retention(wb: &mut Workbench, body: &RetentionBody, now: &str) -> Result<Value, Failure> {
    let selected = retention_select(wb, body.days, now);
    if body.apply {
        for id in &selected {
            wb.delete(id)?;
        }
    }
    Ok(json!({ "days": body.days, "applied": body.apply, "entries": selected }))
}

#[cfg(test)]
mod tests;
