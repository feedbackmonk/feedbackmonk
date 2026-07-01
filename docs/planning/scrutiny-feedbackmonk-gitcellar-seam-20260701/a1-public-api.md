# Child A1 — Public/End-User API Surface Review

**Reviewer**: A1 (`a1-public-api`) · **Date**: 2026-07-01 · **Mode**: read-only first-principles
**North star**: sole feedback backend for a privacy-branded product, no fallback.

## Scope covered

Read in full: `handlers/{feedback,me_feedback,attachments,board,roadmap(partial),voting_common,solicitation,widget_config,capabilities,health}.rs`, `cors.rs`, `router.rs`, `main.rs::build_app`, `error.rs`, `auth/mod.rs`, crates `feedbackmonk-jwt` and `feedbackmonk-anon` (full), `feedbackmonk-core::ids` (short-code gen), plus `migrations/00021_submit_idempotency.sql` and `repository/feedback.rs` idempotency claim (scoping only — SQL internals fenced to B). Cross-checked `widget/src/api.ts` for the real browser transport, and `tests/{attachment_list_download,router_submission_integration}.rs` for harness fidelity.

**NOT read** (out of scope / fenced): repository SQL internals (B), admin/login/moderation/promote handlers (A2), agentic/work-order/runner handlers (D), contract-doc-vs-consumer drift (F), frontends beyond the one `api.ts` transport check (E), full `roadmap.rs` admin half (A2-adjacent). Privacy findings tagged `→C`.

---

## Findings (ranked)

### F1 — P0 · LIVE · Attachment list/download/upload are unauthenticated; sensitive bytes gated only by a *non-secret* short code
`attachments.rs:34-46` (auth-model doc), `:261-286` (list), `:309-338` (download), `:108-249` (upload).

All three attachment routes mint scope via `open_for_submission(project_id)` and resolve the feedback by the public `FB-XXXXXX` short code. There is **no** identity check — no JWT `sub` match, no anon-cookie match. The `project_id` is public (embedded in every widget as `data-project-id`); the short code is explicitly **non-secret** — `ids.rs:27` says it exists "so customers can reference items in conversation, support tickets, and product comms." So anyone who sees a short code (support ticket, screenshot, shoulder-surf, or the submitter forwarding it) can `GET …/attachments/{id}` and pull the raw uploaded **screenshots and console/service logs** — the most PII-dense artifacts in the system (`→C`). Upload is equally open: any party with a valid short code can attach to *another user's* feedback.

The module doc frames identity-binding as "a hardening follow-up tracked in the completion notes; v1 parity matches the frozen contract." For a sole backend of a privacy-branded product that is not acceptable: the deferral silently makes attachment confidentiality depend on short-code secrecy, and the short code is designed to be shareable.

- **Adjudication (c) the RULE is wrong.** DEC-PODS-001's "public `open_for_submission` scope is enough for attachments" was defensible when attachments were fire-and-forget-after-submit; it is wrong for *read*. Bind reads to the identity that created the feedback: for auth-mode feedback require a JWT whose `sub == end_user_sub`; for anon-mode require the anon cookie whose hash matches `anon_token_hash`. At minimum, gate reads behind the same JWT the `/me/feedback` surface already uses.
- **Remediation**: add an ownership check to list/download (and ideally upload) mirroring `me_feedback::authenticate`. **Effort M.**

### F2 — P1 · LIVE · Anon cookie dedup is architecturally broken for the real browser widget (reads a request header the browser never sends)
`feedback.rs:477-492` + `feedbackmonk-anon/lib.rs:45-47`; transport `widget/src/api.ts:78-83`; CORS allowlist `cors.rs:96`.

