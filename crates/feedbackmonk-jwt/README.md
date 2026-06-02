<!--
Agent Context Header (ULADP):
- Purpose: EdDSA-only JWT verifier for feedbackmonk's submission API. Pure
  computation crate — no async, no I/O, no DB. The end-user JWT is the
  ONLY identity feedbackmonk ever has for an auth-mode submitter (DEC-FBR-04).
- Owner module: crates/feedbackmonk-jwt/
- Read first: this README + Contract C2 in
  docs/planning/plans/20260513T185711-feedbackmonk-v1-build-arc.md
-->

# feedbackmonk-jwt

## Synopsis

EdDSA-only JWT verifier for the public submission endpoint (Contract C2, FR-FBR-05). Pure computation crate — no async, no I/O, no DB. Single public function (`verify` / `verify_with_leeway`) consuming the end-user token signed by the customer's auth service — the ONLY identity feedbackmonk ever has for an auth-mode submitter (DEC-FBR-04). Algorithm allow-list rejects `alg=none`/`HS256` before any signature work; `aud` is checked before signature so wrong-audience tokens fail fast.

## 1. Purpose & Responsibilities

- Verify EdDSA JWTs against a project's active signing keys (Contract C2,
  FR-FBR-05).
- Enforce a strict header allowlist (`alg=EdDSA` only) to defeat
  `alg=none` and HMAC-confusion attacks.
- Enforce required claims (`sub`, `iat`, `exp`, `aud`), strict `exp`
  expiry, lenient `iat` (configurable clock-skew leeway, 5s default),
  and `aud` matching the URL-bound `project_id`.
- Cap `external_metadata` JSON at 4096 bytes (Contract C2 invariant 6).
- Try each active signing key in registration order; first verifying
  key wins (supports zero-downtime key rotation).

## 2. File Index

| File | What it does |
|---|---|
| `src/lib.rs` | The entire crate surface — `verify`, `verify_with_leeway`, `VerifiedClaims`, `JwtError`, `MAX_EXTERNAL_METADATA_BYTES`, `DEFAULT_IAT_LEEWAY_SECONDS`, `ACCEPTED_ALG`, plus structural unit tests. |
| `tests/verify.rs` | Hard-invariant tests (Contract C2 §Hard invariants 1–6): `alg=none` rejection, HMAC-confusion rejection, wrong-audience precedence over signature, missing-claim handling, strict `exp`, metadata-size cap, key-rotation order. |

## 3. Public API & Usage

```rust
use feedbackmonk_jwt::{verify, verify_with_leeway, JwtError, VerifiedClaims};
use feedbackmonk_core::SigningKey;
use uuid::Uuid;

// `now_unix` is injectable for testability — production callers pass the
// current epoch seconds.
let claims: VerifiedClaims = verify(
    bearer_token_str,
    project_id,
    &active_keys,       // Vec<SigningKey> from SigningKeyRepo::list_active
    chrono::Utc::now().timestamp(),
)?;

// claims.sub / .email / .name / .external_metadata are downstream inputs to
// FeedbackRepo::submit_authenticated.
```

Override the `iat` leeway in tests:

```rust
verify_with_leeway(token, project_id, &keys, now, /* iat_leeway_seconds = */ 0)
```

`JwtError` variants carry stable `variant_name()` strings (`"BadSignature"`,
`"Expired"`, `"WrongAudience"`, `"AlgorithmNotAllowed"`, etc.) — the HTTP
handler returns these in `{"error": "..."}` 401 bodies so integrations can
disambiguate without parsing free-form messages.

## 4. Constraints & Business Rules

1. **`alg=EdDSA` is the ONLY accepted algorithm.** Header is parsed
   before any signature work, so `alg=none` and `alg=HS256` (HMAC
   confusion) fail fast with `AlgorithmNotAllowed` regardless of key
   state. This is the load-bearing defense against the most common JWT
   exploits.
2. **`exp` is strict; `iat` has leeway.** `now > exp` → `Expired` with
   zero tolerance. `iat > now + leeway` → `NotYetValid`. Customer-side
   minting conventions (e.g., 5-min sliding TTLs) are the customer's
   call, not the verifier's.
3. **Wrong-audience precedes signature check.** A token signed by
   project A's key that lands at project B's endpoint fails with
   `WrongAudience` without revealing whether a candidate key would have
   verified. Stops cross-project key-existence probing.
4. **`external_metadata` cap: 4096 bytes JSON.** Larger payloads yield
   `ExternalMetadataTooLarge`. The schema column is `JSONB`; this cap
   matches the row-level size invariant.
5. **Key rotation: registration-order list.** `active_keys` is consumed
   in `registered_at ASC` order per `SigningKeyRepo::list_active`. First
   key whose signature verifies wins. Customers rotate by registering a
   new key + deactivating the old one with overlap; no downtime.
6. **NEVER use `verify` to validate non-end-user tokens.** Per DEC-FBR-04
   the customer's auth provider's JWT IS the identity. There are no
   long-lived bearer tokens for feedbackmonk-issued identities anywhere —
   this verifier is single-purpose.

## 5. Relationships & Dependencies

- **Consumed by**: `crates/feedbackmonk-api/src/handlers/feedback.rs`
  (auth-mode submission path) and its router-level integration tests.
- **Crate deps**: `ed25519-dalek` (signature verification), `base64`
  (URL-safe-no-pad decode), `serde`, `serde_json`, `thiserror`, `uuid`,
  `feedbackmonk-core` (SigningKey type).
- **No DB / no async / no I/O.** Pure computation — keeps the crate
  ammortized-zero in the request hot path and trivially auditable.
- **`multi-tenant-isolation-check` oracle**: the crate has no SQL access,
  so the raw-SQL ban is N/A. The handler that calls this verifier
  enforces scope via the `ProjectScope` returned from
  `ProjectRepo::open_for_submission`.

## 6. Decision Log

- **EdDSA only, not RSA / ECDSA.** Single algorithm minimizes verifier
  surface area. Customers integrating with feedbackmonk's submission API
  mint their tokens; expanding to RSA/ECDSA is a feature request, not a
  threat-model concession.
- **`now_unix: i64` is injected, not `chrono::Utc::now()` inside the
  verifier.** Tests pass fixed timestamps to drive the `Expired` /
  `NotYetValid` paths deterministically without freezing the clock.
- **Wrong-audience BEFORE signature.** Information-leak hardening per
  Contract C2 invariant 3. The naive ordering (signature first, audience
  second) leaks "your key is registered against project A" via
  observable timing/error differences. Audience-first reverses the
  leak direction (an attacker observing a `WrongAudience` learns only
  that they're hitting the wrong project endpoint).
- **`MissingRequiredClaim(&'static str)` carries the claim name in the
  variant.** Stable error identity for tests + telemetry; `variant_name()`
  returns the bare `"MissingRequiredClaim"` form for client-facing bodies
  so the claim name never leaks externally.
- **Stable variant-name strings.** Asserted by
  `variant_names_are_stable_strings` (`src/lib.rs::tests`). Renaming a
  variant requires a deliberate test edit, which surfaces the
  client-contract break in code review.
