<!--
Agent Context Header (ULADP):
- Purpose: Anonymous-mode rate-limit + cookie dedup for the public submission
  endpoint (FR-FBR-06). In-memory keyed governor + BLAKE3-domain-separated
  token hash. NO durable state — restarting the API resets all buckets.
- Owner module: crates/feedbackmonk-anon/
- Read first: this README + Contract C3 (submission API) in
  docs/planning/plans/20260513T185711-feedbackmonk-v1-build-arc.md
-->

# feedbackmonk-anon

> **Synopsis**: Anonymous-mode rate-limit gate for the public submission
> endpoint (FR-FBR-06). In-memory governor keyed on `(BLAKE3(ip, cookie,
> project_id), project_id)`; per-(bucket, project) hourly quota with
> `Retry-After`-friendly exceedance reporting. No persistence — restarts
> reset buckets (acceptable per the P0 threat model).

## 1. Purpose & Responsibilities

- Rate-limit anonymous-mode feedback submissions per `(client_ip,
  anon_cookie, project_id)` bucket.
- Mint opaque anon cookies (16 random bytes, base64url-no-pad) when a
  request arrives with no `X-Feedbackmonk-Anon-Cookie` header.
- Compute `token_hash` — the domain-separated BLAKE3 input that binds
  `(ip, cookie, project_id)` together so a cookie-forging attacker on a
  different IP cannot exhaust someone else's budget.
- Stay process-local: no DB, no Redis, no shared state across instances.
  Multi-instance deploys accept independent buckets per instance (P0
  threat model — DEC-FBR-IMPL-* — explicitly downgrades the cross-instance
  invariant for simplicity).

## 2. File Index

| File | What it does |
|---|---|
| `src/lib.rs` | The entire crate surface — `AnonGate`, `RateLimitError`, `AnonAccepted`, `HASH_DOMAIN_PREFIX`, `DEFAULT_RATE_LIMIT_PER_HOUR`, `ANON_COOKIE_BYTES`, `ANON_COOKIE_HEADER`, plus deterministic-token-hash unit tests. |

## 3. Public API & Usage

```rust
use feedbackmonk_anon::{AnonGate, RateLimitError, ANON_COOKIE_HEADER};
use std::num::NonZeroU32;
use uuid::Uuid;

// Construction (production binary: from env-driven quota).
let gate = AnonGate::new(NonZeroU32::new(10).unwrap());

// Handler-level usage.
let project_id: Uuid = /* parsed from URL path */;
let cookie: &str = /* X-Feedbackmonk-Anon-Cookie header value or AnonGate::mint_cookie() */;
let ip: &str = /* ConnectInfo addr */;
let token_hash = AnonGate::token_hash(ip, cookie, project_id);

match gate.check(&token_hash, project_id) {
    Ok(_) => { /* proceed to repository write */ }
    Err(RateLimitError::Exceeded { retry_after_seconds }) => {
        /* 429 with Retry-After: <retry_after_seconds> */
    }
}
```

`HASH_DOMAIN_PREFIX = b"feedbackmonk-anon-v1"` — increment the version suffix
on any hash-input layout change so old and new hashes occupy disjoint
domains.

## 4. Constraints & Business Rules

1. **Domain separation prefix is load-bearing.** Replacing or removing
   `HASH_DOMAIN_PREFIX` silently merges the post-rotation hash space with
   the pre-rotation one. Version bumps are the *only* legal way to change
   hash inputs.
2. **No HMAC on the cookie itself.** The hash binds `(cookie, ip,
   project_id)` together; forging a cookie on a different IP lands in a
   different bucket, which is what the rate limiter cares about. HMAC on
   the cookie would be Q26-level overkill for the P0 threat model.
3. **Process-local, deferred-decision OK.** Cross-instance rate-limit
   coordination is out of scope for P0/P1. Sticky-session deploys retain
   the per-(bucket, project) invariant; non-sticky deploys accept
   independent budgets per instance.
4. **Quota lower bound: `NonZeroU32`.** A zero quota is rejected at
   construction time, never at request time (eliminating a "0 quota →
   every request 429" deploy-time misconfiguration).
5. **Cookie length: 16 random bytes** (`ANON_COOKIE_BYTES`). Base64url-no-pad
   encoding produces ~22 chars. Larger keys do not improve the security
   property (the limiter cares about bucket identity, not entropy).

## 5. Relationships & Dependencies

- **Consumed by**: `crates/feedbackmonk-api/src/handlers/feedback.rs`
  (`submit_anonymous_path`) and its tests.
- **Crate deps**: `governor` (keyed rate limiter), `blake3` (hash),
  `uuid`, `base64`, `rand`, `thiserror`.
- **Test deps**: dev-tests are in-crate (`#[cfg(test)] mod tests`); no
  `tests/` directory.
- **Multi-tenant isolation**: the BLAKE3 hash includes `project_id`, so
  per-project rate budgets are isolated even when an attacker reuses a
  cookie across projects (asserted by
  `token_hash_differs_per_project_id`). The crate has no DB access path,
  so the `multi-tenant-isolation-check` oracle's raw-SQL ban is N/A here.

## 6. Decision Log

- **In-memory governor over Redis.** Stage 2 / Stage 3 (P1) had the option
  to introduce Redis for cross-instance rate-limit coordination; the P0
  threat model explicitly accepted independent per-instance budgets, and
  single-instance dev/SaaS topology stays correct under the in-memory
  governor. Adding Redis is deferred until the multi-instance topology
  ships (post-P4 if ever).
- **Hash input order: `version || ip || \0 || cookie || \0 || project_id`.**
  `\0` separators prevent length-extension ambiguity (e.g., `ip="A"
  cookie="BC"` vs. `ip="AB" cookie="C"` produce different hashes). The
  separator choice predates the BLAKE3 keyed-hash mode addition in
  blake3 1.5 — keeping the explicit `\0` keeps the hash byte-stable
  through future blake3 upgrades that change keyed-mode semantics.
- **`AnonGate::token_hash` is `pub` (not handler-private).** The
  feedback-submission handler calls it directly so the same hash is used
  for the gate check AND the repository's `submit_anonymous` write path
  (both consume the 32-byte hash). Hiding it would force the handler to
  re-derive the same value twice with no semantic gain.
- **No FOREIGN KEY from this crate to anywhere.** The crate is pure
  computation + governor state. Repository-layer writes accept the hash
  bytes as `&[u8; 32]`; the crate boundary stays narrow on purpose
  (DEC-FBR-03 multi-tenant safety).
