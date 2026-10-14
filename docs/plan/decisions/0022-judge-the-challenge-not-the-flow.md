<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0022: Judge the Challenge, Not the Flow — the Authorization Pages' Observable Subset

**Date:** 2026-10-13
**Status:** Accepted (implements decision D4 of the
[2026-09-24 review](../../reports/project-review-2026-09-24.md), item 2.8)
**Author:** Tom F.

---

## Context

The authorization pages (`basic/authorization` at `2025-11-25`; four pages under
`basic/authorization/` at `2026-07-28`) were out of scope wholesale, with `TRAN-009`'s
exclusion as the boundary. D4 decided to enter "the HTTP-observable auth clauses (401 +
`WWW-Authenticate`, protected-resource metadata)" and keep full OAuth flows out. Since the
census gate (`spec-drift`) requires every MUST- and SHOULD-family keyword on an in-scope
page to sit inside an entry's quote, bringing the pages in means entering all of them —
194 entries across both revisions — and deciding, clause by clause, what a recording can
show.

What a Streamable HTTP trace carries was fixed before this decision: each exchange's
method, status and the headers on `RECORDED_HEADERS`, and its JSON body. Three facts about
that format decide the subset:

1. **`Authorization` is never recorded**, by construction
   ([05-security-model.md](../05-security-model.md)). No clause about which requests carry a
   token, or which token, can be judged.
2. **No request target is recorded.** An `http` event has no path or query. A token in the
   query string is invisible, and so is the identity of any non-MCP fetch.
3. **Protected Resource Metadata is not on the MCP endpoint.** `mcp-trace-capture`'s proxy
   relays every path, but maps it under the upstream's path, so the well-known URI a client
   builds from the proxy's origin (`/.well-known/oauth-protected-resource`) reaches the
   server at `/mcp/.well-known/...`; the `resource_metadata` URL in a challenge names the
   server's own address, which the proxy does not rewrite, so the client fetches it past
   the proxy. Where a fetch does pass through, fact 2 means its JSON body cannot be told from
   any other.

What *is* on the endpoint is the server's answer when it refuses: a `401` or `403` with a
`WWW-Authenticate` challenge. Until now the header was not on the allowlist either.

## Decision

1. **Record the challenge.** `www-authenticate` joins `RECORDED_HEADERS`. It is the server
   asking for a credential — scheme, `scope`, `error`, `resource_metadata` — and carries
   none; the client's `Authorization` stays off the list.
2. **Judge the challenge.** Seven checks (`checks::auth`) read a server response's parsed
   challenge (RFC 9110 §11.6.1): a `401`'s `scope`; the status the error-handling table
   gives each declared error (`invalid_request` 400, `invalid_token` 401,
   `insufficient_scope` 403) and the `401` for an invalid token; the insufficient-scope
   response's `403`, Bearer scheme, `scope` and `resource_metadata`; and at `2026-07-28`
   no `offline_access` in a challenge and no new scope on a retried request's second
   challenge. Each antecedent the clauses need — "the token was invalid", "the scope was
   insufficient" — is the server's own statement in its `error` parameter, which is why
   these can be judged at all.
3. **Exclude the rest with the reason that applies**: the authorization server's conduct,
   the client's OAuth flow (discovery, registration, PKCE, the `resource` parameter,
   `iss` validation), the credential (fact 1), the request target (fact 2), Protected
   Resource Metadata (fact 3), and the server's internal token validation. The disjunctive
   discovery MUST ("one of" the `WWW-Authenticate` parameter or the well-known URI) is
   excluded too: a `401` without `resource_metadata` is conforming when the other branch,
   which no recording shows, is taken.
4. **A request refused at the HTTP layer is not delivered.** A request whose exchange the
   framing ties to a `401`, or a `403` with a challenge, and that drew no JSON-RPC response,
   was never read as a request; the client's retry — the same message, with the same id, as
   the official SDKs send it — is the delivery. Without this, every conforming
   authorization session failed `BASE-003` (`2025-11-25`) or `BASE-045` (`2026-07-28`) and
   counted two sessions (`context/refusal.rs`). The refusal's own JSON body — a resource
   server's `{"error":"invalid_token"}`, which a recorder writes as a message event like
   any JSON body — is not a protocol message either when it carries no `jsonrpc` member:
   read as one it failed `BASE-006`, `BASE-008`, `BASE-009` and the wire schema.

## Consequences

### Positive

- The registry's completeness claim now covers the authorization pages, so an added or
  reworded authorization clause fails the weekly census like any other.
- A protected server's challenges are judged on every recording that carries them,
  including the `MUST` that a declared error gets its status.
- Conforming authorization sessions stop failing the id rules.

### Negative

- **The judged subset is small**: 5 of 85 entries at `2025-11-25`, 7 of 109 at
  `2026-07-28`. That is the honest size of what a recording of the MCP endpoint shows;
  the D4 wording ("protected-resource metadata") reads larger than what could be built.
- **Recordings made before this change carry no challenge**, so the new clauses read
  *not observed* on them — including every committed capture, none of which is of a
  protected server. No capture evidences them; the corpus pairs are authored.
- **The body rule leans on adjacency.** A refusal body is recognised as the event right
  after its status, which is how both recorders write a response's status and JSON body.
  A hand-made trace that separates them is judged as before, body and all.
- `RECORDED_HEADERS` is a public `[&str; N]`; its length changes.

## Alternatives Considered

- **Record the request path to identify metadata fetches.** Rejected: paths are where
  some deployments put credentials (`/mcp/<key>`), and a query string is where the clause
  forbids tokens to be — recording either to judge that clause would write the credential
  into every trace. And the fetch would still bypass the proxy (fact 3).
- **Recognise a Protected Resource Metadata body by its shape** (a non-JSON-RPC object with
  `resource` and `authorization_servers`). Rejected: a guess fitted to the document it is
  looking for convicts nothing reliably and invents a classification the trace does not
  carry.
- **Treat any non-2xx without a JSON-RPC response as a refusal** for the id rules.
  Rejected for now: the authorization statuses are the case this decision brings in and
  the SDKs' retry behaviour is known for them; widening it changes how existing `4xx`
  traces are judged and is its own decision.
- **Build the OAuth client and gate the suite's `auth/*` scenarios.** Left deferred
  (`auth-client-scenarios`): a large build whose only consumer is the host-as-SUT gate,
  and nothing it exercises reaches the recording.
