# Oracle Index

This is the project's oracle catalog. Agents: scan this file to find an oracle that answers your question before investigating manually.

Read `README.md` in this directory for conventions, manifest schema, and authoring guidance. Read `FOUNDATIONS/ORACULURGY_DESIGN.md` for the full conceptual framework.

---

## Universal Starter Set

These oracles apply to nearly any ULDF project. They are installed by `/0-uldf-setup-project` and activate automatically at session start (hook-injected briefing).

### environment

| Oracle | Question | Strategy | Est. savings/call |
|---|---|---|---|
| **`project-type`** | What language, framework, and build system does this project use? | always-fresh | ~400 tokens |
| **`ui-surface-detector`** | Does this project have a UI / runtime surface that ARIA could instrument, and what kind? | trigger-invalidate (`package.json`/`Cargo.toml`/`pubspec.yaml`) | ~600 tokens |
| **`project-runtime-state`** | Does this project have live dev servers, shared build artifacts, file watchers, or stateful runtimes that would conflict under PODS worktree isolation? *(WT-05 — heuristic-only consumer at `/0-uldf-pods-parallelize` Step 6 (WT-06); not surfaced at session-start; lineage from DEC-61, 2026-05-10)* | always-fresh | ~600 tokens |

### git

| Oracle | Question | Strategy | Est. savings/call |
|---|---|---|---|
| **`git-state`** | Current branch, uncommitted counts, last commit | always-fresh | ~300 tokens |
| **`recent-activity`** | Last N commits, touched areas, commit cadence | always-fresh | ~500 tokens |

### spec

| Oracle | Question | Strategy | Est. savings/call |
|---|---|---|---|
| **`spec-status`** | Which spec items are done, pending, or removed? | trigger-invalidate | ~800 tokens |

### module

| Oracle | Question | Strategy | Est. savings/call |
|---|---|---|---|
| **`module-index`** | Module list with purpose, README status, compliance | trigger-invalidate | ~1500 tokens |
| **`module-tree-map`** | Hierarchical synopsis triage tree across the project (HCT-03 — load-bearing mechanism for breadth-first agent triage at log(n) cost). Sibling of `module-index`; distinct question (hierarchical, not flat). | trigger-invalidate (`**/README.md`) | ~4000 tokens |

### followup

| Oracle | Question | Strategy | Est. savings/call |
|---|---|---|---|
| **`pending-followups`** | CLAUDE.md pending follow-ups, overdue flagged | always-fresh | ~200 tokens |

### ltads

| Oracle | Question | Strategy | Est. savings/call |
|---|---|---|---|
| **`ltads-state`** | LTADS session state: permanent/temporary/legacy, id, status | always-fresh | ~300 tokens |

### workflow