The anon path reads the cookie from **`headers.get("X-Feedbackmonk-Anon-Cookie")`** — a *request header* — and, when absent, mints one via `Set-Cookie: X-Feedbackmonk-Anon-Cookie=…`. But the widget submits cross-origin with `credentials: "include"` (api.ts) and never sets that custom header; api.ts sends only `Content-Type`/`Accept`/`Authorization`. A browser that stored the Set-Cookie will replay it in the standard **`Cookie:`** header, which no handler parses (grep confirms only `session.rs`/`admin_tier.rs` touch `header::COOKIE`). Furthermore the CORS `allow_headers` list (`content-type, authorization, idempotency-key`) omits the anon header, so the widget *couldn't* send it cross-origin even if it tried. Net effect: every real widget submit mints a fresh cookie and never matches a prior one — **per-cookie dedup (FR-FBR-06) silently degrades to IP-only** for all browser traffic. `cors.rs:20-31` even asserts the cookie "travels" via `credentials:include`, but the read path never consults the cookie jar.

Tests mask this: `router_submission_integration.rs:216` and `tier_enforcement_smoke.rs:162` pass the value as an explicit `X-Feedbackmonk-Anon-Cookie` **header** (a non-browser client), so they exercise a path the widget never takes — false confidence.

- **Adjudication (b) implementation wrong** (with an honesty-of-claims defect in cors.rs + tests). The design intent (cookie transport) is sound; the read must parse the `Cookie:` header.
- **Remediation**: read the anon value from the `Cookie` header (via a cookie extractor) with the custom-header path kept as a fallback for non-browser consumers (GitCellar Desktop); add a browser-flow integration test that sends `Cookie:` not the custom header. **Effort S-M.**

### F3 — P1 · LIVE · No rate limiting on any public attachment route (upload storage-exhaustion DoS + enumeration)
`attachments.rs` (`AttachmentState` holds no gate — confirmed no `AnonGate` reference); router `:410-422`.

Upload/list/download carry no `AnonGate`, no `LoginGate`, no per-IP throttle. Images are capped at 4/feedback, but **log parts have no per-feedback count cap** — each upload can add a `service_log` + `console_log` (≤1 MB each), and nothing limits the number of upload requests, so an attacker with one valid short code can write unbounded 1 MB objects → object-store/DB exhaustion. Download has no throttle either, so short-code enumeration (F1) is unmetered.

- **Root cause (class-level)**: rate limiting is **bolted onto individual handlers** (`AnonGate` in submit + voting only) rather than applied as a router-level layer. Every public route added after P0 — the Phase-A attachment reads, and the `/me/*` writes — shipped with no gate because there is no default. New public surfaces will keep inheriting the gap.
- **Adjudication (c) rule wrong** (rate-limit-per-handler strategy). Add a tower middleware that rate-limits by client IP across the whole public/CORS-exposed subtree (submit, attachments, board, roadmap, widget-config) so coverage is default-on, not opt-in.
- **Remediation**: per-IP layer on the public routers; add a per-feedback log-attachment count cap. **Effort M.**

### F4 — P1 · LIVE (low exploitability) · Cross-user idempotency collision within a project drops a user's feedback and leaks the victim's short code
`feedback.rs:436-445` (verbatim key), `repository/feedback.rs:762-807` + `migrations/00021:31-40` (PK is `(project_id, idempotency_key)`).

The dedupe key is `(project_id, idempotency_key)` — **not** scoped to the submitter (`end_user_sub` / `anon_token_hash`). Within one project, if two different users send the same `Idempotency-Key`, the second submit is deduped to the first user's row and returns the **first user's `feedback_id`**. Two harms: (1) user B's real feedback is silently discarded (correctness); (2) B receives A's `FB-XXXXXX`, an existence/enumeration oracle — and combined with F1's unauthenticated attachment reads, a guessed key hands the attacker a victim's short code that unlocks the victim's attachment bytes. Exploitability is bounded by clients choosing unguessable keys (UUIDs), but nothing in the contract *requires* that, and the server offers no defense.

- **Adjudication (c) rule wrong** — idempotency scope should include submitter identity. First-write-wins is right; the key domain is too wide.
- **Remediation**: extend the dedupe key/uniqueness to include `end_user_sub` (auth) or `anon_token_hash` (anon). **Effort M** (migration + repo, coordinate with B).

### F5 — P1 · LIVE · JWT verifier enforces no maximum TTL and there is no per-token revocation
`feedbackmonk-jwt/lib.rs:22-24,207-210`.

