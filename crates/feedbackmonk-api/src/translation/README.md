<!--
Agent Context Header (ULADP):
- Purpose: Multilingual feedback translation (FR-FBR-30) — pluggable provider
  abstraction (default OFF) + the async translate-after-accept worker.
- Owner module: crates/feedbackmonk-api/src/translation/
- Read first: this README + docs/specs/DECISIONS.md DEC-FBR-IMPL-25 / DEC-FBR-IMPL-26
-->

# translation/ — Multilingual feedback translation (FR-FBR-30)

## Synopsis

Pluggable, **default-OFF** translation provider (`TranslationProvider` trait + DeepL adapter + no-op) plus the async **translate-after-accept** worker. Non-English feedback is translated to a canonical language (English, v1) by a background poll-loop — NEVER on the public submit path — so English-assuming consumers (sentiment, the agentic loop / clustering, admin FTS) work correctly, while the verbatim `body` is never overwritten (Q24). Egress is a conscious, disclosed, opt-in choice. Since FR-FBR-40 the same provider has a **second, unrelated consumer**: `translate_to()` translates the *team's* outbound status notes and public replies into the submitter's language for one email — a different string, persisted nowhere (§ Two consumers below).

## 1. Purpose & Responsibilities

Implements **FR-FBR-30** under **DEC-FBR-IMPL-25** (data/processing model) and **DEC-FBR-IMPL-26** (provider posture):

- **Provider abstraction** — a `Send + Sync` trait decouples the worker from the concrete backend, mirroring the `email::Mailer` precedent. `main.rs::build_translation_provider()` env-selects the impl; the provider **defaults to `off`** (returns `None` → no worker, no egress).
- **Translate-after-accept worker** — a poll-loop (`spawn_translation_worker`, modelled on `spawn_voting_cache_refresh`) drains rows the submit path stamped `translation_status='pending'`, translates each off the request path, and writes `body_translated` + `source_lang` back. Tolerates per-row provider failure (marks `failed`, bounded retry).

The per-row storage + the four worklist repository methods (`claim_pending_translations` / `set_translation` / `mark_translation_skipped` / `mark_translation_failed`) live in `feedbackmonk-repository::feedback`; this module is the provider + worker only.

### Two consumers, two different strings (FR-FBR-30 vs FR-FBR-40)

The provider is built once and shared. What each consumer sends it is not the same kind of text, and conflating them is the mistake worth naming here:

| | **Inbound** (FR-FBR-30, `worker.rs`) | **Outbound** (FR-FBR-40, `translate_to`) |
|---|---|---|
| Whose words | the **submitter's** feedback body | the **team's** status note / public reply |
| Into what | the canonical content language (English) | the **submitter's** own UI language |
| Trigger | a row stamped `pending`, drained off the request path | one email being sent |
| Stored | `feedback.body_translated` + `source_lang` | **nothing** |
| Gate | the provider being configured | the provider **and** `tenants.translate_outbound` (both default off) |
| Failure | row marked `failed`, bounded retry | the original text is sent, unchanged |

