# Execution Plan
**Source**: /0-uldf-ldis-plan
**Generated**: 2026-06-21T16:00:22
**Task**: feedbackmonk — FR-FBR-30 multilingual feedback translation
**Strategy**: SEQUENTIAL (dependency-ordered, single focused agent)
**Spec Source**: docs/specs/SPECIFICATION.md FR-FBR-30; docs/specs/DECISIONS.md DEC-FBR-IMPL-25/26
**Ideation**: docs/planning/ideations/20260621T144557-non-english-feedback-translation.md

---

## Ratified decisions (DO NOT relitigate)

Carried verbatim from DEC-FBR-IMPL-25/26 — settled, off the table:
store-both (verbatim `body` never overwritten — Q24-safe) · async translate-after-accept
(never on the public submit path) · English canonical target (v1) · universal / not tier-gated ·
lazy backfill (pre-existing rows untouched) · provider pluggable, **default OFF** · DeepL (cloud,
EU) + off for v1, local engine deferred · FTS indexes the translation · egress disclosed (C21).

---

## Strategy Rationale

**SEQUENTIAL**, not PARALLEL/PODS. Collaboration Value Assessment:
- Specialization 2 / Quality 2 / Discovery 2 / Speed 2 = **8/20**
- Friction: Boundary Clarity 2 (low — the five work units all touch `feedback.rs` repo and `main.rs`),
  Coupling 2 (high — strict A→B→C→D dependency: the job needs the provider trait *and* the new repo
  methods *and* the columns) = **4/10**
- Net ≈ 8 − 2 = **6**, but the boundary/coupling reality dominates: the streams cannot be cleanly owned
  in parallel (shared files + compile-order dependency). Scope is MEDIUM and fits one effective context.

**CTD-01 predicate: FALSE** — does not exceed one context, does not genuinely parallelize (units are
sequenced, not independent), shared-file contention. → traditional sequential, **no CTD Plan section**
(empty CTD plan is part of the design). CTD-07 short-circuit does not apply (>2–3 files), but the
predicate failing on coupling routes to sequential regardless.

**Build order is the plan**: A (schema+repo) → B (provider) → C (job) → D (consumer wiring) → E (config/docs/oracle).

---

## Context Budget

Single agent, MEDIUM scope. Estimate: migration + repo methods (~6 files), new translation module
(~3 files), main.rs/state wiring (~2 files), one consumer SQL change, docs+oracle (~3 files). ~14–18
files, well within one context window. **Pass** — no decomposition needed.

---

## Component Breakdown (5 sequential streams)

### Stream A — Schema + repository (migration 00019)
**Files**: `migrations/00019_feedback_translation.sql` (new), `crates/feedbackmonk-repository/src/feedback.rs`, `.sqlx/` (regen).

Migration 00019:
1. `ADD COLUMN body_translated TEXT NULL` (no length check needed; provider output).
2. `ADD COLUMN source_lang TEXT NULL` (BCP-47 / ISO-639-1, e.g. `de`, `pt-BR`; provider-detected).
3. `ADD COLUMN translation_status TEXT NULL CHECK (translation_status IS NULL OR translation_status IN ('pending','translated','skipped','failed'))`.
   - NULL = pre-feature row or sentiment-only (lazy-backfill: never touched).
   - `pending` = stamped at submit when provider enabled + body present.
   - `translated` = job wrote `body_translated` + `source_lang`.
   - `skipped` = source already English (no translation needed).
   - `failed` = provider error after bounded retries (re-pollable).
4. **Repoint FTS** (the ratified "FTS indexes the translation"): `DROP INDEX feedback_body_tsv_idx; ALTER TABLE feedback DROP COLUMN body_tsv;` then re-add
   `body_tsv tsvector GENERATED ALWAYS AS (to_tsvector('english', coalesce(body_translated, body))) STORED;`
   and re-create the GIN index. Because it's `GENERATED ALWAYS`, when the job UPDATEs `body_translated`
   the vector auto-recomputes from the translation — **zero extra maintenance**, mirrors 00011's
   no-UPDATE-trigger convention. Untranslated rows fall back to `english(body)` = today's behavior.
5. Index for the job's claim query: `CREATE INDEX feedback_translation_pending_idx ON feedback (id) WHERE translation_status = 'pending';` (partial — tiny).

Repo methods (additive):
- `submit_authenticated` / `submit_anonymous` (lines 460-554): stamp `translation_status='pending'`
  when translation enabled **and** body is non-empty; else leave NULL. (Enablement signal — see Deferred Decision D1.)
- `claim_pending_translations(scope, limit) -> Vec<{id, body}>` — SELECT pending rows (tenant/project-scoped per DEC-FBR-03 if scoping applies; the worker iterates all projects like the voting-cache loop — confirm scope shape in impl).
- `set_translation(id, translated, source_lang)` — UPDATE body_translated/source_lang, status='translated'.
- `mark_translation_skipped(id)` / `mark_translation_failed(id)`.

### Stream B — Provider abstraction
**Files**: `crates/feedbackmonk-api/src/translation/mod.rs` (new, + `deepl.rs`, `noop.rs`).

