# Execution Plan — UI localization, 31 locales (FR-FBR-34 … FR-FBR-40)

**Source**: /0-uldf-ldis-plan
**Generated**: 2026-09-06T21:37:06Z
**Task**: feedbackmonk — UI localization, 31 locales (FR-FBR-34..40, phases L0–L4)
**Strategy**: STAGED — a short serial Task-Zero stage, then PARALLEL (PODS) inside each stage, CTD-tiered
**Intake Source**: none for this lane — the spec session of 2026-09-06 is the upstream artifact (`docs/specs/SPECIFICATION.md` § Capability extension — UI localization; ideation `docs/planning/ideations/20260906T120000-ui-localization-31-locales.md`)
**Decisions**: DEC-FBR-15/16/17 · DEC-FBR-IMPL-30/31 (all RESOLVED 2026-09-06)
**Autonomy**: autopilot:director (machine default) · **Owner direction** (2026-09-06): *"be the LD and leverage CTD"*
**SPODS**: not eligible (judgment mid-flight in every unit; see § SPODS)

---

## The one sentence that matters

Every human-facing string moves into one catalog tree with English as the source and **per-key
English fallback at runtime**, so the product is shippable in all 31 locales *before* a single word
is translated — and the translation pass itself is a release step that runs only on the owner's word.

## Sequencing constraints (from the spec)

1. **Contracts before code.** C34 (locale table + resolver) and C35 (catalog format) are frozen in
   Stage 0 because every worker consumes them; nothing else is shared.
2. **End-user surfaces before the admin console.** Widget, public pages and emails ship in Stage 1;
   the admin string extraction (the largest count) is Stage 2, after the i18next bootstrap exists.
3. **Content stays verbatim.** No public surface reads `body_translated` (Q24; the
   `translation-egress-q24-isolation` oracle is the tripwire). This plan localizes *chrome* only.
4. **No translation in this arc.** The arc ends with `en` complete and 30 catalogs empty-but-valid.
   `/1-translate` is invoked by the owner before a release, never by a worker or finalize (DEC-FBR-17).
5. **Schema changes are named here** (safety rail): migrations `00031_submitter_locale.sql` and
   `00032_tenant_locale.sql`, both additive nullable columns. The owner's "defaults … begin
   implementation" instruction is the consent; no worker adds any other migration.

---

## Strategy Rationale — Collaboration Value Assessment

**Scope**: LARGE (five surfaces, three runtimes, one new crate, two migrations, tooling, four oracle
changes). Clears `SCOPE_GATE`; ≥ 3 separable units (`MIN_PARALLEL_UNITS`).

**Friction check**
- *Subdivisible?* **Yes** — the natural seams are the three runtimes (widget / SPA / Rust) plus the
  tooling, each with one owner. The one shared artifact — the catalog — is made collision-free by
  splitting it **per namespace file** (`i18n/locales/<code>/<ns>.json`, a plan-level refinement of
  C35): the widget worker owns `widget.json`, the SPA worker `public.json` + `admin.json` +
  `status.json`, the backend worker `email.json`. No two workers write one file.
- *Spec stable?* **Yes** — all six open questions resolved; contracts C34–C37 frozen in the spec.
- *Coupling low?* **Yes** — the cross-worker surface is enumerable up front: C34/C35 (Stage 0,
  read-only afterwards), C37 payload field (widget→backend, one optional string), C38 admin locale
  endpoint (SPA→backend, one GET/PUT). Everything else is private to a worker.

**Value questions**: specialization (byte-capped vanilla-JS widget vs React/i18next vs Rust/sqlx vs
Python tooling are four different skill profiles) · cross-checking (the resolver is implemented twice,
TS and Rust, against one fixture file — a deliberate two-implementation check) · discovery (RTL and
CSP behaviour will be learned in the widget lane and must not block the others) · wall-clock (four
lanes in parallel vs ~4× serial).

**Verdict: STAGED (PODS inside stages)**. Stage 0 is serial by construction (the contracts). Stages
1 and 2 are PARALLEL.

## Context Budget Assessment

| Worker | Assigned scope | Sibling summaries | Contracts | Reserve | Estimate | Verdict |
|---|---|---|---|---|---|---|
| W-A widget | `widget/src/**` (~1.2k lines) + vite + oracle + e2e | 2 × 500 | C34–C37, C42 | 40% | ~140k | pass |
| W-B SPA | `admin-ui/src/{i18n,pages/board,pages/roadmap,pages/public,shared/format}.*` + bootstrap | 2 × 500 | C34, C35, C38 | 40% | ~180k | pass |
| W-C backend | new crate + 2 migrations + submit handler + email templates + settings handler | 2 × 500 | C34, C35, C37, C38, C40 | 40% | ~200k | pass |
| W-T tooling | `scripts/i18n/**`, `/1-translate`, 2 oracles | 1 × 500 | C35, C41 | 40% | ~120k | pass |
| W-D admin extraction (Stage 2) | `admin-ui/src/pages/**` except board/roadmap/public (~40 files) | 1 × 500 | C35 | 40% | ~250k | pass (mechanical, high volume) |
| W-E outbound translation (Stage 2) | `translation/`, `email/`, one migration column | 1 × 500 | C40 | 40% | ~120k | pass |

No unit exceeds 85% of any tier's effective capacity. No RSPD-03 gate fires: no sub-scope has ≥ 3
independent sub-units of its own — every component is a **fully-scoped leaf**.

## CTD Plan (Capability-Tiered Delegation — Principle 2.14)

**Predicate**: CTD-01 TRUE — genuinely_parallelizes (4 units in Stage 1, 2 + reviewers in Stage 2) AND has_verifiable_boundaries (every unit closes on a test suite + an oracle).
**Model tiering**: ON — chain-resolved from `~/.claude/config.json` (`modelTiering: true`, stance `cost-first`; project `.claude/config.json` is `{}`). Ladder: frontier = Fable 5.1, standard = Opus 5, cheap = Sonnet 5.