The outbound path reads and writes **no feedback column at all** — which is precisely why FR-FBR-40 leaves the Q24 read-isolation invariant (and the `translation-egress-q24-isolation` oracle's Probe B allowlist) untouched. A future change that makes the outbound path read the stored translation is not an optimisation; it is a Q24 violation.

Locale → provider code goes through `provider_target_code()`, which reads the C34 table's `deepl` column. Five locales (`ga`, `fa`, `ml`, `is`, `si`) have no target code and are English by design — `translate_to` returns `Ok(None)`, never an error.

## 2. File Index

| File | One-line summary |
|------|---|
| `mod.rs` | Module surface — `TranslationProvider` trait + `TranslateOutput` + re-exports, plus the FR-FBR-40 outbound helpers `provider_target_code(Locale)` and `translate_to(provider, text, target)`. |
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

```rust
// email/send.rs (FR-FBR-40): translate ONE outbound, team-authored string.
// Ok(None) = "send the original" and is a normal outcome, not a failure.
let mt: Option<String> = translate_to(provider, reason_note, submitter_locale).await?;
```

Tests substitute a `FakeTranslator` implementing `TranslationProvider` and drive `translate_once(provider, repo, target_lang)` directly (the trait is the test seam; the real DeepL call is non-deterministic and exercised only on demand). The outbound path's own decision function is `email::outbound_translation`, which takes the tenant opt-in as a `bool` so every branch is testable with no database and no network (`crates/feedbackmonk-api/tests/outbound_translation.rs`).

## 4. Constraints & Business Rules (load-bearing — never silently relax)

- **Provider DEFAULTS to `off`.** Enabling a cloud provider egresses feedback bodies (personal data) to a GDPR data processor (DEC-FBR-IMPL-26). Do NOT change the default away from `off`, remove the `off` option, or hardcode a provider without re-opening DEC-FBR-IMPL-26. Enforced by the `translation-egress-q24-isolation` oracle (Probe A) + disclosed in `docs/operations/SELFHOST_ENV.md` (Contract C21).
- **NEVER on the submit path.** Translation runs ONLY in the background worker, after a row is accepted (DEC-FBR-IMPL-25 D3). The submit path only STAMPS `pending`; it never calls a provider. Keeps the public widget endpoint's latency + uptime independent of the provider.
- **Store-both; the original is never overwritten.** The worker writes `body_translated` + `source_lang` alongside the verbatim `body` (Q24). `set_translation` is the ONLY writer of `body_translated`, and ONLY the worker calls it (enforced by the oracle, Probe C).
- **Public surfaces read the verbatim original.** The machine consumer `list_member_bodies_for_cluster` reads the translation (with fallback to the original), and the data-controller admin may view it via the ONE scoped reader `get_translation_for_admin` (the admin-UI original↔translation toggle, FR-FBR-30 #3). No **public / end-user / board** read may select `body_translated` (Q24; oracle Probe B allowlist).
- **Lazy backfill + manual escape hatch.** Only feedback accepted after a provider is enabled is translated. The operator endpoint `POST /api/v1/ops/translation/backfill` (behind `FEEDBACKMONK_OPS_TOKEN`) stamps pre-existing body-bearing rows `pending` so the worker picks them up (FR-FBR-30 #5).
- **Provider outage is never user-visible.** A failed translation marks the row `failed` (re-pollable until the attempts cap); every consumer falls back to the verbatim `body` while a row is un-translated.
- **No body in logs.** The worker logs only `feedback_id` + error on failure; the DeepL adapter never logs the response body verbatim. The outbound path logs `feedback_id` + the error and never the note or reply text.
- **Outbound translation never blocks a send (FR-FBR-40).** Provider absent, tenant opted out, no submitter locale, an English recipient, a locale with no provider code, a provider error, an empty or echoed response — every one of them sends the original email unchanged. `outbound_translation` returns `Option`, not `Result`, so there is nothing for a caller to `?` into a dropped notification.
- **Outbound translation is opt-in TWICE.** The provider defaults `off` (deployment), and `tenants.translate_outbound` defaults `false` (tenant, migration `00032`). Neither default moves without re-opening DEC-FBR-IMPL-26.

## 5. Relationships & Dependencies

- **Consumes** `feedbackmonk_repository::FeedbackRepo` (the four worklist methods + the `TranslationFlag` enablement flag), and `feedbackmonk_i18n::Locale` for the outbound target mapping.
- **Consumed by** `email::send::LettreEmailNotifier` (FR-FBR-40), which holds an `Option<Arc<dyn TranslationProvider>>` set via `with_translator` and reads `TenantRepo::get_translate_outbound` per send.
- **Constructed by** `main.rs::build_translation_provider()` + spawned beside the voting-cache tick.
- **Config** via `FEEDBACKMONK_TRANSLATION_*` env vars (`docs/operations/SELFHOST_ENV.md` Contract C21).
- **Guarded by** the `translation-egress-q24-isolation` Verification Oracle (`.claude/oracles/`).
- **FTS**: migration 00019 repoints `body_tsv` to `coalesce(body_translated, body)` — no code in this module touches FTS (the generated column does it).

## 6. Decision Log

- **Worker reads a process-global enablement flag (not an AppState field).** The submit-time `pending` stamp happens inside the repository INSERT, and the repository crate can't depend on this crate where the provider is built. A repo-crate `AtomicBool` (`TranslationFlag`) keeps the stamp decision co-located with the INSERT and avoids threading a bool through every submit signature + ~15 test call-sites (the plan's Deferred Decision D1 — OnceLock recommendation; resolved here as a repo-crate global to honour both "stamp in the repo" and "zero signature churn").
- **DeepL for v1, default off; local engine deferred.** DeepL is EU-based, offers a DPA, and does not train on Pro-API text — the privacy-preferred cloud choice (DEC-FBR-IMPL-26). A local no-egress engine (LibreTranslate the likely AGPL candidate) is future work.
- **Bounded retry via an attempts counter, not a time backoff.** `failed` rows are re-claimed until `translation_attempts` reaches the worker cap, then terminal. Tolerates a transient blip without a backoff-timestamp column or a tight retry loop on a persistent outage (plan Deferred Decision D2).
- **Global (unscoped) worklist claim.** Translation is tenant-agnostic transport keyed on `feedback.id`; the worker drains a global queue rather than looping per project (plan Deferred Decision D3). The four worklist methods are documented pre-auth exceptions in the `multi-tenant-isolation-check` allowlist.
