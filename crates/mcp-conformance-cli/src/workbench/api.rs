// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The workbench's HTTP surface: the page, its assets, and the JSON API the
//! page reads (`/api/...`). Every request passes [`guard`]
//! first; every response leaves through [`guard::harden`].
//!
//! | Request | Answer |
//! |---------|--------|
//! | `GET /` | the page (with `?token=` once: the launch exchange) |
//! | `GET /assets/<file>` | the page's scripts and stylesheets |
//! | `GET /api/library` | every entry's summary, and the files skipped |
//! | `GET /api/search?q=` | the entries a query matches, best first |
//! | `GET /api/sessions/<id>?from=` | one session: findings and a page of events (polled to follow a growing trace) |
//! | `GET /api/sessions/<id>/report.html` | the validator's HTML report, as a download |
//! | `GET /api/diff?a=&b=` | two sessions compared |
//! | `GET /api/stats` | the dashboards' numbers — `library stats --format json` |
//! | `GET /api/export?ids=` | a tar of entries and their sidecars |
//! | `POST /api/import`, `/api/sessions/<id>/tags`, `/redact`, `/delete`, `/api/retention` | [`admin`] |
//!
//! Nothing here makes an outbound request; the server only answers.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use axum::body::Body;
use axum::http::{HeaderValue, Method, Request, StatusCode, header, request::Parts};
use axum::response::Response;
use serde_json::{Value, json};

use super::assets;
use super::guard::{self, Keys, plain};
use super::query;
use super::search;
use super::state::{Failure, Workbench};
use super::stats;
use super::view;
use super::{diff, library};

mod admin;

/// What every request shares.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) keys: Keys,
    pub(crate) workbench: Mutex<Workbench>,
}

impl Shared {
    /// How many entries the library holds.
    pub(crate) fn lock_count(&self) -> usize {
        self.lock().entries.len()
    }