Mirror the **`Mailer` trait + `build_mailer()`** precedent exactly (`email/mod.rs:29`, `main.rs:272`):
```rust
#[async_trait]
pub trait TranslationProvider: Send + Sync {
    /// Translate to target_lang. Returns (translated_text, detected_source_lang).
    /// detected_source == target_lang signals "no translation needed" (caller marks 'skipped').
    async fn translate(&self, text: &str, target_lang: &str) -> anyhow::Result<TranslateOutput>;
}
```
- `DeepLTranslator` — `reqwest 0.12` (rustls) POST to `api.deepl.com` (or `api-free.deepl.com`); DeepL
  **auto-detects** the source language and returns it, so **language detection comes free** — no separate
  detector dependency. Key from `FEEDBACKMONK_TRANSLATION_DEEPL_API_KEY`.
- `NoOpTranslator` — default; the loop is not even spawned when provider=off, but the type exists for
  the `Arc<dyn TranslationProvider>` contract.
- `build_translation_provider()` in `main.rs` (~line 305, beside `build_mailer`): reads
  `FEEDBACKMONK_TRANSLATION_PROVIDER` (default `off`), returns `Option<Arc<dyn TranslationProvider>>`
  (`None` when off → no spawn).

### Stream C — Async translate-after-accept worker
**Files**: `crates/feedbackmonk-api/src/lib.rs` (spawn fn, beside `spawn_voting_cache_refresh`), `crates/feedbackmonk-api/src/main.rs` (spawn site after line 97).

`spawn_translation_worker(provider, pool/repo, target_lang, interval)`:
- Spawned in `main.rs` **only when `build_translation_provider()` returned `Some`** (provider≠off) —
  exactly the `let _tick = spawn(...)` voting-cache pattern (JoinHandle intentionally not held).
- Loop (every N s, default ~15s, `FEEDBACKMONK_TRANSLATION_POLL_SECS`): `claim_pending_translations(batch)`
  → per row: `provider.translate(body, target)` → if `detected==target` → `mark_skipped`; else
  `set_translation`; on error → `mark_failed` (bounded retry: failed rows re-eligible after backoff, or
  left failed — confirm retry policy in impl). Tolerates per-row failure (voting-cache + clustering-best-effort precedent).
- **Never blocks or touches the submit request path** (D-XLATE-3).

### Stream D — Consumer wiring (machine consumers only)
**Files**: `crates/feedbackmonk-repository/src/feedback.rs` (one query).

- `list_member_bodies_for_cluster` (repo 974-1002, analyst grounding via `work_orders.rs:1072`):
  change SELECT `body` → `coalesce(NULLIF(body_translated,''), body)` so the P5b analyst/clustering
  reads the English translation with fallback to original. Shape unchanged (`Vec<String>`).
- **FTS**: no code change — handled entirely by Stream A's generated-column repoint.
- **Everything else unchanged**: end-user `/me/feedback`, public board, admin list/detail/moderation
  display, sentiment_trend all keep the verbatim original (Q24 + display authenticity). Sentiment is
  never computed from body, so no wiring.

### Stream E — Config, disclosure, oracle
**Files**: `docs/operations/SELFHOST_ENV.md` (C21), `.claude/oracles/translation-egress-q24-isolation/` (new), `crates/feedbackmonk-core/README.md` or module README (privacy invariant doc).

- SELFHOST_ENV.md: append a **"Translation Provider"** subsection (table rows, source citations):
  `FEEDBACKMONK_TRANSLATION_PROVIDER` (optional, default `off`), `FEEDBACKMONK_TRANSLATION_DEEPL_API_KEY`
  (optional*, 🔒, required iff provider=deepl), `FEEDBACKMONK_TRANSLATION_POLL_SECS` (optional, default 15),
  `FEEDBACKMONK_TRANSLATION_TARGET_LANG` (optional, default `EN`). Disclose the egress per DEC-FBR-IMPL-26.
- Verification Oracle (see Oracle Pre-Build Plan).

---

## Ripple Analysis

| Modified interface | Consumers | Impact | Mitigation |
|---|---|---|---|
| `feedback` table: drop+re-add `body_tsv`, +3 columns | `.sqlx` offline cache; `search_for_admin` (references `body_tsv` in WHERE) | `body_tsv` keeps same name/type → query still compiles; behavior improves (now coalesce-sourced) | Regen `.sqlx`; oracle revalidates |
| `list_member_bodies_for_cluster` SQL | `work_orders.rs:1072` → analyst `deep_read`/`ingest` | behavioral: returns translated when present; `Vec<String>` shape unchanged | runner/analyst tests get fixture rows with translations |
| new repo methods | none (additive) | none | — |
| translation-enabled signal at submit | submit handlers + (maybe) AppState/test fixtures | see Deferred Decision D1 | OnceLock recommended → zero fixture churn |

**Blast radius: 🟡 Medium** — additive schema + one behavioral SQL change + new module; no breaking interface change, no public/Q24 surface touched.

---

## Testability Gate Findings

Two items flagged (composite-driven + fidelity):