`exp` is checked strict but there is **no cap on `exp - iat`** — the verifier accepts a token expiring years out. The doc calls the 5-minute TTL "a customer-side minting convention, not verifier leeway." Under DEC-FBR-04 the customer-signed JWT is the *only* identity FeedbackMonk holds, and post-cutover FeedbackMonk is the sole record-holder. A single leaked long-lived end-user token cannot be revoked except by deactivating the project's signing key — which revokes **every** user of that project. There is no `jti` denylist, no single-subject revocation, and (see F1) sensitive data is reachable.

- **Adjudication (c) rule wrong / AMEND** — JWT-only identity is fine for authentication but needs a revocation and max-TTL story for a sole backend.
- **Remediation**: enforce a server-side max `exp - iat` (reject over-long tokens with `Expired`/new variant); add an optional `jti` denylist checked in `verify`. **Effort M.** Escalate to root (matches rubric standing challenge on JWT-only identity).

### F6 — P2 · LIVE · Voting anon cookie uses `SameSite=Lax` and omits `Secure` (inconsistent with submit)
`voting_common.rs:158-169` vs `feedback.rs:487-489`.

The vote-path minted cookie is `SameSite=Lax` with no `Secure`; the submit path is `SameSite=None; Secure`. The board/roadmap vote routes are CORS-exposed cross-origin, so a `Lax` cookie would never ride a cross-site request even if the read path worked. Moot in practice because of F2 (the cookie is never read back anyway), but it is an inconsistency that will bite once F2 is fixed, and `Lax`-without-`Secure` is a weaker posture. **Adjudication (b)**. Align with the submit attributes when fixing F2. **Effort S.**

### F7 — P2 · LIVE · Health endpoint leaks exact build version + start time, unauthenticated
`health.rs:39-49`. `/health` and `/health/ready` return `version` (exact crate version) and `started_at` to any unauthenticated internet caller. Version disclosure eases CVE targeting for a sole backend. `→` consider gating the version/`started_at` fields behind the ops token, or returning them only on the internal readiness path. **Adjudication (c) minor rule** — expose liveness without build fingerprint. **Effort S.**

### F8 — P2 · authenticated · `/me/feedback/export` is unbounded + N+1
`me_feedback.rs:415-458`. Export calls `list_for_end_user(u32::MAX, 0)` then issues per-row reply + attachment queries with no pagination and no rate limit. A JWT holder with many rows makes this an expensive, repeatable request. JWT-gated so low risk, but a sole-backend abuse/cost vector. **Remediation**: cap rows or stream; add the F3 rate layer. **Effort S-M.**

### F9 — P2 · LIVE · `email`/`name` JWT claims are not size-capped
`feedbackmonk-jwt/lib.rs:252-259`. Only `external_metadata` is bounded (4096 B). Oversized `email`/`name` strings flow to storage unbounded. Low severity; add a modest cap. **Effort S.**

---

## What was checked and is sound (with citations)