| Block | Scope / context est. | Tier | Cheaping preconditions (fit / contract / verifiable / horizon) | Rationale |
|---|---|---|---|---|
| decompose + contracts (this plan; Stage 0 Task Zero) | — | **frontier** | n/a (the root act) | contracts C34/C35 bound every downstream unit |
| W-A widget | ~140k | **standard** | ✓ / ✓ / ✓ / ✓ — UI module rewrite under a byte cap, ~40 strings + build config + e2e; comparable to the P2 widget work completed at this tier | not routine: 7 KB headroom, CSP `script-src 'self'`, lazy-chunk loading, RTL on injected DOM; oracle + a11y suite verify |
| W-B SPA public + bootstrap | ~180k | **standard** | ✓ / ✓ / ✓ / ✓ — i18next bootstrap + two page families + switcher; comparable to the FR-FBR-33 `HostingSettings` lane | shared bootstrap that Stage 2 builds on — a seam-adjacent leaf; cost-first tiebreak stays standard, not frontier |
| W-C backend | ~200k | **standard** | ✓ / ✓ / ✓ / ✓ — new crate + 2 migrations + handler + templates; comparable to FR-FBR-32 (host binding) which shipped at this tier | touches the public submit path and a PII-adjacent column; guarded by `multi-tenant-isolation-check` + the security review below, so frontier is not required |
| W-T tooling | ~120k | **cheap** | ✓ / ✓ / ✓ / ✓ within — script porting with a reference implementation (`../GitCellar/scripts/i18n/*.py`, read-only) and oracle self-tests as the verifier; ~4 scripts + 1 skill + 2 oracles, comparable to prior oracle-authoring tasks completed at Sonnet-class in this repo | mechanical, contract-bounded (C35/C41), independently verifiable by fixture catalogs; the oracle **assertion blocks are authored by the LD** (seam) before spawn |
| W-D admin extraction (Stage 2) | ~250k | **cheap** | ✓ / ✓ / ✓ / ✓ within — mechanical literal→key extraction across ~40 files with the bootstrap in place; verifier is the `i18n-literal-ratchet` oracle + vitest + Playwright; comparable to codemod-class tasks | high volume, low decision density; a miss is auto-caught (ratchet) and cheaply recovered |
| W-E outbound translation (Stage 2) | ~120k | **standard** | ✓ / ✓ / ✓ / ✓ — provider-trait extension + email rendering; comparable to the FR-FBR-30 follow-on batch | egress-bearing (DEC-FBR-IMPL-26); `translation-egress-q24-isolation` must stay green |
| R-SEC security review (Stage 2) | read-only | **frontier** | — | security-class leaf: `submitter_locale` never on a public surface, `?lang=` / `data-locale` injection, `localStorage` posture |
| R-A11Y accessibility review (Stage 2) | read-only | **standard** | ✓ | RTL, `lang`/`dir`, screen-reader announcement across three locales |
| assembly / converge | — | **frontier** | n/a | the seams are where quality is lost |

**Interface contracts**: C34–C42 (§ Interface Contracts below).

**Assembly plan**: Stage 0 (LD) → Stage 1 four workers → LD converge (oracles A–D + both test suites) → Stage 2 three workers + two reviewers → LD converge → finalize. **Assembler tier: frontier.**

**Seam row** (CTD-25, cost-first): **LD seat @ frontier for this arc** — the owner designated the LD explicitly ("be the LD"), and the Stage-0 Task Zero (contract authoring + oracle assertions) is frontier work the seat performs itself; a standard-floor seat would have to escalate at Stage 0, both convergences and every oracle-assertion edit — i.e. most of its own work. No `seam-judgment` events expected; if the seat is handed to a fresh session, that session inherits this designation.

**Economics** (CTD-15): tier-down saves ~2.2× blended on ~80% of arc tokens (frontier $10/$50 vs standard $5/$25 → 2× on the ~60% standard share; vs cheap $3/$15 → ~3.3× on the ~25% cheap share; ~15% stays frontier) vs overhead: moderate (four-lane coordination, two convergences, two fresh-context reviews); expected rework low (every lane is oracle- or suite-verified). **Verdict: worth it.**

## Oracle Pre-Build Plan

| Oracle | Question | Consumer(s) | Timing | Status |
|---|---|---|---|---|
| `i18n-catalog-integrity` | Every `i18n/locales/<code>/<ns>.json` parses; keys ⊆ `en`; placeholders match per key; CLDR plural suffixes present per locale; `_meta` present; no mojibake; generated tables equal `i18n/locales.json` | all workers, every commit | **assertion + skeleton before spawn (LD, Stage 0)**; probes implemented by W-T in Stage 1 | not yet built |
| `translation-gap-status` | MISSING + DRIFTED counts per locale/namespace — *is a translation pass due?* | owner (release gate), finalize (advisory), briefing | W-T, Stage 1 | not yet built |
| `i18n-literal-ratchet` | New hard-coded user-facing literal in `widget/src` or `admin-ui/src` beyond baseline? | W-D, converge, every later session | assertion by LD in Stage 0; baseline frozen at Stage-1 converge; implemented by W-T | not yet built |
| `widget-bundle-size` amendment | Cap measures the English page-load set; `dist/locales/*.js` ≤ 4,096 B each. **Required, not optional**: `oracle.py:83-100` uses `rglob("*")`, so a `dist/locales/` directory is counted today | W-A | W-A Task Zero | LIVE (amend) |

**Rationale**: four lanes and two convergences all ask "is the catalog still consistent?" — one
oracle answers it for everyone; the ratchet is the only thing that makes Stage 2's cheap-tier
extraction safe (a missed literal is caught mechanically, not by review).

**Deferrals**: none — every candidate the spec listed is scheduled.

---

## Execution Overview

```
Stage 0: Contracts + Task Zero (LD, serial, frontier)
├── i18n/locales.json + generator + resolver fixtures + catalog skeleton
├── oracle assertion blocks (i18n-catalog-integrity, i18n-literal-ratchet)
└── Sync: C34/C35/C41 frozen; skeleton committed; workers spawn

Stage 1: End-user surfaces + tooling (PODS, 4 workers)
├── W-A widget          (standard)  FR-FBR-35 + C36 + C37 client half + bundle oracle amendment
├── W-B SPA public      (standard)  FR-FBR-36 + i18next bootstrap + FR-FBR-38 settings page (against C38)
├── W-C backend         (standard)  FR-FBR-37 + C37 server half + C38 + migrations 00031/00032 + capabilities
├── W-T tooling         (cheap)     FR-FBR-39 scripts + /1-translate + two oracles
└── Sync: converge — ci-local green, oracles green, ratchet baseline frozen

Stage 2: Admin extraction + outbound translation + reviews (PODS, 3 workers + 2 reviewers)
├── W-D admin extraction (cheap)    FR-FBR-38 strings
├── W-E outbound reply MT (standard) FR-FBR-40
├── R-SEC (frontier, read-only) · R-A11Y (standard, read-only)
└── Sync: converge — finalize; spec rows CONFIRMED → DONE; DEFER filed to GitCellar for the widget re-sync

Release (outside this arc, owner's word): /1-translate → validate → commit → release
```

## Component Breakdown

### Stage 0 — Task Zero (LD, frontier, serial)

**Goal**: freeze what every worker consumes; nothing a worker could do differently.

