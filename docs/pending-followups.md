# Pending Follow-Ups — ARCHIVE (closed 2026-09-07)

> **⚠️ This is history. Do not read it as a to-do list, and do not add to it.**
>
> **Then**: through 2026-09-07 this project's `CLAUDE.md` § Pending Follow-Ups carried the full body of
> every deferred item — 20,494 bytes of that file's 41,023, auto-loaded into every session in this repo.
> Most of it narrated finished work: three completed renames and a rename fixup, the org registration,
> the public-board voting build, the a11y login fix, and the shipped half of the UI-localization arc.
>
> **Now**: the live to-do list is **`CLAUDE.md` § Pending Follow-Ups** — one line per entry, trigger
> first, with any longer body in `docs/pending/` and anything larger filed as a DEFER brief under
> `docs/planning/deferred/`. Everything the 2026-09-07 cut removed is preserved verbatim below, in
> fuller form, unedited.
>
> **Why this file is kept rather than pruned**: it is the record of why several still-live constraints
> exist — the identifier-stability rule, the pin on `stranded-dirty-files`, the four items stacked on
> one Railway redeploy, and the exact contract surfaces Phase A delivered.
>
> Nothing below is guaranteed current. Verify against the system, never against an entry's own claim.

---

## Pending Follow-Ups

- **UI localization (FR-FBR-34..40) — SHIPPED 2026-09-07 (Stages 1 + 2). Two owner actions remain.**
  Widget, public board/roadmap/tenant-host pages, all emails **and the full admin console** render in the
  user's language (31 locales, DEC-FBR-15); outbound team-authored replies/status notes are machine-translated
  into the submitter's language, opt-in per tenant and off by default at two independent levels (FR-FBR-40).
  `feedback.submitter_locale` + `tenants.locale` + `tenants.translate_outbound` (migrations 00031/00032, no
  new migration in Stage 2). Five oracles live; `i18n-literal-ratchet` baseline is **0** (was 191).
  FR-FBR-41 (marketing site) stays DEFERRED until `feedbackmonk.com` is live.

  **① OWNER ACTION — run `/1-translate` before the next release.** No translation has ever run, by design
  (DEC-FBR-17): every non-`en` catalog is a skeleton and every surface falls back to English per key.
  `translation-gap-status` now reads **30 locales · 16,500 missing · 0 drifted · ~471k chars · DUE** (up from
  3,525 — Stage 2 added the whole admin namespace). This is the release gate; nothing else waits on it.

  **② RESOLVED 2026-09-07 — `lang` now states the language the words are IN.** Five locales
  (`fa, ga, ml, is, si`) have no MT provider, so their catalogs are English *permanently* and
  `/1-translate` will never fill them; `fa` is also the only RTL locale shipped. Declaring
  `<html lang="fa">` over English words made a screen reader read English with Persian phonology.
  On the owner's word, every runtime now declares `lang="en"` for those five while keeping the
  locale's `dir` — the visitor keeps the mirrored layout they chose. Keyed on the C34 table's
  `deepl === null` (the *cause* of the fallback), so it self-corrects if a provider ever covers one.
  FR-FBR-34 amended; `i18n/README.md` C35 rule 2 carries the runtime consequence.

  **Widget headroom is now 689 B** of the 30,720 cap (was 884 B; Stage 2 spent 197 B on a locale-gate
  prototype-chain fix, a restored 5xx error message and the A-2 content-language fix). No further widget bytes without spending the
  documented `widget.`-prefix lever (~735 B, `widget/README.md`).

  **Dev DB**: resolved — `feedbackmonk_dev` was recreated on the owner's word 2026-09-07 and is back at
  **32/32**, so it is the normal `DATABASE_URL` target again. `feedbackmonk_prepare` is also 32/32 and stays
  a valid alternative for `cargo sqlx prepare`. `docs/operations/LOCAL_DEV.md` § Known state carries the
  detail, including what to do if the migration ledger is ever found wiped again.

  **GitCellar must re-vendor the whole `widget/dist/` tree** (`dist/locales/` is new, and `widget.js` moved
  again in Stage 2) — filed to that repo.
<!-- /0-uldf-schedule writes here -->

