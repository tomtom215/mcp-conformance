// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! SEP-2549 caching hints, and which results carry them.
//!
//! rmcp gives every cacheable result the same pair of builders — `with_ttl_ms`
//! and `with_cache_scope` — but they come from four separate `impl` blocks
//! (three expanded from its paginated-result macro, one hand-written on
//! [`DiscoverResult`]) with no trait tying them together, so a handler cannot
//! be generic over "a result that can carry hints" without declaring that
//! trait. [`Cacheable`] is that declaration and nothing more: one method,
//! implemented by delegation, so the four call sites in the handler read the
//! same and a fifth cacheable result is one line here rather than another
//! copied `if`.

use rmcp::model::{
    CacheScope, DiscoverResult, ListPromptsResult, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, ReadResourceResult,
};

use super::ServedRevision;

/// How long a client may treat this server's answers as fresh (`ttlMs`).
///
/// Every answer it applies to — the discovery result, the resource catalogue,
/// the templates, the resource bodies — is a compile-time constant of this
/// binary, so any finite lifetime is honest and the value is a readability
/// choice rather than a correctness one. A minute is short enough that a human
/// reading a capture sees a plausible number, and long enough to be visibly
/// not "do not cache".
const TTL_MS: u64 = 60_000;

// CACH-008 forbids a negative TTL, which `u64` already makes unrepresentable.
// What remains worth stating is that the hint is not zero: a zero TTL is the
// wire's way of saying "do not cache", so a server shipping one while claiming
// to demonstrate caching would be contradicting itself. Enforced at compile
// time rather than by a test, because there is no runtime input to vary.
const _: () = assert!(TTL_MS > 0, "a zero TTL would mean `do not cache`");

/// Who may reuse a cached answer (`cacheScope`).
///
/// [`CacheScope::Public`] because this server authenticates nobody and varies
/// nothing by caller: two clients asking the same question get the same bytes,
/// which is exactly the condition the scope names. A server that did vary its
/// answers by identity would owe `Private` here.
const SCOPE: CacheScope = CacheScope::Public;

/// A result that can carry caching hints.
pub(super) trait Cacheable: Sized {
    /// This result with [`TTL_MS`] and [`SCOPE`] applied.
    fn with_hints(self) -> Self;
}

macro_rules! cacheable {
    ($($result:ty),+ $(,)?) => {
        $(impl Cacheable for $result {
            fn with_hints(self) -> Self {
                self.with_ttl_ms(TTL_MS).with_cache_scope(SCOPE)
            }
        })+
    };
}

// The six operations the `2026-07-28` caching page names, less the two this
// crate does not build the result for: `tools/list` and `prompts/list` are
// answered by rmcp's `#[tool_handler]`/`#[prompt_handler]` expansions, which
// set their own hints. Their result types are listed anyway so the set here is
// the specification's set rather than "whatever we happened to reach", and so
// a hand-written replacement for either handler has the impl waiting.
cacheable!(
    DiscoverResult,
    ListToolsResult,
    ListPromptsResult,
    ListResourcesResult,
    ListResourceTemplatesResult,
    ReadResourceResult,
);

/// `result`, with caching hints when `revision` defines them.
pub(super) fn applied<T: Cacheable>(revision: ServedRevision, result: T) -> T {
    if revision.emits_caching_hints() {
        result.with_hints()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_older_revision_carries_no_hints() {
        let result = applied(
            ServedRevision::V2025_11_25,
            ListResourcesResult::with_all_items(Vec::new()),
        );
        assert_eq!(result.ttl_ms, None);
        assert_eq!(result.cache_scope, None);
    }

    #[test]
    fn the_stateless_revision_carries_both_hints() {
        let result = applied(
            ServedRevision::V2026_07_28,
            ListResourcesResult::with_all_items(Vec::new()),
        );
        assert_eq!(result.ttl_ms, Some(TTL_MS));
        assert_eq!(result.cache_scope, Some(SCOPE));
    }

    #[test]
    fn a_read_result_takes_the_same_hints() {
        let result = applied(ServedRevision::V2026_07_28, ReadResourceResult::new(vec![]));
        assert_eq!(result.ttl_ms, Some(TTL_MS));
        assert_eq!(result.cache_scope, Some(SCOPE));
    }

    #[test]
    fn a_discovery_result_replaces_rmcps_do_not_cache_defaults() {
        // `DiscoverResult`'s fields are not optional, so "no hint" is the
        // constructor's `0` / `Private` — a real answer meaning "immediately
        // stale, one caller only", not an absent one. The older revision keeps
        // it; the newer replaces it.
        let bare = DiscoverResult::new(Vec::new(), rmcp::model::ServerCapabilities::default());
        assert_eq!(bare.ttl_ms, 0);
        assert_eq!(bare.cache_scope, CacheScope::Private);
        let hinted = applied(ServedRevision::V2026_07_28, bare.clone());
        assert_eq!(hinted.ttl_ms, TTL_MS);
        assert_eq!(hinted.cache_scope, SCOPE);
        let unchanged = applied(ServedRevision::V2025_11_25, bare);
        assert_eq!(unchanged.ttl_ms, 0);
        assert_eq!(unchanged.cache_scope, CacheScope::Private);
    }
}