1. `i18n/locales.json` — the 31-row table (C34): `code`, `name` (endonym), `dir`, `deepl` (target code or `null` for `ga, fa, ml, is, si`), `base` (for `pt-BR`→`pt` etc.). Copied from `../GitCellar/apps/gitcellar-landing/src/i18n/locale-map.ts` `url`/`name`/`dir`.
2. `scripts/i18n/gen-locales.py` (+ `.ps1/.sh` shims) → `admin-ui/src/i18n/locales.gen.ts`, `widget/src/locales.gen.ts`, `crates/feedbackmonk-i18n/src/locales.gen.rs` (C41). Generated files carry a `// GENERATED — do not edit` header and are committed.
3. `i18n/resolution-fixtures.json` — the resolver truth table (≥ 40 cases incl. the override table, `pt`/`zh` bare codes, `da`→unshipped, casing/underscore canonicalisation).
4. Catalog skeleton: `i18n/locales/en/{widget,public,admin,email,status}.json` each `{"_meta": {"language": "English", "status": "source"}}`; the 30 other locales get the same five files with `_meta.status: "untranslated"` (`"english-fallback — provider unsupported"` for the five). `i18n/source-hashes.json` = `{}`.
5. `i18n/README.md` — Contract C35 in prose (format, namespaces, plural suffixes, `_meta`, the source-hash sidecar, "developers edit `en/` only").
6. Oracle assertion blocks (`manifest.json` `assertion` for `i18n-catalog-integrity` and `i18n-literal-ratchet`), written before any probe exists.
7. `Cargo.toml` workspace member `crates/feedbackmonk-i18n` with an empty lib (so W-C and W-T never both touch the workspace file).

**Exit**: commit `chore(i18n): Stage 0 — locale table, generator, catalog skeleton, oracle assertions`; `scripts/ci-local.sh` green; `python scripts/i18n/gen-locales.py --check` exits 0.

### Stage 1 — W-A · Widget (FR-FBR-35) — standard

**Owns**: `widget/**` (all), `.claude/oracles/widget-bundle-size/**`, `i18n/locales/*/widget.json`.