    fn lock(&self) -> MutexGuard<'_, Workbench> {
        self.workbench
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Answers one request.
pub(crate) async fn handle(shared: Arc<Shared>, request: Request<Body>) -> Response {
    let (parts, body) = request.into_parts();
    let report = parts.uri.path().ends_with("/report.html");
    let mut response = route(shared, &parts, body).await;
    let policy = if report && response.status() == StatusCode::OK {
        guard::REPORT_POLICY
    } else {
        guard::CONTENT_SECURITY_POLICY
    };
    guard::harden(&mut response, policy);
    response
}

async fn route(shared: Arc<Shared>, parts: &Parts, body: Body) -> Response {
    if let Err((status, why)) = shared.keys.check_origin(&parts.method, &parts.headers) {
        return plain(status, why);
    }
    let authenticated = shared.keys.authenticated(&parts.headers);
    if parts.uri.path() == "/" && guard::query(&parts.uri, "token").is_some() && authenticated {
        // The browser that launched, reloading the link: it already holds
        // the cookie, so it is sent on rather than refused.
        let mut response = plain(StatusCode::SEE_OTHER, "");
        response
            .headers_mut()
            .insert(header::LOCATION, HeaderValue::from_static("/"));
        return response;
    }
    if parts.uri.path() == "/"
        && let Some(launch) = shared.keys.launch(&parts.uri)
    {
        return match launch {
            Ok(cookie) => {
                let mut response = plain(StatusCode::SEE_OTHER, "");
                let headers = response.headers_mut();
                headers.insert(header::LOCATION, HeaderValue::from_static("/"));
                if let Ok(value) = HeaderValue::from_str(&cookie) {
                    headers.insert(header::SET_COOKIE, value);
                }
                response
            }
            Err((status, why)) => plain(status, why),
        };
    }
    if !authenticated {
        return plain(
            StatusCode::UNAUTHORIZED,
            "unauthorized: open the link `mcp-conformance serve` printed when it started\n",
        );
    }
    let path: Vec<&str> = parts
        .uri
        .path()
        .trim_start_matches('/')
        .split('/')
        .collect();
    match (&parts.method, path.as_slice()) {
        (&Method::GET | &Method::HEAD, [""]) => assets::page(),
        (&Method::GET | &Method::HEAD, ["assets", name]) => assets::asset(name),
        (&Method::POST, _) => admin::post(shared, parts, &path, body).await,
        (&Method::GET | &Method::HEAD, ["api", rest @ ..]) => read(shared, parts, rest).await,
        (&Method::GET | &Method::HEAD, _) => plain(StatusCode::NOT_FOUND, "not found\n"),
        _ => plain(StatusCode::METHOD_NOT_ALLOWED, "method not allowed\n"),
    }
}

/// A query parameter, percent-decoded (`+` is a space).
pub(crate) fn param(parts: &Parts, name: &str) -> Option<String> {
    guard::query(&parts.uri, name).map(decode)
}

/// Percent-decodes a query component; an invalid escape is kept as written.
pub(crate) fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = |offset: usize| {
            bytes
                .get(at + offset)
                .and_then(|byte| char::from(*byte).to_digit(16))
        };
        match (bytes[at], hex(1), hex(2)) {
            (b'+', ..) => out.push(b' '),
            (b'%', Some(high), Some(low)) => {
                out.extend(u8::try_from(high * 16 + low));
                at += 2;
            }
            (byte, ..) => out.push(byte),
        }
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A JSON response.
pub(crate) fn json_response(status: StatusCode, value: &Value) -> Response {
    let mut response = Response::new(Body::from(value.to_string()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

/// A failure as JSON: `{"error": …}` with its status.
pub(crate) fn failure(failure: &Failure) -> Response {
    let (status, why) = match failure {
        Failure::NotFound(why) => (StatusCode::NOT_FOUND, why),
        Failure::Invalid(why) => (StatusCode::BAD_REQUEST, why),
        Failure::Io(why) => (StatusCode::INTERNAL_SERVER_ERROR, why),
    };
    json_response(status, &json!({ "error": why }))
}

/// Runs `work` on the library off the async threads, after bringing the
/// library in step with its directory.
pub(crate) async fn blocking<T: Send + 'static>(
    shared: Arc<Shared>,
    work: impl FnOnce(&mut Workbench) -> Result<T, Failure> + Send + 'static,
) -> Result<T, Failure> {
    tokio::task::spawn_blocking(move || {
        let mut workbench = shared.lock();
        workbench.refresh()?;
        let answer = work(&mut workbench);
        drop(workbench);
        answer
    })
    .await
    .map_err(|error| Failure::Io(format!("the request's work stopped: {error}")))?
}

fn checked_id(id: &str) -> Result<String, Failure> {
    if library::valid_id(id) {
        Ok(id.to_owned())
    } else {
        Err(Failure::Invalid(format!("not an entry id: {id:?}")))
    }
}

async fn read(shared: Arc<Shared>, parts: &Parts, path: &[&str]) -> Response {
    let answer = match path {
        ["library"] => blocking(shared, |wb| Ok(library_listing(wb))).await,
        ["search"] => {
            let text = param(parts, "q").unwrap_or_default();
            blocking(shared, move |wb| Ok(search_listing(wb, &text))).await
        }
        ["stats"] => {
            return match blocking(shared, |wb| {
                Ok(stats::to_json(&stats::compute(wb.entries.values())))
            })
            .await
            {
                Ok(text) => {
                    let mut response = json_response(StatusCode::OK, &Value::Null);
                    *response.body_mut() = Body::from(text);
                    response
                }
                Err(error) => failure(&error),
            };
        }
        ["sessions", id] => {
            let from = param(parts, "from")
                .and_then(|from| from.parse().ok())
                .unwrap_or(0);
            let id = (*id).to_owned();
            blocking(shared, move |wb| {
                let id = checked_id(&id)?;
                let events = wb.events(&id)?;
                Ok(view::session(wb.entry(&id)?, &events, from))
            })
            .await
        }
        ["sessions", id, "report.html"] => return report(shared, id).await,
        ["diff"] => {
            let (a, b) = (
                param(parts, "a").unwrap_or_default(),
                param(parts, "b").unwrap_or_default(),
            );
            blocking(shared, move |wb| {
                let (a, b) = (checked_id(&a)?, checked_id(&b)?);
                let (left, right) = (wb.events(&a)?, wb.events(&b)?);
                let compared = diff::diff((wb.entry(&a)?, &left), (wb.entry(&b)?, &right));
                serde_json::to_value(compared).map_err(|error| Failure::Io(error.to_string()))
            })
            .await
        }
        ["export"] => return export(shared, param(parts, "ids").unwrap_or_default()).await,
        _ => Err(Failure::NotFound("no such API".to_owned())),
    };
    match answer {
        Ok(value) => json_response(StatusCode::OK, &value),
        Err(error) => failure(&error),
    }
}

fn library_listing(workbench: &Workbench) -> Value {
    json!({
        "library": workbench.library.root().display().to_string(),
        "tool_version": super::meta::tool_version(),
        "sessions": workbench.entries.values().map(super::entry::Entry::summary).collect::<Vec<_>>(),
        "skipped": workbench.skipped,
    })
}

fn search_listing(workbench: &Workbench, text: &str) -> Value {
    match query::parse(text) {
        Ok(parsed) => {
            let hits = search::search(
                &parsed,
                workbench
                    .entries
                    .iter()
                    .map(|(id, e)| (id.as_str(), &e.doc)),
            );
            json!({ "query": text, "notes": parsed.notes, "hits": hits })
        }
        Err(error) => json!({ "query": text, "error": error.0, "hits": [] }),
    }
}

async fn report(shared: Arc<Shared>, id: &str) -> Response {
    let id = id.to_owned();
    let page = blocking(shared, move |wb| {
        let id = checked_id(&id)?;
        let events = wb.events(&id)?;
        let entry = wb.entry(&id)?;
        if entry.reports.is_empty() {
            return Err(Failure::Invalid(format!(
                "{id} could not be judged, so it has no report: {}",
                entry.unjudged.as_deref().unwrap_or("no report")
            )));
        }
        let name = format!("{id}.jsonl");
        let page = mcp_trace_validator::html::render(
            &entry.reports,
            mcp_trace_validator::html::Trace {
                name: Some(&name),
                events: &events,
            },
        );
        Ok((id, page))
    })
    .await;
    match page {
        Ok((id, page)) => download(
            page.into_bytes(),
            "text/html; charset=utf-8",
            &format!("{id}.html"),
        ),
        Err(error) => failure(&error),
    }
}

/// A response the browser saves as `name`.
fn download(bytes: Vec<u8>, media: &'static str, name: &str) -> Response {
    let mut response = Response::new(Body::from(bytes));
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(media));
    // `name` is made of library ids, which are plain ASCII.
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

async fn export(shared: Arc<Shared>, ids: String) -> Response {
    let archive = blocking(shared, move |wb| {
        let ids: Vec<&str> = ids.split(',').filter(|id| !id.is_empty()).collect();
        for id in &ids {
            checked_id(id)?;
        }
        super::ops::export_archive(wb, &ids)
    })
    .await;
    match archive {
        Ok(bytes) => download(bytes, "application/x-tar", "mcp-library-export.tar"),
        Err(error) => failure(&error),
    }
}

#[cfg(test)]
mod tests;