- **🚨 BLOCKED / RESUME HERE — Railway cannot create containers for `feedbackmonk-api`**: the
  won't-fix white-screen fix is built, merged, migrated and staged, but cannot ship. 78 of 79 prod
  feedback rows are `wontfix`, so the GitCellar triage inbox is ~99% unusable right now. **Do NOT
  change any env var / setting / image pin on that Railway service** — the only container serving
  `feedback.gitcellar.com` is irreplaceable while this persists. Full resume record:
  [`DEFER-009`](docs/planning/deferred/DEFER-009_railway-deploy-blocked-feedbackmonk-api.md) +
  `docs/planning/feedbackmonk-deploy-state.md` § Stage E.
- **Unpin stranded-dirty-files oracle — TRIGGER HAS FIRED (measured 2026-08-30)**: the synced baseline is clean, so the pin is now the only thing keeping this oracle off upstream fixes. Full detail in PF-UNPIN-01 below. Test (assembles the identifier at runtime — do NOT paste the literal back in, see DEFER-003): `U=$(id -un); grep -ciE "$U|$(printf %s "$U" | tr a-z A-Z | cut -c1-6)~1" ~/.claude/oracles/stranded-dirty-files/validate.ps1` -> `0`.

### PF-SAAS-STANDUP-01: provision `feedbackmonk.com` + migrate GitCellar onto it (DEC-FBR-14 ops half)

**Status (2026-08-30): BLOCKED ON THE OWNER — ops, not code.** FR-FBR-32/33 are complete and tested;
what is missing is somewhere to run them. Full runbook: `docs/operations/SAAS_HOSTING.md`.

Two items:

1. **Provision the deployment** — hosting account, `*.feedbackmonk.com` + `app.feedbackmonk.com` DNS,
   wildcard TLS (DNS-01 needs a Caddy build with the provider module + an API token), and
   `FEEDBACKMONK_ROOT_DOMAIN` / `FEEDBACKMONK_ADMIN_HOST` / `FEEDBACKMONK_TRUSTED_PROXY_HOPS=1`.
   **Recommendation: run it separately from GitCellar's Railway.** The vendor's SaaS living inside
   customer #1's infrastructure is precisely the arrangement DEC-FBR-14 exists to undo; reproducing it
   would leave the dogfooding gap where it was.
2. **Cut GitCellar over** — `SAAS_HOSTING.md` § 4, ordered and reversible. The GitCellar side of
   this is filed in that repo as **DEFER-084** (`feedbackmonk-saas-tenant-cutover`), which carries
   the DNS/data/decommission/doc work and the measured stacked-redeploy finding below. **Coordinate with the
   GitCellar side before touching DNS**: live GitCellar sessions exist on this machine and may be
   measuring against `feedback.gitcellar.com`. **No GitCellar source file is edited by any step** —
   `triage.gitcellar.com` keeps working as an operator-registered admin alias that 301s to the
   canonical admin host (DEC-FBR-IMPL-27), so `TRIAGE_URL` is never touched. A plan that requires
   editing it has misread DEC-FBR-14.

### PF-PHASEA-01: GitCellar feedback-consolidation contract build-out ("Phase A") — CODE DONE; DEPLOY (A6) is the remaining GATE

**Status (2026-07-01): all five Phase-A contract surfaces BUILT + verified locally (CI-parity green); crate → v0.3.0. The only remaining item is the deploy+verify GATE (A6), which is GitCellar-Railway ops, not this repo.** Phase A adds the end-user capabilities GitCellar's feedback consolidation depends on (its Phases B/C — delete the internal Cloud-API feedback backend, unify the two feedback screens — do NOT start until these are live + capability-verified on `feedback.gitcellar.com`). Source program: `../GitCellar/docs/planning/plans/20260701-feedback-consolidation-onto-feedbackmonk.md`. This repo's intake+plan: `docs/planning/intakes/20260701T160735-*.md` + `docs/planning/plans/20260701T161200-*.md`.

Delivered (all ADDITIVE to the frozen contract; each advertised via `GET /api/v1/capabilities`; contract doc `docs/integrations/gitcellar-adoption.md` updated §0/§5.5/§6.1/§6.2/§6.4/§6.5/§6.6/§8/§11 + change log):
- **A1 (P0) `DELETE …/me/feedback/{id}`** — hard-delete + FK cascade + **object-store attachment-byte purge** (byte purge BEFORE row delete); sub-scoped (404 cross-user). Capability `feedback.delete`. New Verification Oracle `feedback-erasure-completeness` (A/B/C GREEN).
- **A2 (P1) attachment list + tenant-scoped download** (upload pre-existed). `feedback.attachments`.
- **A3 (P1) `updated_at` + `reply_count` (+ `?since=`)** on the me/feedback list (public replies only). `feedback.reply_state`.
- **A4 first-class optional `severity`** (`low|medium|high|blocker`, migration `00020`) replacing the `external_metadata.severity` side-channel + **`Idempotency-Key`** submit dedupe (transactional exactly-once, migration `00021`). `feedback.severity` + `feedback.idempotency`.
- **A5 `GET …/me/feedback/export`** GDPR portability. `feedback.export`.