**Tasks**
1. Task Zero: amend `widget-bundle-size` — Probe A sums only top-level `dist/*.{js,css}`; new Probe C caps each `dist/locales/*.js` at 4,096 B; adversarial self-test (a 5 KB `de.js` goes red). Update `manifest.json` `assertion` + README; never touch `SIZE_CAP_BYTES`.
2. `widget/src/i18n.ts`: `t(key, args?)` with `{{name}}` interpolation and `_one/_other` (+ `_few/_many/_zero`) plural selection via `Intl.PluralRules(locale)`; `resolveLocale(explicit?, htmlLang?, navigatorLanguages)` per C34 (port GitCellar's `locale-resolution.ts`; run the shared fixtures in vitest); `loadLocale(code)` → `import(\`./locales/${code}.js\`)` with the `en` map inlined; per-key fallback to `en`.
3. Extract every literal in `ui.ts`, `widget.ts`, `attachments.ts`, `redact.ts` into `i18n/locales/en/widget.json` (keys `widget.<area>.<name>`); build step slices `widget.*` per locale into `widget/src/locales/<code>.ts` (generated, gitignored) and Vite emits `dist/locales/<code>.js` (`chunkFileNames` already `[name].js`; add `manualChunks`/entry map so each locale is its own unhashed chunk).
4. `data-locale` attribute + `MountOptions.locale` (C36); set `lang` + `dir` on `.fbm-root`; RTL: audit `styles.css` for physical → logical properties (`margin-inline-start`, `inset-inline-end`, `text-align: start`).
5. Error handling: `ui.ts:346` renders `t('widget.error.' + code)` when the key exists, else `t('widget.error.generic')`; add codes for `invalid_input`, `network_error`, `http_4xx/5xx`, `payload_too_large`, `rate_limited`, `tier_cap`.
6. Submit payload: include `locale: <resolved code>` (C37).
7. Tests: vitest for `t()`/plurals/fallback + resolver fixtures; `widget/e2e/widget-a11y.spec.ts` matrix over `en`, `de`, `fa` (Playwright `locale`) asserting `lang`/`dir` on `.fbm-root`, the chunk request, and axe clean; a fixture page with `<meta http-equiv="Content-Security-Policy" content="script-src 'self'">` proving the lazy chunk loads under GitCellar's CSP (TGF-01).
8. `widget/README.md` + `docs/…/widget.astro`-adjacent embed docs: new attribute, new `dist/locales/` directory in the vendoring note.

**Done-means**: `widget-bundle-size` PASS (English set ≤ 30,720; every chunk ≤ 4,096); e2e matrix green; `en` renders byte-identical text to today (snapshot test).

### Stage 1 — W-B · SPA public surfaces + bootstrap + settings page (FR-FBR-36, FR-FBR-38 UI half) — standard

**Owns**: `admin-ui/src/i18n/**`, `admin-ui/src/pages/{board,roadmap,public}/**`, `admin-ui/src/shared/format.ts`, `admin-ui/src/pages/settings/LanguageSettings.tsx` (new), `admin-ui/src/App.tsx` (this stage only), `admin-ui/index.html`, `admin-ui/package.json`, `admin-ui/e2e/{public-board,public-roadmap}-a11y.spec.ts` + new `language-settings-a11y.spec.ts`, `i18n/locales/*/{public,status}.json`.

**Tasks**
1. Add `i18next` + `react-i18next`; `admin-ui/src/i18n/index.ts` bootstraps with `fallbackLng: 'en'`, `escapeValue: false`, per-locale **dynamic import** of `i18n/locales/<code>/{public,admin,status}.json` (Vite code-split); resolver = the TS implementation shared with the widget (copy `resolveLocale` into `admin-ui/src/i18n/resolve.ts` and run the same fixtures — one fixture file, two runtimes).
2. Locale source of truth in the SPA: `?lang=` → `localStorage['fbm_lang']` → (admin only) `tenants.locale` from C38 → `navigator.languages` → `en`. Set `document.documentElement.lang/dir` on change.
3. Extract `PublicBoard.tsx`, `PublicRoadmap.tsx`, `TenantHostLanding.tsx` strings into `public.json`; the 15 enum label maps: add `status.json` keys (`status.<value>`, `kind.<value>`, …) and a `useLabels()` hook; **leave the English constants in `types.gen.ts` in place** for Stage 2 (W-D deletes them and migrates the remaining consumers).
4. `LanguageSwitcher.tsx` (native `<select>`, endonyms from `locales.gen.ts`) in the public page chrome; persists `fbm_lang`.
5. `format.ts`: `formatRelative(iso, locale, now?)` / `formatAbsolute(iso, locale)`; update all 19 `Intl`/`toLocale*` call sites in owned files to pass the active locale (the D-FBR-31 fix). Unowned call sites (`SentimentTrendChart`, `TierSettings`, `UsageMeter`) are listed for W-D.
6. `LanguageSettings.tsx` at `/admin/settings/language` using C38 (`GET/PUT /api/v1/admin/settings/locale`), the `BoardSettings` query/mutation pattern; switching applies immediately.
7. e2e: public board + roadmap a11y specs run in `en`/`de`/`fa`; assert `<html lang/dir>`; `language-settings-a11y.spec.ts`.

**Done-means**: vitest + Playwright green; `translation-egress-q24-isolation` unchanged; no public-page string left in JSX (`i18n-literal-ratchet` baseline for `pages/{board,roadmap,public}` = 0).

### Stage 1 — W-C · Backend (FR-FBR-37, C37 server half, C38, migrations) — standard

**Owns**: `crates/feedbackmonk-i18n/**`, `migrations/00031_submitter_locale.sql`, `migrations/00032_tenant_locale.sql`, `crates/feedbackmonk-api/src/email/**`, `crates/feedbackmonk-api/src/handlers/{feedback.rs (submit only), capabilities.rs, tenant_settings.rs (new)}`, `crates/feedbackmonk-repository/src/{feedback.rs (insert fns + a `submitter_locale` read for admin), tenants.rs}`, `crates/feedbackmonk-core/src/models.rs` (`Feedback.submitter_locale`), `.sqlx/**`, `docs/operations/SELFHOST_ENV.md` (no new env expected — confirm), `i18n/locales/*/email.json`.

**Tasks**
1. `feedbackmonk-i18n` (C40): `Locale` newtype (`parse`, `EN`, `dir()`, `base()`), `resolve(candidates: &[&str]) -> Locale` (port the resolver; run `i18n/resolution-fixtures.json` in a Rust test), `t(locale, key)` / `t_args(locale, key, &[(&str,&str)])` over `include_str!`-embedded `email.json` + `status.json` per locale, fallback active → base → `en` → key. Also `parse_accept_language(&HeaderMap) -> Vec<String>` (q-value ordered; input capped at 200 bytes — GitCellar's DoS guard).
2. Migration `00031_submitter_locale.sql`: `ALTER TABLE feedback ADD COLUMN submitter_locale TEXT CHECK (submitter_locale IS NULL OR char_length(submitter_locale) BETWEEN 2 AND 35)`; comment: preference, not detection; admin-only.
3. Migration `00032_tenant_locale.sql`: `ALTER TABLE tenants ADD COLUMN locale TEXT CHECK (…same…)`, `ADD COLUMN translate_outbound BOOLEAN NOT NULL DEFAULT false` (reserved for W-E; comment says so).
4. Submit: `FeedbackRequest.locale: Option<String>`; resolution `payload.locale` → `Accept-Language` → `None`, validated against C34 (unknown → `None`, never 400 — the widget must not break on a locale we don't ship); thread through `submit_authenticated_full` / `submit_anonymous_full` (new trailing arg) into the INSERT. `multi-tenant-isolation-check` must stay green (tenant-scoped repo path only).
5. `templates.rs`: `render_*` gain a `locale: Locale` parameter; every literal moves to `email.json`; `status_human` → `t(locale, "status.<value>")`; C10 subject shape preserved byte-for-byte for `en` (snapshot tests for all three templates × `en`/`de` with the `de` catalog holding one translated key to prove per-key fallback). `send.rs::render_for_kind` resolves `submitter_locale` → `tenants.locale` → `EN`. Account emails in `mailpit.rs` builders take `Locale` (`tenants.locale` → `Accept-Language` at signup → `EN`).
6. C38: `GET/PUT /api/v1/admin/settings/locale` (`{ "locale": string | null }`; PUT validates against C34 → 400 `invalid_locale`), `TenantRepo::{get_locale,set_locale}` on the `get_subdomain/set_subdomain` pattern, behind `AdminSession` + `bind_admin_routes`.
7. `capabilities.rs`: append `"i18n.locales"` (+ a `i18n: { locales: [...] }` block listing C34 codes) and `"feedback.submitter_locale"`.
8. Admin read: `FeedbackDetailResponse.submitter_locale` (admin_feedback.rs) — **never** on `FeedbackListItem`'s public projection or any board/roadmap read.
9. `cargo sqlx prepare --workspace -- --all-targets`; `bash scripts/ci-local.sh --tests` green.

**Done-means**: all Rust suites green offline; `multi-tenant-isolation-check`, `pii-scrub-audit`, `translation-egress-q24-isolation`, `public-board-moderation-gate`, `host-tenant-binding` unchanged and green; new tests `tests/submit_locale.rs` (payload / header / unknown / absent) + `tests/email_locale.rs` + crate unit tests.

### Stage 1 — W-T · Tooling + oracles (FR-FBR-39) — cheap

**Owns**: `scripts/i18n/**` (except `gen-locales.py`, Stage 0), `.claude/skills/1-translate/**`, `.claude/oracles/{i18n-catalog-integrity,translation-gap-status,i18n-literal-ratchet}/**` (probes; assertions pre-authored), `i18n/source-hashes.json` semantics, `docs/operations/TRANSLATION.md` (new).

**Reference (read-only)**: `../GitCellar/scripts/check-translation-gaps.ps1`, `../GitCellar/scripts/i18n/{retranslate-drifted,complete-plural-categories,check-stripped-diacritics,check-untranslated-authored}.py`, `../GitCellar/scripts/validate-translations.ps1`, `../GitCellar/.claude/skills/1-translate/SKILL.md`. Port the behaviour; do not copy GitCellar-specific surfaces.

**Tasks**
1. `check-gaps.py` — MISSING (key in `en/<ns>.json`, absent in `<code>/<ns>.json`) + DRIFTED (SHA-256 of the `en` value ≠ `source-hashes.json[ns][key]`), `--json`, `--locale`, `--namespace`, `--update-baseline`. Exit 0 always (advisory) unless `--strict`.
2. `translate.py` — DeepL only in v1 (`DEEPL_API_KEY` env; `:fx` suffix → free host), quota preflight (`/usage`) that **refuses a run it cannot finish**, `ignore_tags` around `{{…}}` placeholders and a do-not-translate list, `ZH-HANT` mapping for `zh-HK`/`zh-TW`, `null`-deepl locales skipped with a printed reason, byte-for-byte JSON round-trip check before any write, `--dry-run`, `--locale`, `--namespace`, `--formality`. Writes `_meta.status: "machine-translated — community review welcome"` and refreshes `source-hashes.json` for the keys it wrote.
3. `validate.py` — the nine classes (missing/extra keys, placeholder mismatch, empty, mojibake, leaked entities, untranslated-authored, stripped diacritics, plural-category completeness, `_meta` presence). Exit 1 on the hard classes; warn on the soft ones.
4. `complete-plurals.py` — CLDR category completion by numeral instantiation (ru/uk/pl/cs/sk `_few/_many`; lv `_zero`), refusing to write when the numeral does not survive the round trip.
5. `/1-translate` skill: four phases (quota preflight → gap detection → translate → validate + baseline), **hard rule in the skill text: only the owner invokes it**; finalize/CI/hooks call `check-gaps.py` at most.
6. Oracles: implement the pre-authored assertions — `i18n-catalog-integrity` (Verification, Python canonical + shims, adversarial self-test), `translation-gap-status` (project-state, wraps `check-gaps.py --json`, prints the release-gate line), `i18n-literal-ratchet` (baseline file `i18n/literal-baseline.json`; scans JSX text nodes, `aria-label=`, `notify(`, `title=` in `admin-ui/src` and string literals in `widget/src` DOM-building calls; baseline only shrinks). Register all three in `.claude/oracles/INDEX.md` and the session-start briefing list.
7. `docs/operations/TRANSLATION.md`: the release step, the DeepL account/DPA note (DEC-FBR-IMPL-26 applies to *product* egress; catalog MT is developer tooling, still disclosed), the five-language fallback.
8. Tests: fixture catalogs under `scripts/i18n/tests/fixtures/` exercising every gap/validate class; `translate.py` tested against a mocked DeepL HTTP server (no live key in CI).

**Done-means**: `python .claude/oracles/i18n-catalog-integrity/oracle.py` PASS on the Stage-0 skeleton and RED on each fixture defect; `translation-gap-status` prints `30 locales · N missing · 0 drifted · translation pass: DUE`; `check-gaps.py --strict` red, `--strict` absent green.

### Stage 2 — W-D · Admin extraction (FR-FBR-38 strings) — cheap

**Owns**: `admin-ui/src/pages/**` except `board/`, `roadmap/`, `public/`, `settings/LanguageSettings.tsx`; `admin-ui/src/components/**`; `admin-ui/src/shared/types.gen.ts` label maps (delete after migrating consumers) and the generator that emits them; `i18n/locales/*/admin.json`; remaining `Intl` call sites (`SentimentTrendChart`, `TierSettings`, `UsageMeter`).

**Tasks**: extract every literal into `admin.json` (keys `admin.<page>.<name>`); migrate label-map consumers to `useLabels()`; delete the English constants from `types.gen.ts` and stop the generator emitting them; pass the active locale to the three remaining `Intl` sites; extend the a11y specs (`moderation`, `board-kanban`, `tier-settings`, `hosting-settings`, `autopilot`, `a11y`) with a `de` run; RTL check on the admin shell in `fa` (logical properties).
**Done-means**: `i18n-literal-ratchet` baseline for `admin-ui/src` = 0; vitest + all Playwright specs green; no `STATUS_LABELS`-style English constant remains.

### Stage 2 — W-E · Outbound reply translation (FR-FBR-40) — standard

**Owns**: `crates/feedbackmonk-api/src/translation/**` (trait extension only), `crates/feedbackmonk-api/src/email/{send.rs,templates.rs}` (the reply/status-note paths), `crates/feedbackmonk-repository/src/tenants.rs` (`translate_outbound` get/set), `handlers/tenant_settings.rs` (`PUT …/locale` gains `translate_outbound`), `LanguageSettings.tsx` (one checkbox — coordinate with W-D via the channel; W-D does not own that file).

**Tasks**: `TranslationProvider::translate_to(text, target: Locale)`; in `send.rs`, when `tenants.translate_outbound` and `submitter_locale` is set and ≠ `en`, translate the note/reply body, render MT **above** the original with `t(locale, "email.machine_translated")`; provider failure or `off` ⇒ send the original unchanged (never block); tests for on/off/failure/`en` submitter; `translation-egress-q24-isolation` Probe B must stay green (no new reader of `body_translated` — this path translates *outbound* text, not feedback bodies). Disclosure line added to `SELFHOST_ENV.md` under the existing provider entry.
**Done-means**: suites + oracle green; the tenant checkbox round-trips.

### Stage 2 — Reviews (read-only, fresh context)

- **R-SEC (frontier)**: `submitter_locale` reachable only via admin routes (grep every projection); `?lang=` and `data-locale` are validated against C34 before any use (no reflected/injected string reaches `lang`/`dir`/`import()`); the dynamic `import()` path is whitelisted to the generated code list (no template-string import of user input); `localStorage['fbm_lang']` value validated on read; `Accept-Language` parser bounded; verdict PASS / CONCERN / VETO into the channel.
- **R-A11Y (standard)**: screen-reader announcement of `lang` changes, `dir=rtl` in `fa` on widget + public pages + admin shell, focus order under RTL, axe across the locale matrix; files defects as dispatches to the owning worker.

## Testability Gate Findings

| ID | Item | Q1 | Q2 | Q3 | Q4 | Q5 | Composite | Flag | Recommendation |
|----|------|----|----|----|----|----|-----------|------|---------------|
| TGF-20260906T213706-01 | Widget lazy locale chunk under a host CSP `script-src 'self'` (GitCellar's landing) | 3 | 4 | 4 | 4 | 2 | 17 | Composite>12 | The only real consumer's CSP is not in this repo; a passing Playwright run against a permissive fixture proves nothing. Pair with a **CSP fixture page** (`widget/e2e/fixture-csp.html`, `<meta http-equiv="Content-Security-Policy" content="script-src 'self'">`) asserting the chunk loads and no CSP violation is logged (`page.on('console')`). Drift detection: the fixture is run by the same spec every commit. |
| TGF-20260906T213706-02 | RTL correctness for `fa` on injected widget DOM + public pages | 3 | 4 | 2 | 3 | 2 | 14 | Composite>12 | axe does not judge visual direction. Pair with (a) DOM assertions `dir="rtl"` on every root in the `fa` matrix run, (b) a stylelint rule (`plugin/use-logical-properties` or an equivalent grep oracle over `widget/src/styles.css` and `admin-ui/src/**/*.css`) so physical `margin-left`/`text-align: left` cannot return; R-A11Y does the one human-eye pass. |
| TGF-20260906T213706-03 | `translate.py` against the live DeepL API | 4 | 4 | 1 | 5 | 3 | 17 | Composite>12 | Cannot run in CI without a key and must not spend quota in tests. Pair with a **mocked DeepL server** fixture (recorded request/response pairs incl. `/usage` and a quota-exhausted reply) so preflight refusal, `ignore_tags`, `ZH-HANT` mapping and the round-trip safety check are all exercised offline. Drift detection: the first real `/1-translate` run is `--dry-run` first and its diff is reviewed by the owner (the release step already requires this). |

Q6–Q9: the widget and SPA lanes verify through Playwright + axe (Tier already declared by the existing a11y suites — no `TIER-SHORTFALL`); no input-diversity need (Q7); Q8 module footprint aligns with the three runtimes (new crate + new `i18n/` root — `MOD-OK` by construction, the seams are the module edges); Q9 observation surface is the DOM (`lang`/`dir` attributes, chunk network requests) — readable without a build cycle via Playwright.

## Ripple Analysis

| Modified interface | Consumers | Owner of the migration |
|---|---|---|
| Submit payload `FeedbackRequest` (+`locale`, optional) | widget `api.ts`; GitCellar Desktop native feedback panel (POSTs the same route — unaffected, field optional); `docs/integrations/gitcellar-adoption.md` § submit | W-C (docs), W-A (widget) |
| `submit_authenticated_full` / `submit_anonymous_full` (+ trailing arg) | handlers/feedback.rs only; tests under `crates/feedbackmonk-api/tests/*submit*` and repo tests | W-C |
| `render_confirmation` / `render_status_change` / `render_public_reply` (+ `Locale`) | `email/send.rs::render_for_kind` (3 sites); template snapshot tests | W-C |
| `format.ts` `formatRelative`/`formatAbsolute` (+ `locale`) | 19 call sites; those in W-B-owned files by W-B, the rest listed for W-D | W-B → W-D |
| `types.gen.ts` label maps | every admin page that renders a status/kind/… label | kept until W-D; W-D deletes |
| `WidgetConfig` | **unchanged** | — |
| Widget embed contract (C36: `data-locale`, `dist/locales/`) | GitCellar vendoring ×2 (`apps/gitcellar-landing/public/feedback/`, `forge-versions/current/custom/public/assets/feedback/`) — **cross-repo**, filed as an inject to GitCellar at Stage-1 converge, never edited from here | LD |
| `capabilities.rs` `CAPABILITIES` | `docs/integrations/gitcellar-adoption.md` § capabilities table | W-C |
| `.claude/oracles/widget-bundle-size` measured set | CLAUDE.md oracle table row; SPECIFICATION § Oracles row | W-A (oracle), LD (docs at converge) |

Blast radius: 🟡 Medium — every change is additive; the only behaviour change for an English visitor is none (snapshot-tested), and the only cross-repo effect is the vendoring re-sync.

## Interface Contracts

- **C34 — locale vocabulary + resolver** (spec): 31 codes; `i18n/locales.json` schema `[{code, name, dir, deepl, base}]`; resolver semantics + `i18n/resolution-fixtures.json` (`[{input: [..candidates..], expect: code|null}]`). Both runtimes must pass every fixture.
- **C35 — catalog format** (spec, refined here): **per-namespace files** `i18n/locales/<code>/<ns>.json`, `<ns> ∈ {widget, public, admin, email, status}`; nested JSON; keys dotted, camelCase leaves; `_meta: {language, status}` at the top of every file; `{{name}}` interpolation; plural suffixes `_one/_other/_few/_many/_zero`; `en` is the source; `i18n/source-hashes.json` = `{ "<ns>": { "<dotted.key>": "<sha256 of en value>" } }`.
- **C36 — widget embed** (spec): `data-locale="<C34 code>"`, `MountOptions.locale?: string`; `dist/locales/<code>.js` beside `widget.js`; `lang` + `dir` on `.fbm-root`.
- **C37 — submitter locale** (spec): payload `locale?: string`; server resolves payload → `Accept-Language` → `None`; unknown ⇒ `None` (never an error); column `feedback.submitter_locale`; exposed only on `FeedbackDetailResponse`.
- **C38 — admin locale endpoint** (new): `GET /api/v1/admin/settings/locale` → `200 { "locale": "de" | null, "translate_outbound": false }`; `PUT` same body (fields optional) → `200` echo; unknown code → `400 { code: "invalid_locale" }`; `AdminSession`, `bind_admin_routes`.
- **C39 — capabilities** (new): `"i18n.locales"` + `"feedback.submitter_locale"` strings; `i18n: { locales: ["en", …] }` block.
- **C40 — `feedbackmonk-i18n` crate API** (new): `pub struct Locale`; `Locale::parse(&str) -> Option<Locale>`; `Locale::EN`; `fn dir(&self) -> Dir`; `fn base(&self) -> Option<Locale>`; `pub fn resolve(candidates: &[&str]) -> Locale`; `pub fn t(locale: Locale, key: &str) -> Cow<'static, str>`; `pub fn t_args(locale: Locale, key: &str, args: &[(&str, &str)]) -> String`; `pub fn parse_accept_language(headers: &HeaderMap) -> Vec<String>`; `pub const LOCALES: &[LocaleEntry]` (generated).
- **C41 — generator** (new): `scripts/i18n/gen-locales.py [--check]` reads `i18n/locales.json`, writes the three `locales.gen.*` files; `--check` exits 1 on drift (the `i18n-catalog-integrity` oracle calls it).
- **C42 — widget locale chunk** (new): `export default { "widget.form.subject": "Betreff", … }` — flat, full dotted keys, only `widget.*`, minified ≤ 4,096 B; generated at build from `i18n/locales/<code>/widget.json`; missing keys omitted (runtime falls back to the inlined `en`).

## Coordination Requirements

- **Shared files (single owner each)**: `Cargo.toml` (Stage 0 adds the member; W-C only thereafter), `admin-ui/src/App.tsx` (W-B in Stage 1, W-D in Stage 2), `widget/src/types.ts` (W-A), `capabilities.rs` (W-C), `.claude/oracles/INDEX.md` (W-T), `docs/specs/**` + `CLAUDE.md` (LD at converge only), `i18n/locales.json` + `i18n/resolution-fixtures.json` (frozen after Stage 0 — a worker who needs a change posts to the channel; the LD edits).
- **Channel**: PODS `channels/`; C38 shape questions from W-B go to W-C; W-E's checkbox in `LanguageSettings.tsx` is a dispatch to W-D's owner boundary (W-E posts the exact JSX; W-D applies) — or, if W-D has already converged, W-E edits it.
- **Sync points**: Stage-0 commit (spawn); Stage-1 converge (LD runs `scripts/ci-local.sh --tests`, all oracles, both e2e suites; freezes the ratchet baseline; files the GitCellar inject); Stage-2 converge (finalize; spec rows → DONE; CLAUDE.md oracle table + Pending Follow-Ups entry for the release step).
- **Conflict resolution**: ownership map above; the LD never implements (PODS rule); disputes over a key name are resolved by C35's convention, not by discussion.

## Deferred Decisions

| Decision | Deferred to | Default if not revisited |
|---|---|---|
| Whether the five DeepL-unsupported locales appear in the switcher | first `/1-translate` run | listed (Q28 default) |
| `projects.default_locale` for boards whose tenant wants a non-browser default | after a tenant asks | none — browser wins |
| Optional `locale` claim in the end-user JWT (C2) | GitCellar's Desktop auth-mode integration | `data-locale` suffices |
| Automated key extraction (`i18next-parser`) | when the ratchet becomes annoying | manual keys + ratchet |

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Widget cap: `en` inline strings + `t()` + resolver push past 30,720 B | W-A measures first (Task Zero) and reports headroom in its first channel post; `t()` is ≤ 40 lines; the resolver's override table is a compact array; if still over, the resolver's override table moves into the `en` chunk-loader path and the kind-label map becomes catalog-only |
| CSP breaks lazy chunk on GitCellar (TGF-01) | CSP fixture in the e2e suite; same-origin `import()` is what `redact.js` already does under that CSP today |
| RTL regressions invisible to axe (TGF-02) | `dir` assertions + logical-properties lint + R-A11Y pass |
| Cheap-tier W-D misses literals | `i18n-literal-ratchet` baseline = 0 is the exit gate; a miss cannot converge |
| Cheap-tier W-T ports GitCellar-specific behaviour | the fixture catalogs under `scripts/i18n/tests/` are authored from C35, not from GitCellar's files; the reviewer at converge diffs the CLI surface against the spec row |
| Two resolver implementations drift | one fixture file, both suites; `i18n-catalog-integrity` fails if either suite skips it |
| `submitter_locale` leaks to a public surface | R-SEC grep of every projection; `public-board-moderation-gate` Probe B's no-PII wire scan extended with the new column name (one-line change, W-C) |
| `tenants.translate_outbound` egress surprise | default `false`; disclosed beside the provider entry; `translation-egress-q24-isolation` Probe A still asserts default-off provider |
| Someone runs `translate.py` in CI or finalize | the script refuses without `--i-am-the-owner` **or** an interactive TTY; the skill text and `TRANSLATION.md` say so; finalize calls `check-gaps.py` only |

## SPODS

Not eligible: every unit carries judgment mid-flight (byte-cap trade-offs, RTL, key naming inside a page, resolver edge cases) and the arc spans two convergences with restart survival needs. Classic PODS.

## Execution Commands

```bash
# Stage 0 — LD, in this (or the successor) session, before any spawn
python scripts/i18n/gen-locales.py --check        # after authoring the table
bash scripts/ci-local.sh                           # workspace member compiles

# Stage 1 — PODS
/0-uldf-pods-parallelize --from-ldis-plan=docs/planning/plans/20260906T213706-ui-localization-31-locales-fr-fbr-34-40.md
/0-uldf-pods-spawn-collaborator --all             # W-A/W-B/W-C standard, W-T cheap (per CTD table)
/0-uldf-pods-collab-sync                           # monitor
/0-uldf-pods-converge                              # Stage-1 converge

# Stage 2 — PODS (same registry, next wave)
/0-uldf-pods-spawn-collaborator W-D --model cheap
/0-uldf-pods-spawn-collaborator W-E --model standard
# reviewers spawned at Stage-2 converge: R-SEC --model frontier, R-A11Y --model standard
/0-uldf-pods-converge --finalize

# Release (owner only, later)
/1-translate
```

## Convergence notes — Stage 1 (collab-20260906-215851, 2026-09-07)

Outcome: all four lanes COMPLETE; CI-parity + tests green; 17/17 oracles; admin 208 vitest / 35 e2e; widget 93 vitest / 29 e2e; critic verdict in the archived convergence report. Deviations from this plan, all ruled by the LD in-session (messages MSG-001..008) and none changing a contract's public shape:

- **C35 rule 3 amended**: `status.json` is the shared-enum namespace with families `status|kind|sentiment|roadmapStatus` (MSG-001).
- **C40 note**: Rust `status_human` reads `email.status.value.<wire>` from `email.json` (sentence case, byte-identical to shipped emails), **not** `status.*` (title-case UI chips). Six deliberately duplicated keys (MSG-004).
- **C41 for the widget**: `widget/src/locales.gen.ts` is generated and policed but is **type-only** for the widget — the 31-row table is ~1.9 KB and unshakeable. The widget consumes `widget/src/locale-chunks.ts`, a build-time projection of the same `i18n/locales.json` (MSG-005). Final: 29,836 B / 30,720.
- **C42**: all 30 chunks are emitted from day one as empty maps so the output file set never changes at the first translation (MSG-002). Vite lib mode emits no chunks for a template-literal `import()`; the loader is a generated literal map (also the R-SEC whitelist).
- **C36**: `data-locale` resolves through the lenient resolver (embedder-trusted attribute; resolver is table-bounded); `validateLocale` (exact-only) gates `?lang=`, `localStorage` and stored tenant values (MSG-006).
- **`format.ts`** locale parameter shipped **optional** (ten unowned call sites); Stage 2 passes the active locale everywhere and flips it required.
- **Pre-existing finding routed to Stage 2**: `error.rs` emits `{"error": msg}` while `widget/src/types.ts` expects `{code, message}`; the widget maps HTTP status classes instead. Stage 2 backend lane adds an additive `code` field (precedent: C38's 400 body).
- **Dev DB**: `feedbackmonk_dev` was unrepairable by migration during Stage 1; **recreated on the owner's word 2026-09-07 and back at 32/32** (`LOCAL_DEV.md` § Known state). Stage 2 ran its Rust lanes against `feedbackmonk_prepare`, which is also 32/32.

**Stage 2 (W-D admin extraction, cheap) brief inputs** — from the lanes' `## Deltas for LEAD`:
1. `format.ts`: pass the active locale at `FeedbackList`, `FeedbackDrawer`(4), `ModerationQueue`, `AutopilotDigest`, `BoardCard`, `ClusterDetail`(2), `WorkOrderList`, `WorkOrderDetail`(2), `RunnerTokenCard`(3), `SentimentTrendChart`(2 + 5 `toLocaleString`), `TierSettings`(1), `UsageMeter`(4); then make the parameter required.
2. `admin.settings.language.*` keys for `LanguageSettings.tsx` (inline `[CLAUDE-B][NOTE]` marks the spot); W-E's `translate_outbound` checkbox attaches there.
3. Admin-only physical CSS → logical: `.search-box`, `.search-icon`, `.search-kbd`, `.feedback-table td`, `.drawer`, `.char-counter`, `.upgrade-prompt`, `.runner-tokens-security`, `.hosting-settings-page ol`, `.domain-table td`.
4. `admin-ui/src/pages/settings/README.md` consolidation (five pages, one documented).
5. `i18n-literal-ratchet` exit gate: baseline for `admin-ui/src` → 0 (191 today, incl. `AdminRoadmap.tsx` 11, `PromoteButton.tsx` 3).
6. Remove the English label constants from `types.gen.ts` and stop the generator emitting them; all consumers via `useLabels()`.

**Stage 2 (W-E outbound translation, standard) brief inputs**: `tenants.translate_outbound` + `TenantRepo` accessors exist; `send.rs` already resolves a `Locale`; add the additive `code` field on `ApiError` bodies + C39/adoption-doc line (see above).

## Convergence notes — Stage 2 (collab-20260907-034037, 2026-09-07)

Outcome: **FR-FBR-38 and FR-FBR-40 DONE**; five lanes COMPLETE; critic verdict **CONCERN, no VETO**
(17/0 oracles, 5/5 compositions). Post-remediation gate: `ci-local.sh --tests` green, 17/17 oracles,
admin 226 vitest / 54 e2e, widget 166 vitest / 29 e2e, `widget-bundle-size` 30,031 B / 30,720 (**689 B**) — final numbers, after the A-2 amendment below.

**Plan text this stage supersedes** (corrections, not drift):

- **`email.machine_translated` → `email.machineTranslated`.** The plan's § W-E spelling was snake_case;
  C35 rule 3 says camelCase leaves and every existing `email.*` leaf is camelCase. The shipped key is
  camelCase; this plan's earlier line is the stale one.
- **"stop the generator emitting them" (§ W-D task 6) describes machinery that does not exist.**
  `admin-ui/src/shared/types.gen.ts` is hand-rolled — its own header says so — despite the `.gen` name.
  The 14 `*_LABELS` constants were simply deleted; no generator changed.
- **`TranslationProvider` needed no trait change.** `translate(text, target_lang: &str)` already took the
  target; W-E added `provider_target_code(Locale)` + a `translate_to` helper over the existing trait.
- **The FR-FBR-40 gate is NOT `resolve_recipient_locale`** (§ W-E step 5 said the resolved recipient locale).
  That ladder's second rung is `tenants.locale` — the admin's own language, i.e. most likely the note's
  *source* language — so it would translate text into the language it was written in. The shipped gate keys
  on the **submitter's own captured locale** only.
- **A third worker was added** (W-F): the `_catalog.py` `OTHER_ONLY` correction the Stage-1 finalize
  surfaced, split out of W-D rather than folded in — different language, zero file overlap, and a CLDR
  correctness call under a two-way ratchet does not belong in a cheap-tier mechanical lane.
- **R-SEC was spawned early, not at converge.** The plan's reason for holding both reviewers was a moving
  tree; that applied to the *repo*, not to *each reviewer's subject*. The entire security surface belonged to
  W-E and W-F, both done, and W-D's remaining lane added no input path or sink. R-A11Y did wait — it needed
  W-D's RTL pass and `de` runs.

**LD rulings (GUIDE § 11 + channel), binding on later stages:**

- **R-1** — the four *shared* enum families stay in `status.json` (`include_str!`-compiled into the Rust
  binary for 31 locales); admin-only families live in `admin.json` under `admin.enum.<family>.<wireValue>`
  behind a new `useAdminLabels()`. W-D shipped **12** such families, two beyond the ten named (`moderationStatus`,
  `tokenLifecycle`) — an in-scope extension, since the alternative was leaving that enum text unlocalized.
- **R-2** — the `ApiError.code` vocabulary is chosen against the widget's existing keys, **except** that
  `forbidden` is distinct from `unauthorized`: `code` is a machine contract with an external consumer and
  collapsing 401/403 is a defect in a contract, whatever the widget renders.
- **R-4** — the Rust-vs-ICU plural divergence at n=0 (`fa`, `fr`, `pt-BR`, and `ga`/`is`/`si` more broadly)
  is OUT of scope and stays ratcheted; only the `OTHER_ONLY` catalog partition moved.
- **Q24 ruling on `dir="auto"`** — `PublicBoard.tsx`'s *"do not wrap `item.body` in anything"* guards
  **attribution**; `dir="auto"` is an attribute on the existing element, adds no wrapper and no
  submitter-derived value, and does not touch Q24. Exposing `submitter_locale` publicly to set an exact
  `lang` was considered and **rejected** (it is in `public-board-moderation-gate`'s `PII_FIELDS`).

**Reviews.** R-SEC: CONCERN (low) — `in`/index reads on generated tables walk the prototype chain, so
`constructor` passed the locale gate and blank-paged a public board. Fixed at the generator + 7 sites; the
ratchet test, written *before* the fix and against the defect class, found **two more holes than the review
traced**. R-A11Y: CONCERN, 8 findings — six fixed in-wave (`dir="auto"` on user content, language-change
announcement, an unmirrored glyph, a raw BCP-47 tag where an endonym belongs, a `tabIndex={-1}` blocking
keyboard access to show-password, a dangling `aria-controls`); **A-1** (`isAdminPath` missing `/feedback`,
which flipped the console's language *and direction* mid-session) fixed; **A-2 escalated to the owner** —
see below.

**RESOLVED on the owner's word, 2026-09-07 — A-2 (`lang` over permanently-English content).**
`fa, ga, ml, is, si` carry `_meta.status: "english-fallback — provider unsupported"`, so English is
their *shipped steady state*, not an interim one — and `fa` is the only RTL locale we ship, so we were
asserting `<html lang="fa">` over English words and a screen reader read English with Persian
phonology. `/1-translate` fixes 25 of 30 and never fixes these five. The LD recommended adopting
R-A11Y's fix but did not take it, because it qualifies FR-FBR-34's ratified *"every surface sets `lang`
and `dir` from the active locale"* and is therefore a spec amendment. **The owner approved it**; FR-FBR-34
is amended and the fix shipped in the same arc.

What shipped: `contentLanguageOf()` (SPA, `useLocale.ts`) and `contentLang()` (widget, `i18n.ts`) return
the language the TEXT is in; `applyDocumentLocale` and `createRoot` use it for `lang` and leave `dir` on
the chosen locale, so the mirrored layout the visitor selected survives. Keyed on the C34 table's
`deepl === null` rather than on a new generated flag or on the `_meta` string — that field is the *cause*
of the fallback, so the derivation self-corrects the day a provider covers Irish, with no second list to
maintain. The widget's build-time projection gained `ENGLISH_FALLBACK` beside `RTL` (both derived the same
way in `slice-locales.mjs`). Cost: **21 widget bytes**, 710 B → 689 B headroom, `widget-bundle-size` PASS.

Test evolution: eight test files re-pointed from `lang === "fa"` to `lang === "en"` with `dir === "rtl"`
retained (and newly *added* in two places, to pin that `dir` did not move), plus positive tests that the
other 26 locales still declare themselves. The two public-page e2e matrices gained an `expectSelected`
field — the failure exposed a real conflation in the old tests, which used one value for both the
`<html lang>` attribute and the language switcher's selected option; those are now different facts, and a
Persian visitor sees "فارسی" selected while `lang` honestly says `en`.

**Carried to `docs/planning/observations-ledger.md`** (7 lines this stage): the Q24 oracle's Probe B is a
substring proxy with an empty `assertion.asserts` and no self-test (renaming an identifier clears it);
`i18n-catalog-integrity` Probe D is **vacuous on this tree** — every non-`en` catalog is a 10-leaf skeleton
with zero plural keys, so it returns PASS identically with or without the `fa`/`tr` fix, whose only
falsifiability evidence is an out-of-tree temp mirror; the admin vitest suite flakes on a **cold** run
(i18next in every render pushes the slowest tests past the 5 s default; CI does not run vitest); the SPA has
**no React error boundary** anywhere; stray `.claude/session-state/` dirs inside `admin-ui/`; four
`admin-ui/src` directories are modules with no README and `module-index` is structurally blind to that; and a
CSI registry-write race that erased a worker entry and self-recorded.

**Jig candidate** (critic advisory): W-F hand-built a throwaway `i18n/` mirror twice — once to demonstrate
Probe D's teeth, once for its own red-first proofs. The fixture-mirror/sandbox archetype would have replaced
both and left a `docs/falsifiability/` receipt as a side effect.
