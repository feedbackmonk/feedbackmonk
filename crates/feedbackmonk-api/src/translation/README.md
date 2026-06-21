<!--
Agent Context Header (ULADP):
- Purpose: Multilingual feedback translation (FR-FBR-30) — pluggable provider
  abstraction (default OFF) + the async translate-after-accept worker.
- Owner module: crates/feedbackmonk-api/src/translation/
- Read first: this README + docs/specs/DECISIONS.md DEC-FBR-IMPL-25 / DEC-FBR-IMPL-26
-->

# translation/ — Multilingual feedback translation (FR-FBR-30)

## Synopsis

Pluggable, **default-OFF** translation provider (`TranslationProvider` trait + DeepL adapter + no-op) plus the async **translate-after-accept** worker. Non-English feedback is translated to a canonical language (English, v1) by a background poll-loop — NEVER on the public submit path — so English-assuming consumers (sentiment, the agentic loop / clustering, admin FTS) work correctly, while the verbatim `body` is never overwritten (Q24). Egress is a conscious, disclosed, opt-in choice.

## 1. Purpose & Responsibilities

Implements **FR-FBR-30** under **DEC-FBR-IMPL-25** (data/processing model) and **DEC-FBR-IMPL-26** (provider posture):

- **Provider abstraction** — a `Send + Sync` trait decouples the worker from the concrete backend, mirroring the `email::Mailer` precedent. `main.rs::build_translation_provider()` env-selects the impl; the provider **defaults to `off`** (returns `None` → no worker, no egress).
- **Translate-after-accept worker** — a poll-loop (`spawn_translation_worker`, modelled on `spawn_voting_cache_refresh`) drains rows the submit path stamped `translation_status='pending'`, translates each off the request path, and writes `body_translated` + `source_lang` back. Tolerates per-row provider failure (marks `failed`, bounded retry).

The per-row storage + the four worklist repository methods (`claim_pending_translations` / `set_translation` / `mark_translation_skipped` / `mark_translation_failed`) live in `feedbackmonk-repository::feedback`; this module is the provider + worker only.

## 2. File Index

| File | One-line summary |
|------|---|
| `mod.rs` | Module surface — `TranslationProvider` trait + `TranslateOutput` + re-exports. |
| `deepl.rs` | DeepL **cloud** adapter (`reqwest` 0.12 rustls) — egress. Auto-detects source language; routes free (`…:fx`) vs Pro keys by host. |
| `libretranslate.rs` | LibreTranslate adapter — the **no-egress** option (self-hosted, AGPL). Operator-supplied URL + optional api key; `source: "auto"` returns the detected language. |
| `noop.rs` | `NoOpTranslator` — the default-off contract type; returns input unchanged (detected == target → worker marks `skipped`). |
| `worker.rs` | `spawn_translation_worker` + `translate_once` poll-loop; `same_language` skip-detection; worklist constants (poll secs, batch, attempts cap). |
| `README.md` | This file. |

## 3. Public API & Usage

```rust
// main.rs: env-select the provider (defaults OFF) and spawn the worker.
let provider = build_translation_provider()?;                 // Option<Arc<dyn TranslationProvider>>
feedbackmonk_repository::TranslationFlag::set(provider.is_some());
if let Some(p) = provider {
    let _tick = spawn_translation_worker(p, Arc::clone(&state.feedback), target_lang, poll_secs);
}
```

Tests substitute a `FakeTranslator` implementing `TranslationProvider` and drive `translate_once(provider, repo, target_lang)` directly (the trait is the test seam; the real DeepL call is non-deterministic and exercised only on demand).

## 4. Constraints & Business Rules (load-bearing — never silently relax)

- **Provider DEFAULTS to `off`.** Enabling a cloud provider egresses feedback bodies (personal data) to a GDPR data processor (DEC-FBR-IMPL-26). Do NOT change the default away from `off`, remove the `off` option, or hardcode a provider without re-opening DEC-FBR-IMPL-26. Enforced by the `translation-egress-q24-isolation` oracle (Probe A) + disclosed in `docs/operations/SELFHOST_ENV.md` (Contract C21).
- **NEVER on the submit path.** Translation runs ONLY in the background worker, after a row is accepted (DEC-FBR-IMPL-25 D3). The submit path only STAMPS `pending`; it never calls a provider. Keeps the public widget endpoint's latency + uptime independent of the provider.
- **Store-both; the original is never overwritten.** The worker writes `body_translated` + `source_lang` alongside the verbatim `body` (Q24). `set_translation` is the ONLY writer of `body_translated`, and ONLY the worker calls it (enforced by the oracle, Probe C).
- **Public surfaces read the verbatim original.** The machine consumer `list_member_bodies_for_cluster` reads the translation (with fallback to the original), and the data-controller admin may view it via the ONE scoped reader `get_translation_for_admin` (the admin-UI original↔translation toggle, FR-FBR-30 #3). No **public / end-user / board** read may select `body_translated` (Q24; oracle Probe B allowlist).
- **Lazy backfill + manual escape hatch.** Only feedback accepted after a provider is enabled is translated. The operator endpoint `POST /api/v1/ops/translation/backfill` (behind `FEEDBACKMONK_OPS_TOKEN`) stamps pre-existing body-bearing rows `pending` so the worker picks them up (FR-FBR-30 #5).
- **Provider outage is never user-visible.** A failed translation marks the row `failed` (re-pollable until the attempts cap); every consumer falls back to the verbatim `body` while a row is un-translated.
- **No body in logs.** The worker logs only `feedback_id` + error on failure; the DeepL adapter never logs the response body verbatim.

## 5. Relationships & Dependencies

- **Consumes** `feedbackmonk_repository::FeedbackRepo` (the four worklist methods + the `TranslationFlag` enablement flag).
- **Constructed by** `main.rs::build_translation_provider()` + spawned beside the voting-cache tick.
- **Config** via `FEEDBACKMONK_TRANSLATION_*` env vars (`docs/operations/SELFHOST_ENV.md` Contract C21).
- **Guarded by** the `translation-egress-q24-isolation` Verification Oracle (`.claude/oracles/`).
- **FTS**: migration 00019 repoints `body_tsv` to `coalesce(body_translated, body)` — no code in this module touches FTS (the generated column does it).

## 6. Decision Log

- **Worker reads a process-global enablement flag (not an AppState field).** The submit-time `pending` stamp happens inside the repository INSERT, and the repository crate can't depend on this crate where the provider is built. A repo-crate `AtomicBool` (`TranslationFlag`) keeps the stamp decision co-located with the INSERT and avoids threading a bool through every submit signature + ~15 test call-sites (the plan's Deferred Decision D1 — OnceLock recommendation; resolved here as a repo-crate global to honour both "stamp in the repo" and "zero signature churn").
- **DeepL for v1, default off; local engine deferred.** DeepL is EU-based, offers a DPA, and does not train on Pro-API text — the privacy-preferred cloud choice (DEC-FBR-IMPL-26). A local no-egress engine (LibreTranslate the likely AGPL candidate) is future work.
- **Bounded retry via an attempts counter, not a time backoff.** `failed` rows are re-claimed until `translation_attempts` reaches the worker cap, then terminal. Tolerates a transient blip without a backoff-timestamp column or a tight retry loop on a persistent outage (plan Deferred Decision D2).
- **Global (unscoped) worklist claim.** Translation is tenant-agnostic transport keyed on `feedback.id`; the worker drains a global queue rather than looping per project (plan Deferred Decision D3). The four worklist methods are documented pre-auth exceptions in the `multi-tenant-isolation-check` allowlist.
