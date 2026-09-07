# feedbackmonk — Project Context for Claude Code

Project-specific context. The global ULDF framework guidance lives at `~/.claude/CLAUDE.md` and is the authoritative reference for framework commands, autonomy levels, propagation rules, and the agentic disciplines (Contexturgy, Oraculurgy, Probandurgy). This file ONLY documents what is specific to **feedbackmonk**.

> **Identifier stability**: the project was renamed Feedbackr → feedbackmonk on 2026-05-14 (DEC-FBR-11). Code, env vars (`FEEDBACKMONK_*`), crates, containers and paths are fully renamed; the identifier prefixes `DEC-FBR-*` and `FR-FBR-*` are **not**, and historical documents keep the old spelling deliberately — do not "fix" them. Full record: `docs/planning/20260907-claude-md-cut-archive.md`.

---

## What feedbackmonk is

Standalone open-source SaaS user-feedback platform: submission widget + status-workflow triage + public roadmap with voting + status emails. Multi-product per tenant.

- **Elevator pitch**: *Plausible Analytics for product feedback.*
- **License**: AGPL-3.0-or-later (see `LICENSE` — full canonical AGPL-3.0 text).
- **Stage**: v1 (P0–P4) shipped + deployed (self-host LIVE at `feedback.gitcellar.com`). P5a Stage 1 (Agentic Feedback Resolution Loop, recommend-only — FR-FBR-19/20/21/22 + FR-FBR-25 approval-gate leg) complete 2026-06-18; P5b (FR-FBR-23 implementer + FR-FBR-24 runner) upcoming.

## Read first (always, for any session in this repo)

| File | What it tells you |
|---|---|
| `docs/specs/SPECIFICATION.md` | 18 functional requirements (FR-FBR-01..18) across phases P0–P4 |
| `docs/specs/DECISIONS.md` | DEC-FBR-01..11 + DEC-FBR-IMPL-* — load-bearing context for every implementation choice |
| `docs/specs/ARCHITECTURE.md` | System architecture |
| `docs/planning/plans/20260513T185711-feedbackmonk-v1-build-arc.md` | Full build-arc plan: phase ordering, gates, interface contracts, Oracle Pre-Build Plan, Testability Gate findings |

The arc plan is the single most important downstream artifact — it pre-commits phase ordering and exit gates but defers intra-phase topology to each phase's own `/0-uldf-ldis-plan` round.

## Stack (load-bearing for tooling decisions)

- **Backend**: Rust. Reference implementation is `gitcellar-cloud/src/feedback/` in the peer GitCellar repo — read-only reference, NOT a base to extract from (DEC-FBR-07).
- **Admin UI / widget**: TypeScript. React for admin UI (P1, port pattern from `gitcellar-cloud/admin-ui/`); vanilla JS+CSS for the embeddable widget (P2, <30KB bundle cap per FR-FBR-04).
- **Marketing site**: Astro (P4 only).
- **Database**: PostgreSQL. Multi-tenant via `tenant_id` + `project_id` on every domain row; tenant-scoped repository layer is the **sole** query path (raw SQL is a security incident — DEC-FBR-03).
- **Distribution**: SaaS + self-host via `docker compose up` (P4).
- **Testing**: Vitest for unit tests, Playwright + axe-core for widget a11y (mandated by P2 Testability Gate finding).
- **Billing**: Polar (P3), pattern from GitCellar's existing setup.

## Dev Ports

