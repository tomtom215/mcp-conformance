// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Listing: a `*/list` method followed through its pages.

use serde_json::{Map, Value, json};

use super::{Ended, Reply, Session};

impl Session<'_> {
    /// Every item `method` lists under `member`, following `nextCursor` for
    /// at most the plan's page budget.
    pub(super) async fn list(&mut self, method: &str, member: &str) -> Result<Vec<Value>, Ended> {
        let mut items = Vec::new();
        let mut cursor: Option<String> = None;
        for page in 0.. {
            if page >= self.plan.max_pages {
                self.summary.stopped_by.get_or_insert_with(|| {
                    format!("{method} was followed for {page} pages and offered more (--max-pages)")
                });
                break;
            }
            let mut params = Map::new();
            if let Some(cursor) = &cursor {
                params.insert("cursor".to_owned(), json!(cursor));
            }
            let Reply::Result(result) = self.ask(method, params, Vec::new(), method).await? else {
                break;
            };
            items.extend(
                result
                    .get(member)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        Ok(items)
    }
}