Decisions confirmed (were AFK-adopted, then user-confirmed): D-A1 hard-delete+byte-purge; D-A4 severity `low|medium|high|blocker` optional; D-A5 export included. Built at autopilot; implementation streams executed on the Fable model, coordinated/reviewed on Opus 4.8.

> **2026-09-01 — a FOURTH item now stacks on this redeploy, and this one is a live production
> defect.** `FeedbackStatus::WontFix` serialised as `wont-fix` (serde `rename_all = "kebab-case"`)
> while the DB CHECK, Contract C6 and every client status union use `wontfix`. Effect on the live
> admin at `triage.gitcellar.com`: any `wontfix` feedback rendered a blank status pill, and opening
> it white-screened the page (`LEGAL_TRANSITIONS["wont-fix"]` is `undefined` →
> `undefined.length` in `StatusControls`); `?status=wontfix` filtering and
> `to_status: "wontfix"` transitions were also rejected as an unknown variant. Fixed at HEAD
> (`#[serde(rename = "wontfix")]` + an all-six-variants JSON⇔DB round-trip test, mirroring
> `RoadmapItemStatus`, which already carried the rename; plus `?? []` / label-fallback hardening in
> `StatusControls` + `StatusBadge` so UI-vs-wire drift can never white-screen the admin again).
> **Only a redeploy clears it for the operator.**

> **Re-measured 2026-08-30 — still outstanding, and now THREE items stack on this one redeploy.**
> `curl -sS https://feedback.gitcellar.com/api/v1/capabilities` returns `"version":"0.2.0"` with 5
> capabilities (health/ready 200). Current code is **0.4.0 with 15**. Waiting on a redeploy of this
> single service: **A6** (≥0.3.0 + migrations 00020/00021 → the six Phase-A capabilities),
> **DEFER-004** (migration 00029 → `feedback.rating`), and **FR-FBR-32/33** (0.4.0 + migration 00030
> → `hosting.*`). The **DEC-FBR-14 cutover retires all three at once** — a SaaS instance runs current
> code with every migration applied — so weigh doing that instead of three separate Railway
> redeploys. If the cutover is far off, A6 still stands on its own merits: it gates GitCellar's
> Phases B/C. Filed to GitCellar as **DEFER-084**.