| Oracle | Question | Strategy | Est. savings/call |
|---|---|---|---|
| **`workflow-position`** | Where is this project in the LDIS/LTADS workflow, and what is the next `/0-uldf-proceed` step? | always-fresh | ~500 tokens |
| **`dispatchable-sessions`** | What live sibling sessions can THIS session dispatch work to right now? *(also `--gc-cheap` and `--gc` modes for CSI-05 registry hygiene sweeps; closes DISC-PRO-05's REGISTRY-GC-01)* | always-fresh | ~400 tokens |
| **`aria-status`** | What is the ARIA instrumentation status of this project (surface present? endpoints reachable? foundation-layer healthy?) | trigger-invalidate (`.claude/aria.json`/manifests) + live probe (≤300ms) | ~800 tokens |
| **`gitignore-template-drift`** | Does this project's `.gitignore` lack any framework-managed patterns from the current `claude-template` baseline? *(HYGIENE-03 — emits a session-start nudge to run `/0-uldf-migrate-hygiene`; empty briefing on no-drift; lineage from DISC-HYGIENE-01)* | trigger-invalidate (`.gitignore` / `**/.claude/.gitignore`) | ~300 tokens |
| **`stranded-dirty-files`** | Which dirty files in this project have no live owner and predate the most-recent-finalize boundary? *(CSI-15, Phase 1.7 — visibility-only; emits a session-start nudge to run `/0-uldf-finalize --include-stranded`; empty briefing on no-strands; lineage from DISC-HYGIENE-01)* | always-fresh | ~400 tokens |

### cleanup

| Oracle | Question | Strategy | Est. savings/call |
|---|---|---|---|
| **`archive-retention`** | Which archived PODS sessions exist, and which are old enough to sweep under the retention threshold? *(also `--gc-cheap` and `--gc` modes per RETENTION-01..06; lineage from CSI-05)* | always-fresh | ~200 tokens |
| **`handoff-retention`** | Which `.claude/handoff/handoff-*.md` briefs are older than the 30d default TTL? *(also `--gc-cheap` and `--gc` modes per SWEEP-01; sibling `<file>.KEEP` exempt; lineage from archive-retention; pre-delete `_summary.jsonl` audit per SWEEP-08)* | always-fresh | ~200 tokens |
| **`pid-orphan-detector`** | Which `worker-shell-*.pid` files reference dead processes (liveness-based, no TTL)? *(also `--gc-cheap` and `--gc` modes per SWEEP-04..06; three-leg defense per DEC-55; pre-delete `_pid-summary.jsonl` audit per SWEEP-08; lineage from archive-retention + dispatchable-sessions)* | always-fresh | ~200 tokens |

### discovery

| Oracle | Question | Strategy | Est. savings/call |
|---|---|---|---|
| **`workspace-shared-repos`** | Which sibling git repos does this project consume via workspace declarations? *(SHARED-CSI-01; pnpm + Cargo + npm/yarn + explicit-list per DEC-35; lineage from CSI-05 per DISC-CSI-09)* | trigger-invalidate (`pnpm-workspace.yaml` / `Cargo.toml` / `package.json` / `.claude/config.json`) | ~600 tokens |

---

## Verification Oracles

These oracles answer *"did the last action break or violate something?"* — execution-state checks rather than project-state queries. Each conforms to the Verification Oracle contract (`kind: "verification"`, `<2s` runtime, agent-actionable failure entries, read-only). See `FOUNDATIONS/ORACULURGY_DESIGN.md` Part 11 for the full category spec.

### documentation

| Oracle | Question | Strategy | Est. savings/call |
|---|---|---|---|
| **`markdown-link-validity`** | Do all internal markdown links in tracked documentation files resolve to existing targets? | always-fresh | ~1500 tokens (saves a multi-step "find the missing target" investigation when a link breaks) |
| **`synopsis-coverage`** | What fraction of this project's modules conform to the HCT Synopsis discipline (presence + length 1-5 lines)? *(HCT-04 — paired with the `module-tree-map` oracle; the dogfood progress meter for HCT-06 and the underlying check for HCT-07's Synopsis validation in `/0-uldf-uladp-compliance`)* | trigger-invalidate (`**/README.md`) | ~800 tokens |

### spec

| Oracle | Question | Strategy | Est. savings/call |
|---|---|---|---|
| **`planning-doc-staleness`** | Which `docs/planning/intakes/` and `docs/planning/plans/` docs reference work that has shipped (60-day commit-message scan + spec-status all-DONE heuristic)? *(SWEEP-02; action leg lives in `/0-uldf-finalize` Phase 8.7 — SWEEP-03; `unknown[]` partition surfaces but is never auto-archived per DEC-53; `<2s` speed contract per Part 11)* | always-fresh | ~800 tokens |

---

## Project-Specific Oracles

*Oracles added by this project's specific needs go here, organized by category.*

### security

| Oracle | Question | Kind | Strategy | Consumer Scope | Est. savings/call |
|---|---|---|---|---|---|
| **`multi-tenant-isolation-check`** | Does every domain-touching code path go through tenant-scoped repository methods? Are there raw SQL strings, methods accepting `Connection`/`Pool` outside the repository layer, or repository methods that take `tenant_id` as a non-`TenantScope` argument? *(P0 Task Zero; three-leg defense leg 2 — paired with TenantScope/ProjectScope newtypes (leg 1) and clippy/cargo-deny (leg 3); enforces DEC-FBR-03)* | verification | trigger-invalidate (`migrations/**`, `crates/feedbackmonk-repository/**`, `crates/feedbackmonk-core/**`, `crates/feedbackmonk-api/**`, `crates/feedbackmonk-jwt/**`, `crates/feedbackmonk-anon/**`, allowlist) | P0+ all crates (every commit; CI gate from commit 1) | ~1200 tokens |
| **`pii-scrub-audit`** | Does every emitted log line pass through the canonical 20-pattern PII scrubber installed by `feedbackmonk_tracing::install_global_subscriber`? Has the pattern set drifted from the byte-for-byte canonical port? *(P1 Stage 1 Task Zero; three-leg defense leg 2 — paired with `install_global_subscriber` chokepoint (leg 1) + clippy/cargo-deny (leg 3); enforces FR-FBR-10 + DEC-FBR-01 Persona D privacy)* | verification | trigger-invalidate (`crates/feedbackmonk-tracing/**`, `crates/feedbackmonk-api/src/main.rs`, `expected_hash.txt`) + freshness via pattern-set SHA-256 | P1+ all crates emitting logs (every commit; CI gate) | ~800 tokens |