- **JWT hardening** (`jwt/lib.rs:168-180,215-219,231-270`): EdDSA-only; `alg` parsed and rejected **before** any signature work (alg-none + HS256 confusion fail fast); wrong-audience returned **before** signature (no key-probing oracle); `verify_strict` (rejects malleable/small-order sigs); required-claim checks. Solid.
- **me_feedback delete has no TOCTOU** (`me_feedback.rs:357-386`): ownership resolved by `sub`, and the `DELETE` is `sub`-scoped again inside the repo call. Byte-purge-before-row order documented and correct for retry-safety.
- **Board moderation gate** (`board.rs:189-276`): vote cast/retract resolve through `resolve_approved_board_feedback_id` (approved-only) **before** any write; pending/rejected/board-disabled → 404 identically to reads (no existence oracle). `ensure_board_enabled` before every board read.
- **Image validation** (`attachments.rs:373-398`): MIME allowlist **and** magic-byte sniff; charset suffix tolerated. Good.
- **Error hygiene** (`error.rs:88-101,136-151`): `Internal`/sqlx detail logged but body is a flat `"internal error"` — no DB detail leaks. `{"error": variant}` shape is consistent across public handlers.
- **CORS credentials guard** (`cors.rs:87-99`): never `Any` with credentials; empty allowlist = deny — secure default. Correct (its *purpose* still depends on F2's cookie path).
- **404-vs-403 discipline**: cross-user/cross-project feedback resolution returns `NotFound` throughout (`me_feedback`, `board`), not 403 — no existence oracle **except** the F4 idempotency case.

---

## Local mandate verdicts

| ID | Mandate | Verdict | Reasoning |
|---|---|---|---|
| M-A1 | DEC-FBR-04 JWT-only end-user identity | **AMEND** | Fine for authN, but sole-backend needs max-TTL + revocation (F5) and out-of-band erasure/data-subject flows. |
| M-A2 | `open_for_submission` public scope is sufficient for attachment routes | **REPLACE** | Sensitive bytes must be identity-bound, not short-code-gated (F1). |
| M-A3 | Rate limiting applied per-handler (AnonGate in submit/voting) | **REPLACE** | Router-level middleware so every public write is covered by default (F3). |
| M-A4 | 5-min JWT TTL is a minting convention, not verifier-enforced | **AMEND** | Verifier should reject over-long `exp - iat` (F5). |
| M-A5 | Anon dedup transported via custom `X-Feedbackmonk-Anon-Cookie` header | **REPLACE** | Browsers send the value in `Cookie:`; the header transport is dead for the widget (F2). Parse `Cookie:`, keep header as non-browser fallback. |
| M-A6 | 404-not-403 for cross-scope resolution | **KEEP** | Correctly applied everywhere except the F4 idempotency oracle. |
| M-A7 | `{"error": <variant>}` body shape + no internal-detail leak | **KEEP** | Consistent and non-leaky. |
| M-A8 | Anon rate limit only on *anon* submit; auth-mode relies on tier cap | **AMEND** | Auth-mode submit has no abuse throttle; self-host tier is uncapped. Add a per-sub/IP ceiling. |

An all-KEEP result would have been suspicious; three REPLACE + three AMEND reflect genuine sole-backend gaps.

## Retire-or-justify

- `resolve_voter` vs `resolve_voter_no_rate_limit` (voting_common) — near-duplicate; **justify** (retract intentionally skips the gate). Fine.
- `extract_bearer` duplicated across `feedback`, `me_feedback`, `solicitation`, `voting_common` — **consolidate** (polish, not retire); one shared helper reduces drift risk.
- No dead handlers found in this cluster.

## ADD candidates (missing capabilities the rubric demands)

- **Per-token/`jti` revocation + max-TTL enforcement** — goal: sole-backend identity control, data-subject/erasure flows. Gap: F5, none today.
- **Router-level rate-limit middleware for all public writes** — goal: abuse resistance for the sole backend. Gap: F3; attachments and `/me/*` writes wholly unthrottled.
- **Attachment identity binding (read + upload)** — goal: confidentiality of PII-dense artifacts. Gap: F1.
- **Auth-mode submit throttle** — goal: abuse resistance where tier cap is the only backstop (and is `None` for self-host). Gap: M-A8.

## Rubric challenges (escalations to root)

1. **JWT-only identity (DEC-FBR-04) is insufficient as-is for a sole record-holder** — no revocation, no max-TTL, no out-of-band erasure path (F5). Matches the rubric's own standing challenge; recommend M1 critic family adopt.
2. **Attachment confidentiality resting on a non-secret short code** (F1) — privacy escalation `→C`: the most PII-dense artifacts (screenshots, logs) are readable by short-code alone.
3. **Rate-limiting-per-handler as the strategy** — generates recurring coverage gaps every time a public route is added (F3); a class-level default is the fix, not per-route patches.

## Coverage / confidence notes

- High confidence on F1/F2/F3/F5 (read the full code paths + the browser transport + CORS allowlist + tests). F2 in particular is verified against the actual `Cookie`-header parsing absence (grep) and the widget's `api.ts` headers.
- F4 confirmed at the migration PK + repo claim level; **did not** run it live (no DB) — exploitability rated low pending client key-generation behavior (F/consumer-side, escalate to F for the GitCellar client's key scheme).
- Did not execute oracles or tests (read-only; no DB). Repository SQL internals deliberately left to B; I only inspected idempotency-key *scoping*, which is a semantics concern surfacing in this cluster.