1. **DeepL adapter (Stream B)** — Q2 fidelity risk high: an external HTTP translation call is
   non-deterministic and can't be unit-tested against the real API in CI. **Scaffolding (Q4):** the
   `TranslationProvider` trait *is* the seam — pair with a **fake provider** (`FakeTranslator` returning
   canned `(text, source_lang)` per input) for the worker + consumer integration tests. **Drift detection
   (Q5):** a thin live smoke against DeepL behind `--full`/ignored-by-default (like the oracle `--full`
   probes), so CI stays deterministic but the real adapter is exercised on demand.
2. **Privacy / Q24 read-isolation (Streams D/E)** — Q2: the load-bearing invariant is that **no
   public/end-user/board read ever returns `body_translated`** and the **async job is the only writer**.
   This is exactly the moderation-gate oracle's shape. **Scaffolding + drift:** the Verification Oracle
   below (detection-from-code, not a self-reported flag).

Iteration cost (Q1) moderate (cargo test + sqlx); critical path (Q3) Stream A blocks B/C/D.

---

## Oracle Pre-Build Plan

| Oracle | Question | Consumer(s) | Timing | Status |
|---|---|---|---|---|
| `translation-egress-q24-isolation` | (1) provider defaults OFF + never wildcard/hardcoded-cloud; (2) NO public/end-user/board/admin-display read SQL selects `body_translated` (Q24 + privacy); (3) the async worker is the ONLY writer of `body_translated`; (4) FTS `body_tsv` sources from `coalesce(body_translated, body)` | implementer + finalize gate | build during Stream E (the verifiability surface for the privacy posture) | not yet built |

**Rationale**: this is the anti-reward-hacking leg for DEC-FBR-IMPL-26's privacy posture and the Q24
invariant — detection-from-code (parse the read SQL + the provider construction), mirroring the live
`public-board-moderation-gate` / `cors-allowlist-enforcement` oracles. Single-agent build, but the payoff
(a permanent guard that a future edit can't silently route translated text to a public surface or flip the
default to a cloud provider) clearly justifies it.

**Deferrals**: none for v1. (Bilingual FTS — indexing original *and* translation for native-language admin
search — was evaluated and deferred per DEC-FBR-IMPL-25: v1 indexes the translation only.)

---

## Deferred Decisions (resolve at implementation, recommendation attached)

- **D1 — translation-enabled signal at submit.** Submit must decide whether to stamp `'pending'`.
  **Recommend: a module-level `OnceLock<bool>`/`AtomicBool` in `translation::`** set once at startup from
  the same env read that builds the provider, read by the submit handlers — **zero AppState field churn**
  (avoids the ~9-test-fixture ripple the solicitation work, DEC-FBR-IMPL-24, explicitly flagged). Alternative:
  a `translation_enabled: bool` on AppState (more discoverable, but touches every AppState literal).
- **D2 — failed-row retry policy.** Recommend bounded re-poll (e.g. `failed` rows re-eligible after a
  backoff window via `last_attempt_at`), or simplest-v1: leave `failed`, surface a count, manual re-trigger.
  Pick during Stream C.
- **D3 — worker scope shape.** Voting-cache iterates all projects; confirm whether the claim query is
  global or per-project-looped (DEC-FBR-03 scoping). Recommend global claim with tenant/project carried on
  each row (translation is tenant-agnostic transport; no cross-tenant read).

---

## Interface Contracts

- **Repo ↔ worker**: `claim_pending_translations(limit) -> Vec<PendingTranslation{ id, body }>`;
  `set_translation(id, translated: &str, source_lang: &str)`; `mark_translation_skipped(id)`;
  `mark_translation_failed(id)`. Worker never holds a DB txn across a provider call.
- **Worker ↔ provider**: `TranslationProvider::translate(text, target) -> TranslateOutput{ translated, detected_source_lang }`. `detected_source_lang == target` ⇒ worker calls `mark_skipped` (no write of body_translated).
- **Submit ↔ status**: handler stamps `translation_status='pending'` iff `translation::is_enabled()` && body non-empty; never calls the provider inline.
- **Analyst read**: `list_member_bodies_for_cluster` returns `coalesce(NULLIF(body_translated,''), body)` per row.

---

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Migration drops/re-adds `body_tsv` → index rebuild on large tables | One-time; partial pending-index is tiny; document in migration header |
| Provider outage stalls translations | Async + `failed` status + fallback-to-original everywhere ⇒ no user-facing failure; rows re-pollable |
| A future edit routes `body_translated` to a public surface (Q24 breach) | `translation-egress-q24-isolation` oracle (detection-from-code) blocks it at finalize |
| `.sqlx` cache staleness after column change | Regen + commit `.sqlx/`; existing `multi-tenant-isolation-check` + new oracle revalidate |
| AppState churn across 9 test fixtures | D1 recommendation (OnceLock) sidesteps it |

---

## Execution Commands

- **Recommended**: `/0-uldf-proceed` — picks topology (likely HANDOFF to a fresh implementation session
  given current context, on this LTADS-adopted project → `/0-uldf-ltads-start`).
- **Explicit sequential**: `/0-uldf-ltads-start` (this plan auto-resolves as the latest).