Invocation: `pwsh .claude/oracles/multi-tenant-isolation-check/oracle.ps1` (Windows) or `bash .claude/oracles/multi-tenant-isolation-check/oracle.sh` (Unix); `python .claude/oracles/pii-scrub-audit/oracle.py` (cross-platform) or `bash .claude/oracles/pii-scrub-audit/oracle.sh` (Unix shim).

### privacy

| Oracle | Question | Kind | Strategy | Consumer Scope | Est. savings/call |
|---|---|---|---|---|---|
| **`widget-bundle-size`** | Is the built feedbackmonk widget bundle (`widget/dist/*.{js,mjs,css}`) at most 30720 bytes (30 KiB; FR-FBR-04 cap), and does it contain zero canonical third-party tracker hostnames? Has the canonical tracker-list drifted from its hashed baseline? *(P2 Task Zero — CLAUDE-A worker, collab-20260514-035703; three-leg defense leg 2 — paired with vite.config.ts terser+CSP-safe bundler chokepoint (leg 1) + Playwright+axe-core a11y harness (leg 3); enforces FR-FBR-04 size cap + DEC-FBR-02 no-third-party-trackers brand promise; cold-start vacuous-PASS supports Task Zero order-of-operations)* | verification | trigger-invalidate (`widget/dist/**`, `widget/src/**`, `widget/vite.config.ts`, `widget/package.json`, `expected-trackers.txt`) | P2+ widget/ (every commit touching widget/dist or widget/src; CI gate) | ~800 tokens |

Invocation: `bash .claude/oracles/widget-bundle-size/oracle.sh` (Unix) or `pwsh .claude/oracles/widget-bundle-size/oracle.ps1` (Windows); `python .claude/oracles/widget-bundle-size/oracle.py` (cross-platform direct).

### tiers

| Oracle | Question | Kind | Strategy | Consumer Scope | Est. savings/call |
|---|---|---|---|---|---|
| **`tier-enforcement-status`** | Does every domain-write handler under `crates/feedbackmonk-api/src/handlers/` either consult `check_tier_quota()` before its first write OR appear in the allowlist? Does `tier_quotas()` in `crates/feedbackmonk-core/src/tier.rs` return the Contract C19 canonical shape per `Tier` variant? With `--full`: do the end-to-end cap-firing smoke tests pass? *(P3 Stage 1 Task Zero; three-leg defense leg 2 — paired with `Tier` enum + `TierQuotas` type-system chokepoint (leg 1) + `sqlx::test` integration smoke (leg 3); enforces FR-FBR-14 cap-firing + DEC-FBR-03 pricing tier matrix; cold-start vacuous-PASS supports Task Zero order-of-operations)* | verification | trigger-invalidate (`crates/feedbackmonk-api/src/handlers/**`, `crates/feedbackmonk-core/src/tier.rs`, `crates/feedbackmonk-repository/src/tier_quota.rs`, allowlist) | P3+ all handlers + tier model + tier-quota repo (every commit; CI gate from P3 start with `--full`) | ~1000 tokens |

Invocation: `bash .claude/oracles/tier-enforcement-status/oracle.sh` (Unix; `--full` runs Probe C integration smoke) or `pwsh .claude/oracles/tier-enforcement-status/oracle.ps1` (Windows); `python .claude/oracles/tier-enforcement-status/oracle.py [--full]` (cross-platform direct).

### deployment

| Oracle | Question | Kind | Strategy | Consumer Scope | Est. savings/call |
|---|---|---|---|---|---|
| **`selfhost-compose-smoke`** | Does `deploy/docker/docker-compose.yml` parse cleanly (yaml-lint), do its application env-var references match the canonical C21 catalog in `docs/operations/SELFHOST_ENV.md`, and (with `--full`) does `docker compose down -v && up -d` from a clean state bring the stack to HTTP 200 on `/health/ready` within 90s? *(P4 Stage 2 Task Zero — CLAUDE-B worker, collab-20260514-170323; three-leg defense leg 2 — paired with env-reader fail-fast chokepoints in `crates/feedbackmonk-api/src/main.rs` (leg 1) + operator runbook `docs/operations/SELFHOST.md` cold-readability (leg 3); enforces FR-FBR-17 self-host distribution + Contracts C21 (env catalog SSOT) + C24 (three-probe schema); cold-start vacuous-PASS supports Task Zero order-of-operations)* | verification | trigger-invalidate (`deploy/docker/**`, `docs/operations/SELFHOST_ENV.md`, oracle.py) | P4+ `deploy/docker/**` + `docs/operations/SELFHOST_ENV.md` (every commit touching either; CI gate post-launch with `--full`) | ~900 tokens |