**Remaining — A6 deploy GATE (NOT this repo's code; still OUTSTANDING as of scrutiny 2026-07-01):** the live instance runs **v0.2.0** with migrations `00020`+`00021` unapplied, so the six Phase-A capabilities are not yet live there. Redeploy `feedback.gitcellar.com` at ≥ v0.3.0 with migrations `00020`+`00021` applied (GitCellar Railway — ordered runbook in `docs/operations/RAILWAY_GITCELLAR.md` § 8), then verify `GET https://feedback.gitcellar.com/api/v1/capabilities` advertises `feedback.delete|reply_state|export|severity|idempotency|attachments` and smoke each new route. That verification unblocks GitCellar Phases B/C. Cannot be performed from this repo/session (needs Railway access).

### ~~PF-BOARD-VOTING-01: Public-board voting (`feedback_board_votes`)~~ — DONE

**Status: DONE (2026-06-19, Contract C30).** Public-board voting is now fully wired, replacing the Stage 1 `vote_count = 0` placeholder. Implemented exactly per the pre-decided design (DEC-FBR-IMPL-21):

- **Migration** `00018_feedback_board_votes.sql` — NEW table keyed on `feedback_id` (mirrors `roadmap_votes`; `roadmap_votes` + `roadmap_voting_cache` left untouched).
- **Repo** `feedbackmonk-repository/src/board_votes.rs` (`BoardVoteRepo`: cast/retract/count/has_voted, 409-on-dup + retraction-window) + `feedback.rs` LEFT-JOIN `vote_count` aggregate (D1 — direct SQL, no cache) + `resolve_approved_board_feedback_id` (the D2 moderation gate).
- **API** `board.rs` `POST`/`DELETE /api/v1/projects/{id}/board/items/{short_code}/vote` (CORS-exposed). The anon/JWT voter chokepoint was extracted into the shared `handlers/voting_common.rs` (consumed by BOTH roadmap + board — migration 00007/00018 inv #2; roadmap behavior byte-identical, regression-tested). Moderation gate (D2): approved-only resolution before any write → vote/retract on pending/rejected/board-disabled returns 404 (no existence oracle).
- **Frontend** `PublicBoard.tsx` vote button (mirrors `PublicRoadmap.tsx`) + `castBoardVote`/`retractBoardVote` + `BoardVoteResponse`/`BoardRetractResponse` types.
- **Oracle** `public-board-moderation-gate` Probe B EXTENDED to the vote path (v1.1.0) — A/B/C GREEN.
- **Tests**: `board_vote.rs` (anon/JWT cast, 409, retract, 429) + `board_vote_moderation_gate.rs` (404 on pending/rejected/board-disabled/unknown) + `board_votes.rs` repo tests; roadmap-vote regression green; admin-ui vitest + board a11y green.
- **DEC-FBR-IMPL-22** (`projects.board_requires_moderation`) remains inert/out-of-scope — only meaningful if/when an auto-approve relaxation is built.

### ~~PF-RENAME-01: Cargo / env-var / package-name rename `feedbackr-*` → `feedbackmonk-*`~~ — DONE

Completed in a single atomic commit at the P1-finalize → P2-plan boundary. Scope delivered:
- Cargo workspace + all 6 member crate `[package].name` + every `[dependencies]` path reference
- Env var prefix `FEEDBACKR_` → `FEEDBACKMONK_` across code, scripts, docs, `.env.example`
- HTTP header constant `X-Feedbackr-Anon-Cookie` → `X-Feedbackmonk-Anon-Cookie` (`feedbackmonk-anon::ANON_COOKIE_HEADER`)
- Session cookie name `feedbackr_session` → `feedbackmonk_session`
- `admin-ui/package.json` name + Vite/CI db name (`feedbackr_test` / `feedbackr_dev` → `feedbackmonk_*`)
- `.sqlx/` offline cache regenerated and re-committed
- Both Verification Oracles GREEN after path updates (`multi-tenant-isolation-check` + `pii-scrub-audit`)
- Plan-file rename: `20260513T185711-feedbackr-v1-build-arc.md` → `…-feedbackmonk-v1-build-arc.md` (+ P0/P1 plan files)
- ID stability preserved: `DEC-FBR-*` and `FR-FBR-*` left untouched per DEC-FBR-11.

### ~~PF-RENAME-02: Working-directory rename `Apps\Feedbackr` → `Apps\feedbackmonk`~~ — DONE

Executed at the v1 arc-terminus (2026-05-14). Scope delivered:
- `Rename-Item "E:\Developer\SourceControlled\Apps\Feedbackr" "feedbackmonk"` (user-action; Windows blocks renaming a CWD, so executed after closing the last Claude session in the directory).
- `~/.claude/MACHINE_CONFIG.md` Dev Port Registry row path updated `Apps\Feedbackr` → `Apps\feedbackmonk` (port numbers + project name unchanged).
- Living docs path references updated in the same commit (CLAUDE.md banner, SPECIFICATION.md Repository home, ARCHITECTURE.md, PROJECT_TRAJECTORY.md Next-Best-Steps).
- Historical records left intact per DEC-FBR-11 identifier-stability rule (planning/intakes, commit-log, decision-record narrative, OPEN_QUESTIONS resolution narrative).
- No git remote existed at rename time (PF-REGISTER-01 still pending), so no remote-URL update required.

### ~~PF-RENAME-03: Local dev container rename `feedbackr-*-dev` → `feedbackmonk-*-dev`~~ — DONE

Executed 2026-05-15 post-arc-terminus. Scope delivered:
- `docker rename feedbackr-pg-dev feedbackmonk-pg-dev` (Postgres dev container on port 5433; `DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackmonk_dev` unchanged).
- `docker rename feedbackr-mailpit-dev feedbackmonk-mailpit-dev` (Mailpit SMTP-capture dev container on ports 1025/8025; ad-hoc dev container originally created during P1 status-emails work, not under `deploy/docker/docker-compose.yml` control).
- `ltads/execution/development-brief.md` constraint row updated to reflect new container name (the row had explicitly flagged the rename as a future item).
- `docs/operations/LOCAL_DEV.md` already prescribed `feedbackmonk-pg-dev` (updated in PF-RENAME-01); the rename brings live state into agreement with the doc.
- Concluded LTADS session records (`current-session.md`, `commit-log.md`, etc.) left intact per append-only history rule — they correctly describe the container name as it was during the concluded session.
- Stale gitignored routing artifacts cleaned up: `.claude/handoff/handoff-*.md` (14 unpinned files referencing dead `crates/feedbackr-*` paths) and `.claude/session-state/finalize-session-files-S001-*.json` / `-S002-*.json` / `-p4-stage1.json` (per-session caches referencing pre-rename paths). All gitignored — local hygiene only, no commit churn.

### ~~Documentation rename fixup (PF-RENAME-FIXUP)~~ — DONE

Executed 2026-05-15 in commit `b73a7b4`. Fixed two categories of issues introduced by PF-RENAME-02's path-rename sweep:
- **Over-rename** (6 fixes): historical "Feedbackr"/`github.com/Feedbackr`/`FEEDBACKR_*` references in `README.md`, `DECISIONS.md` DEC-FBR-11, `OPEN_QUESTIONS.md` Q9 had been corrupted to `feedbackmonk`/`github.com/feedbackmonk`/`FEEDBACKMONK_*`, inverting the meaning of the squat-contingency narrative.
- **Stale forward-references** (6 fixes): `feedbackr.com` → `feedbackmonk.com` (public roadmap URL in DECISIONS.md, Cloudflare deploy landing, scope-table row 16, P4 exit-gate line in arc plan); `feedbackr-tier-quotas` oracle name → `feedbackmonk-tier-quotas` (SPECIFICATION.md); planned P3 webhook signing headers `x-feedbackr-*` → `x-feedbackmonk-*` (DISCOVERIES.md D-FBR-07).

### ~~PF-REGISTER-01: Register `github.com/feedbackmonk` org + buy `feedbackmonk.com`~~ — DONE

Completed by user action (verified 2026-06-02 via `gh api`):
- `github.com/feedbackmonk` org registered 2026-05-16.
- Public repo `github.com/feedbackmonk/feedbackmonk` created (public, default branch `main`) and pushed — last push 2026-05-17; local `main` (`5bf9878`) in sync with `origin/main` (ahead 0 / behind 0). The `origin` remote is configured locally.
- `feedbackmonk.com` purchased.
- **Effect**: the first-public-push gate is cleared. `/0-uldf-finalize` no longer needs `--skip-push`; normal propagation-consent rules apply.

### PF-DEPLOY-01: Stand up a reachable feedbackmonk instance for the GitCellar integration (decision + ops)

**Status (2026-06-03): decision MADE (self-host) and feedbackmonk-api DEPLOYED + LIVE (at v0.2.0).** The instance runs on GitCellar's Railway at `https://feedback.gitcellar.com` (`/health/ready` 200, verified live 2026-06-03; project `a1350be8-…`, tenant `triage@gitcellar.com`, anon submit verified, `FEEDBACKMONK_CORS_ORIGINS` set). **No feedbackmonk _feature-code_ work remains for the original integration** — the admin-ui re-auth gap that was the last feedbackmonk dev item is closed (`POST /api/v1/login`, DEC-FBR-IMPL-10). **Correction (scrutiny 2026-07-01, finding P1-9):** a deploy item DOES remain — the **Phase-A A6 redeploy**. The live instance runs **v0.2.0**; the current code is **v0.3.0** with migrations `00020`+`00021` **unapplied** on the live instance, so the six Phase-A capabilities (`feedback.delete|reply_state|export|severity|idempotency|attachments`) are NOT yet live and `GET /api/v1/capabilities` does not yet advertise them. A6 = redeploy `feedback.gitcellar.com` at ≥ v0.3.0 with those migrations applied, then verify `/api/v1/capabilities` + smoke each new route (see PF-PHASEA-01 and the ordered runbook in `docs/operations/RAILWAY_GITCELLAR.md` § 8). This is **GitCellar-Railway ops, not this repo's code** — no feedbackmonk source change is outstanding — but it is real remaining work and it GATES GitCellar Phases B/C. What's also left is **GitCellar-side ops, not this repo**: re-publish the gitcellar.com landing at launch (currently reverted to placeholder per user direction) + Stage 3 Desktop JWT cutover. **Authoritative resume record**: `docs/planning/feedbackmonk-deploy-state.md` (this repo's pointer) → GitCellar repo's `docs/planning/feedbackmonk-deploy-state.md` (commit `82eaf2ebea`) for the full IDs, WCM credential names, and re-publish command.

**Original trigger** (now fired): when wiring GitCellar (customer #1) to embed the feedbackmonk widget.

The two hosting models that were on the table (self-host was chosen):

- **Self-host (CHOSEN + EXECUTED)**: GitCellar runs the stack on its existing Railway (reusing its Postgres — `docs/operations/RAILWAY_GITCELLAR.md`), `docker compose up` for vanilla self-host (FR-FBR-17, smoke-tested to `/health/ready`). **Does NOT require `feedbackmonk.com` to be live.** Runbooks: `docs/operations/SELFHOST.md` + `SELFHOST_ENV.md`.
- **SaaS (not pursued for GitCellar)**: deploy feedbackmonk behind `api.feedbackmonk.com` + `cdn.feedbackmonk.com`; point `feedbackmonk.com` DNS at it. Only needed if feedbackmonk is later offered as a hosted service rather than self-hosted by GitCellar.

Integration handshake (all built + exercised): customer signs up → gets `project_id` → registers an Ed25519 **public** key (`POST /api/v1/projects/{id}/signing-keys`, Contract C4) → mints EdDSA JWTs (`sub`/`iat`/`exp`/`aud`=project_id; Contract C2) → embeds widget with `data-project-id` + `data-jwt`.

The separate Astro **marketing site** (`feedbackmonk.com` landing page, FR-FBR-16) is product marketing — not required for GitCellar's functional integration.

### ~~PF-A11Y-LOGIN-01: Fix pre-existing broken `admin-ui/e2e/a11y.spec.ts` login test~~ — DONE

**Status (2026-06-19): FIXED.** Two latent bugs in the login a11y spec, both test-harness-only (no app/board/moderation code touched):
- **Strict-mode locator collision** (the named bug): `getByLabel('Password')` matched **two** elements — the password input (`<label>Password</label>`) and the "Show password" toggle (`aria-label="Show password"`), since Playwright `getByLabel` is case-insensitive substring by default. Fix: `getByLabel('Password', { exact: true })`.
- **Stale fake-API mock** (surfaced once the locator was fixed): the mock routed `/auth/login`, but the app moved to `POST /api/v1/login` (DEC-FBR-IMPL-10), so the login POST fell through to the absent backend and `waitForURL('**/feedback')` timed out. Fix: mock matcher → `/\/api\/v1\/login$/`.

Full e2e a11y suite green: **13/13 passed**, including the previously-failing login spec and the two new board specs (`moderation-a11y.spec.ts`, `public-board-a11y.spec.ts`). No regressions.

### PF-UNPIN-01: Unpin `stranded-dirty-files` once ULDF DEFER-221 lands

**Trigger**: the framework baseline no longer ships the developer's Windows account name — i.e. ULDF DEFER-221 is fixed *and* synced to this machine. One-command test:
`U=$(id -un); grep -ciE "$U|$(printf %s "$U" | tr a-z A-Z | cut -c1-6)~1" ~/.claude/oracles/stranded-dirty-files/validate.ps1` → `0`.
**Measured 2026-08-30: it returns `0` — the trigger has FIRED.** The upstream ULDF fix has been synced to `~/.claude/`, so the pin should now be removed (it is a `.claude/` write, hence DEC-84-gated for a worker session — see DEFER-003).

> The command assembles both spellings from the live account rather than embedding either. DEFER-003 records that this repo's previous version of this very note re-published the identifier it was documenting the removal of — do not paste the literal back in.

**Why the pin exists**: commit `5bf9878` (2026-05-17) scrubbed that account name to `someuser` in `validate.ps1` lines 38-39, ahead of this repo's first public push. The framework baseline still carries the unscrubbed form, so a blanket `/0-uldf-migrate-oracles` refresh reverts it — **observed twice**: `d71c35a` (2026-08-06) and again during the DEC-405 refresh (`de297c3`, 2026-08-21). The oracle was refreshed to current baseline *first*, the scrub re-applied, then pinned via `.claude/oracles/stranded-dirty-files/.local-customized` — so it carries the DEC-405 InvariantCulture date fix and diverges from baseline by 2 comment lines only.

**Action when triggered**: delete `.claude/oracles/stranded-dirty-files/.local-customized`, then confirm a refresh no longer reverts the scrub. Leaving the pin after DEFER-221 lands is the real cost — it blocks every future framework fix to this oracle from reaching this project.

**Upstream**: `DEFER-221` in the ULDF repo (`docs/planning/deferred/`).

Remove this entry once the pin is removed.
