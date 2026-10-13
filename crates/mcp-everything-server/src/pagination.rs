// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Cursor-based pagination of the four list operations.
//!
//! Every catalogue this server lists is served in [`PAGES`] pages, and the
//! split is not arbitrary: **the first page is exactly the surface the
//! official suite's scenarios name** (register 2.15), and the later page is
//! the surface this server adds beyond it — icons, `resource_link` content,
//! annotations, an audio prompt, `x-mcp-header` designations. A client that
//! reads one page and stops, which is what every suite scenario does, sees
//! the catalogue it has always seen; a client that follows `nextCursor` sees
//! the rest. Paginating the suite's fixtures across pages instead would make
//! a scenario that lists once and looks for a fixture by name a test of the
//! client's paging rather than of the server.
//!
//! **Cursors are stateless, and that is forced by the newer revision.**
//! `2026-07-28` has no sessions, so a cursor cannot be remembered per client;
//! it has to be checkable on its own by any instance. Every catalogue here is
//! a constant of the binary, so the set of cursors this server can ever issue
//! is a constant too, and "did I issue this cursor?" is a membership test —
//! no state, no clock, and the same answer from a process that started after
//! the cursor was handed out. Anything outside that set was fabricated,
//! modified, or carried over from another server, and draws `-32602`
//! (`PAGE-003` at `2025-11-25`, `PAGE-011` at `2026-07-28`, both SHOULD).
//!
//! The token's spelling carries no meaning a client may rely on: cursors are
//! opaque (`PAGE-002`/`PAGE-010` bind the client to treat them so). It is
//! human-readable because a reader of a capture should be able to see which
//! page a request asked for without decoding anything.

use rmcp::ErrorData as McpError;

/// How many pages each listing is served in.
pub const PAGES: usize = 2;

/// One of the four operations that accept a `cursor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    /// `tools/list`.
    Tools,
    /// `prompts/list`.
    Prompts,
    /// `resources/list`.
    Resources,
    /// `resources/templates/list`.
    ResourceTemplates,
}

impl Listing {
    /// Every listing, in the order the specification's pages introduce them.
    pub const ALL: [Self; 4] = [
        Self::Tools,
        Self::Prompts,
        Self::Resources,
        Self::ResourceTemplates,
    ];

    /// The JSON-RPC method that lists this catalogue.
    #[must_use]
    pub const fn method(self) -> &'static str {
        match self {
            Self::Tools => "tools/list",
            Self::Prompts => "prompts/list",
            Self::Resources => "resources/list",
            Self::ResourceTemplates => "resources/templates/list",
        }
    }

    /// The listing `method` names, if it names one.
    #[must_use]
    pub fn of_method(method: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|listing| listing.method() == method)
    }

    /// The cursor this server issues for page `page` (zero-based) of this
    /// listing; `None` for the first page, which is asked for with no cursor,
    /// and for any page past the last.
    #[must_use]
    pub fn cursor(self, page: usize) -> Option<String> {
        (1..PAGES)
            .contains(&page)
            .then(|| format!("{}#page-{}", self.method(), page + 1))
    }

    /// The page a request asked for, given the `cursor` it presented.
    ///
    /// # Errors
    ///
    /// `-32602` (Invalid params) carrying the cursor in `data`, when the
    /// cursor is not one [`Self::cursor`] issues.
    pub fn requested_page(self, cursor: Option<&str>) -> Result<usize, McpError> {
        let Some(cursor) = cursor else {
            return Ok(0);
        };
        (1..PAGES)
            .find(|&page| self.cursor(page).as_deref() == Some(cursor))
            .ok_or_else(|| {
                McpError::invalid_params(
                    format!(
                        "cursor {cursor:?} was not issued by this server for {}",
                        self.method()
                    ),
                    Some(serde_json::json!({ "cursor": cursor })),
                )
            })
    }

    /// Page `page` of `items`, where `page_of` says which page each item
    /// belongs on, with the cursor of the page after it.
    ///
    /// Items keep their relative order, so a listing that was deterministic
    /// before pagination stays deterministic page by page.
    #[must_use]
    pub fn page<T>(self, items: Vec<T>, page_of: impl Fn(&T) -> usize, page: usize) -> Page<T> {
        Page {
            items: items
                .into_iter()
                .filter(|item| page_of(item) == page)
                .collect(),
            next_cursor: self.cursor(page + 1),
        }
    }
}

/// One page of a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    /// The items on this page.
    pub items: Vec<T>,
    /// The cursor of the next page, or `None` on the last.
    pub next_cursor: Option<String>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn every_issued_cursor_round_trips_to_its_page() {
        for listing in Listing::ALL {
            assert_eq!(listing.requested_page(None).unwrap(), 0);
            for page in 1..PAGES {
                let cursor = listing.cursor(page).unwrap();
                assert_eq!(listing.requested_page(Some(&cursor)).unwrap(), page);
            }
            assert_eq!(listing.cursor(0), None, "the first page has no cursor");
            assert_eq!(listing.cursor(PAGES), None, "nothing past the last page");
        }
    }

    #[test]
    fn a_cursor_this_server_never_issued_is_invalid_params() {
        for listing in Listing::ALL {
            for cursor in ["", "fabricated", "tools/list#page-3", "tools/list#page-1"] {
                let error = listing.requested_page(Some(cursor)).unwrap_err();
                assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
                assert_eq!(error.data, Some(serde_json::json!({ "cursor": cursor })));
            }
        }
    }

    #[test]
    fn a_cursor_is_valid_only_for_the_listing_that_issued_it() {
        let tools = Listing::Tools.cursor(1).unwrap();
        assert!(Listing::Tools.requested_page(Some(&tools)).is_ok());
        for other in [
            Listing::Prompts,
            Listing::Resources,
            Listing::ResourceTemplates,
        ] {
            assert!(other.requested_page(Some(&tools)).is_err(), "{other:?}");
        }
    }

    #[test]
    fn methods_map_both_ways() {
        for listing in Listing::ALL {
            assert_eq!(Listing::of_method(listing.method()), Some(listing));
        }
        assert_eq!(Listing::of_method("tools/call"), None);
    }

    #[test]
    fn pages_partition_the_items_in_order_and_chain_by_cursor() {
        let items = vec![1, 2, 3, 4, 5];
        let later = |item: &i32| usize::from(*item % 2 == 0);
        let first = Listing::Tools.page(items.clone(), later, 0);
        assert_eq!(first.items, [1, 3, 5]);
        let next = first.next_cursor.unwrap();
        let page = Listing::Tools.requested_page(Some(&next)).unwrap();
        let second = Listing::Tools.page(items, later, page);
        assert_eq!(second.items, [2, 4]);
        assert_eq!(second.next_cursor, None, "the last page ends the listing");
    }
}