Invocation: `bash .claude/oracles/selfhost-compose-smoke/oracle.sh` (Unix; `--full` runs Probe C clean-state smoke) or `pwsh .claude/oracles/selfhost-compose-smoke/oracle.ps1` (Windows); `python .claude/oracles/selfhost-compose-smoke/oracle.py [--full]` (cross-platform direct).

---

## Invocation Quick Reference

### Unix

```bash
bash .claude/oracles/<oracle-name>/run.sh
```

### Windows

```powershell
powershell -NoProfile -File .claude/oracles/<oracle-name>/run.ps1
```

### Full oracle details

For any oracle, read its manifest: `.claude/oracles/<oracle-name>/oracle.json`

### Special modes

Some oracles expose additional modes beyond their default read-only invocation:

| Oracle | Mode | Purpose |
|---|---|---|
| `dispatchable-sessions` | `--gc-cheap` | Session-start registry hygiene sweep (≤100ms budget). Flips `status="active"` entries with dead PIDs older than threshold (default 24h) to `status="expired"` and moves them to `closed[]`. Silent on success. |
| `dispatchable-sessions` | `--gc` | On-demand registry hygiene sweep (no time budget). Same semantics as `--gc-cheap` but emits a JSON summary `{swept, before, after, threshold, thresholdSource}`. Threshold configurable via `.claude/config.json` `csi.registryHygieneThreshold` (numeric hours OR ISO-8601 `PnH`/`PnD`). |
| `archive-retention` | `--gc-cheap` | Session-start archive hygiene sweep (≤100ms budget). Sweeps `.claude/collaboration/archived/collab-*/` dirs older than the configurable threshold (default 90 days), excluding KEEP-pinned dirs. Pre-delete summary appended to `_summary.jsonl`. Silent on success. |
| `archive-retention` | `--gc` | On-demand full archive sweep (no time budget). Same semantics as `--gc-cheap` but emits a JSON summary `{swept, before, after, threshold, thresholdSource, summarized, sweptIds?}`. Threshold configurable via `.claude/config.json` `archiveRetention.threshold` (numeric days OR ISO-8601 `PnD`). |
| `handoff-retention` | `--gc-cheap` | Session-start handoff hygiene (silent; read-only per SWEEP-01 spec — drift surfaces via default-mode `briefing` field, not via `--gc-cheap` output). Wired into session-start hook for symmetry with archive-retention. |
| `handoff-retention` | `--gc` | On-demand sweep of `.claude/handoff/handoff-*.md` files older than 30d (default). Sibling `<file>.KEEP` exempt. Pre-delete JSONL line written to `.claude/handoff/_summary.jsonl` BEFORE delete (SWEEP-08 invariant). Threshold configurable via `.claude/config.json` `handoffRetention.threshold`. |
| `pid-orphan-detector` | `--gc-cheap` | Session-start liveness sweep (≤500ms budget; bumped from 100ms for Windows `Get-Process` cold-start overhead). Sweeps `worker-shell-*.pid` files referencing dead processes; silent on success. Three-leg defense per DEC-55 (with SessionEnd hook + operator `--gc`). |
| `pid-orphan-detector` | `--gc` | On-demand full sweep (no time budget). Pre-delete JSONL line written to `ltads/execution/_pid-summary.jsonl` BEFORE delete (SWEEP-08 invariant). No TTL — liveness-based per DEC-54. |

---

## How This Index Is Maintained

- **On oracle creation** — author adds an entry to the appropriate category
- **On `/0-uldf-finalize` Phase 11** — audit verifies all oracle manifests have corresponding INDEX.md entries and flags mismatches
- **On staleness** — stale oracles are marked with a `[STALE]` prefix in the entry
- **On removal** — entries are deleted when oracles are removed

The INDEX.md is designed to be cheaply scannable — a single agent Read call should reveal every oracle available in the project.