- **Frontend**: `14204` (admin UI / React + Vite; bound with `strictPort: true`)
- **Backend API**: `14304` (`feedbackmonk-api` crate; default in `FEEDBACKMONK_PORT` env var)
- **Local Postgres**: `5433` (deconflicted from gitcellar-cloud's `5432`, per DEC-FBR-IMPL-04)

All registered in `~/.claude/MACHINE_CONFIG.md` Dev Port Registry. Local dev setup and the migration-ledger state: `docs/operations/LOCAL_DEV.md`.

## CI parity — run before every push (MANDATORY)

CI (`.github/workflows/ci.yml`) compiles **all targets** in **offline** mode
(`SQLX_OFFLINE=true cargo {build,test,clippy} --workspace --all-targets`, with
`RUSTFLAGS: -D warnings`). A plain `cargo build --workspace` or a targeted
`cargo test --test X` does **not** compile every test target and does **not**
deny warnings — so it silently skips the two failure classes that turn CI red:

1. **`sqlx::query!` in `tests/` with no cached metadata.** `cargo sqlx prepare
   --workspace` (no `-- --all-targets`) misses test-target queries → CI fails
   offline with *"no cached data for this query"*. **Always prepare with
   `cargo sqlx prepare --workspace -- --all-targets`** and commit `.sqlx/`.
2. **clippy/rustc warnings-as-errors** anywhere in the workspace.

**Before any `git push` of Rust changes, run the one-command gate** (it runs the
oracle + offline `clippy --all-targets -D warnings`, which compiles every target
and catches BOTH classes above):

```bash
bash scripts/ci-local.sh           # fast, DB-free (oracle + compile/lint gate)
bash scripts/ci-local.sh --tests   # also the suite (needs DATABASE_URL + Postgres)
```

(PowerShell: `pwsh scripts/ci-local.ps1 [-Tests]`.) Green here ⇒ green CI (modulo
runner-speed flakes). This is the gate `/0-uldf-finalize`'s push step depends on —
do not push without it.

## Git remote (GitHub, not local Gitea) — HTTP/1.1 required

Unlike most projects on this machine (which push to local Gitea), this repo's
`origin` is the real public remote `github.com/feedbackmonk/feedbackmonk`. Over
HTTP/2 the push transfer **hangs indefinitely** when run from an automated shell
(the Claude Bash tool), even though `fetch`/`ls-remote` succeed — it is not an
auth prompt, it is an HTTP/2 send-pack stall. Fix (already set repo-local in
`.git/config`, but `.git/config` isn't version-controlled, so re-apply if the repo
is re-cloned): `git config http.version HTTP/1.1`. With that set, `git push`
completes normally from any context. (2026-07-12, P6 Stage 1 convergence.)

**Second obstacle — the credential helper order breaks automated pushes.** `credential.helper`
resolves to `manager` (Git Credential Manager) then `store`. GCM runs **first**, tries to prompt on
`/dev/tty`, and — with no TTY in the Claude Bash tool — **hard-errors the entire credential lookup**
(`failed to execute prompt script` → `fatal: could not read Username for 'https://github.com'`).
It does *not* fall through, so a perfectly valid credential sitting in `~/.git-credentials` is never
consulted. Symptom is identical to "not logged in", which is misleading: logging in again does not
help, because the stored credential was never the problem.

Workaround that works from an automated shell (one-shot, mutates nothing):

```bash
git -c credential.helper=store push
```

A persistent fix would reorder or drop the `manager` helper — that is a `git config` write, so it
needs the owner's explicit word (see § Propagation Operations); until then, use the `-c` form above.
Verified 2026-08-06: two plain `git push` attempts failed with valid stored credentials; the `-c`
form pushed `4e69262..d41d33a` immediately.

## Workflow

- Use `/0-uldf-ldis-plan "feedbackmonk P<N> — <Phase Name>"` at each phase boundary.
- Use `/0-uldf-proceed` at phase boundaries — let it pick HERE / HANDOFF / PODS topology based on context budget and work shape.
- LTADS is **active** in this repo (initialized during P0 Stage 1 auto-init via spec-presence detection).
- Per DEC-FBR-07, this repo is greenfield — there is no source-level dependency on GitCellar. Do NOT modify GitCellar code from this working tree.

## Oracles

`.claude/oracles/` holds the universal starter pack plus the project-specific Verification Oracles below. The session-start hook runs the every-session fast ones and emits an ORACLE BRIEFING — read it before investigating manually. Audit via `/0-uldf-oracle`.

Each oracle's own `oracle.json` + `README.md` is the authoritative record of its probes, version history and self-test. The table says only **what it defends**, so you can tell which one your change is about to trip. Pre-cut descriptions: `docs/planning/20260907-claude-md-cut-archive.md`.

| Oracle | What it defends |
|---|---|
| `multi-tenant-isolation-check` | the tenant-scoped repository layer is the sole query path (DEC-FBR-03) |
| `pii-scrub-audit` | submitter PII does not escape into logs or public surfaces |
| `widget-bundle-size` | widget page-load set ≤ 30,720 B, each lazy `dist/locales/<code>.js` ≤ 4,096 B, no third-party trackers (FR-FBR-04, DEC-FBR-02) |
| `tier-enforcement-status` | plan caps fire, free-tier footer, `tier_quotas()` shape (FR-FBR-14, C19) |
| `selfhost-compose-smoke` | `docker compose up` distribution + env-catalog SSOT `docs/operations/SELFHOST_ENV.md` (FR-FBR-17, C21) |
| `cors-allowlist-enforcement` | credentialed CORS stays wired into `build_app`, never wildcard (DEC-FBR-IMPL-09) |
| `approval-gate-enforcement` | no work order reaches ≥ `dispatched` without a prior owner-authored `approved` event (FR-FBR-25a/22) |
| `public-board-moderation-gate` | no public board **read or vote** touches a non-`approved` row; board wire shape leaks no PII (C30) |
| `host-tenant-binding` | on a host bound to tenant T, no public route reaches another tenant; admin on exactly one host (DEC-FBR-13) |
| `translation-egress-q24-isolation` | translation provider defaults `off`; no public read of `body_translated` (DEC-FBR-IMPL-25/26) |
| `i18n-catalog-integrity` | catalog shape + generated locale tables (C35, C41) — the exit gate of every localization stage |
| `i18n-literal-ratchet` | **baseline 0** — any new hard-coded user-facing literal in `widget/src` or `admin-ui/src` is a hard failure |
| `translation-gap-status` | advisory only: how much translation is outstanding; finalize reports it, never blocks |

`ls .claude/oracles/` is authoritative — it also carries oracles not listed above (`feedback-erasure-completeness`, `public-route-ceiling`, `submission-idempotency`, …).

## Constraints not in spec artifacts

- **The repo is public** at `github.com/feedbackmonk/feedbackmonk` (AGPL-3.0-or-later, full canonical text). `feedbackmonk.com` is purchased but not yet pointed at a running deployment. Nothing pushed here is private — scrub before you commit.
- **GitCellar is customer #1, not a dependency.** Never modify GitCellar code from this working tree; the peer repo is a read-only reference (DEC-FBR-07). Cross-repo work is filed as a DEFER brief in that repo.

## Privacy invariants (load-bearing — never silently relax)

- **No third-party trackers in the widget, ever** (no Segment, Mixpanel, GA, Intercom). DEC-FBR-02 brand promise.
- **JWT customer signs is the ONLY identity feedbackmonk ever has** for an end-user (DEC-FBR-04). No callbacks to customer auth providers; no long-lived bearer tokens.
- **Q24 invariant** (FR-FBR-12, P2): roadmap items promoted from feedback contain the feedback body verbatim with NO submitter attribution and NO FB-ID reference. Port the byte-for-byte unit test from GitCellar's `roadmap_promote.rs` — same test name, same assertions. Document as untouchable in the module README.

## Pending Follow-Ups

**A to-do list, not a changelog.** One line per entry, ≤400 bytes, trigger first: `- **Trigger: <when>** — <what to do>`. The action's completion IS the removal condition, and **you delete the line in the commit that discharges it** — never append a progress note, rewrite or remove. A body longer than a line goes in `docs/pending/<slug>.md` behind a `` Details: `…` `` pointer; anything bigger is a piece of work, so file it in `docs/planning/deferred/` as a DEFER brief and leave a pointer here. Finished-work narration belongs in git history and `docs/specs/DECISIONS.md`; every entry written before 2026-09-07 is archived verbatim in `docs/pending-followups.md`.

- **Trigger: 🚨 BLOCKED — resume here** — Railway cannot create containers for `feedbackmonk-api`, so the wontfix white-screen fix cannot ship and GitCellar's triage inbox is ~99% unusable. **Change no env var, setting or image pin on that service** — the one container serving `feedback.gitcellar.com` is irreplaceable. Details: `docs/planning/deferred/DEFER-009_*.md`
- **Trigger: before the next release** — run `/1-translate`. No translation has ever run, by design (DEC-FBR-17): every non-`en` catalog is a skeleton and every surface falls back to English per key. `translation-gap-status` reads 30 locales · 16,500 missing · ~471k chars · **DUE**. This is the release gate; nothing else waits on it.
- **Trigger: any `widget/src` change** — only 689 B of headroom under the 30,720 B cap. More bytes need the documented `widget.`-prefix lever (~735 B, `widget/README.md`), or `widget-bundle-size` goes red.
- **Trigger: any `widget/dist/` change** — GitCellar must re-vendor the whole tree, not just `widget.js`; `dist/locales/` is new and the entry point has moved twice. Filed to that repo.
- **Trigger: the one redeploy of `feedback.gitcellar.com`** — the live instance runs v0.2.0 / 5 capabilities; HEAD is v0.4.0 / 15. Four items stack on that single redeploy (Phase-A A6, DEFER-004, FR-FBR-32/33, the `wontfix` serde fix). Details: `docs/pending/gitcellar-instance-redeploy.md`
- **Trigger: your word — ops, not code** — provision `feedbackmonk.com` and cut GitCellar onto it (DEC-FBR-14). FR-FBR-32/33 are built and tested; there is nowhere to run them, and this retires the redeploy stack above at once. Details: `docs/pending/saas-standup.md`
- **Trigger: fired 2026-08-30 — just do it** — unpin the `stranded-dirty-files` oracle (delete `.claude/oracles/stranded-dirty-files/.local-customized`); the pin is now the only thing keeping upstream framework fixes off it. Details: `docs/pending/unpin-stranded-dirty-files.md`
- **Trigger: `feedbackmonk.com` is live** — FR-FBR-41 (Astro marketing site) is DEFERRED until then.
<!-- /0-uldf-schedule writes here -->

---

## License footer

feedbackmonk is AGPL-3.0-or-later. Contributors agree via DCO sign-off (no CLA per DEC-FBR-05). Self-host customers receive identical releases to SaaS; there is no proprietary fork.
