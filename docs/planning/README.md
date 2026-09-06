# Planning Context

This directory contains **pre-implementation planning artifacts** produced by ULDF commands. These files are the product of Contexturgy — the deliberate crystallization of ephemeral cognitive state into durable, recoverable artifacts. They enable Soveredelity by ensuring no planning context is lost across sessions, compactions, or temporal boundaries.

## Lifecycle

```
Phase 1: CREATED by LDIS/Core commands during planning (Contexturgy)
Phase 2: CONSUMED by implementation commands (PODS workers, LTADS)
Phase 3: MIGRATED to permanent ULADP locations by /0-uldf-finalize
         (decisions → module Decision Logs, reasoning → where appropriate)
Phase 4: ARCHIVED or cleaned up post-finalize
```

## Directory Layout

Event-style artifacts are **per-invocation snapshots**, stored in typed subdirectories. Each invocation produces its own file — previous artifacts are preserved, not overwritten.

```
docs/planning/
├── README.md                  (this file)
├── intakes/                   (produced by /0-uldf-ldis-intake)
│   └── {timestamp}-{slug}.md
├── plans/                     (produced by /0-uldf-ldis-plan and /0-uldf-ltads-start analysis)
│   └── {timestamp}-{slug}.md
├── ideations/                 (produced by /0-uldf-ldis-ideate)
│   └── {timestamp}-{slug}.md
└── archive/                   (post-finalize archival)
```

### Filename Convention

`{timestamp}-{slug}.md`

- **timestamp**: ISO-8601 compact form `YYYYMMDDTHHMMSS` (e.g., `20260409T143022`). Lexically sortable — newest-last when sorted ascending.
- **slug**: task description, lowercased, non-alphanumeric replaced with `-`, repeated dashes collapsed, trimmed to 40 chars, trailing dashes stripped. Purpose: human-browseable.
- **Collision suffix**: if the generated filename already exists (same second + same slug), append `-2`, `-3`, etc.

Examples:

- `intakes/20260409T143022-add-user-authentication.md`
- `plans/20260409T150301-fix-login-bug.md`
- `ideations/20260409T140100-recipe-vault.md`

## Resolution Rule (how commands find "the latest")

When a downstream command needs "the most recent intake" (or plan, or ideation), it resolves in this order:

1. **Explicit override** — if the user or calling command passed an explicit path (e.g., `--from-intake=<path>`), use that. No further lookup.
2. **Newest in typed subdirectory** — list `docs/planning/{intakes,plans,ideations}/*.md`, sort lexically, pick the last entry. Because timestamps are ISO-8601 compact, lexical order matches chronological order.
3. **Legacy fallback** — if the subdirectory doesn't exist or is empty, fall back to the pre-subdirectory fixed path:
   - `docs/planning/intake-assessment.md`
   - `docs/planning/execution-plan.md`
   - `docs/planning/ideation-notes.md`
   If the legacy file exists, read it. On the next write, the new artifact goes into the subdirectory — legacy files remain in place until `/0-uldf-finalize` archives them.
4. **Nothing found** — treat as "no prior artifact of this type." Proceed accordingly.

### `plans/` holds TWO artifact classes — resolve by kind, not by date alone (PLANKIND-01/DEC-327)

`docs/planning/plans/` receives both `/0-uldf-ldis-plan` **program plans** (the plan of record) and
`/0-uldf-ltads-start` **start analyses**. Step 2's *newest-wins* rule has no type distinction, so a
small analysis stub shadows the program plan written the same day — and finalize's phase 0.6
testability gate resolves *its* active plan from the `workflow-position` oracle's `latest_plan`, so
it armed against a stub carrying no Testability Gate Findings table and silently did not fire.

- A **start analysis** MUST be named `{timestamp}-{slug}-start-analysis.md` **and** open with front
  matter containing `kind: start-analysis`. Two markers: the filename arm covers artifacts written
  before this rule, the front-matter arm survives a rename.
- `latest_plan` resolves over **plans of record only** — start analyses are excluded. The newest
  analysis is reported separately as `latest_start_analysis`, so nothing is hidden.
- **Workflow POSITION still keys on the newest artifact of either class.** A project holding only
  start analyses is past planning; narrowing the *field* must not regress its *position* to
  POST-SPEC.

### Within a single session

A command that invokes a write (e.g., `/0-uldf-ldis-intake`) generates its path **once** during Phase 6.1 and **reuses that same path** for all subsequent updates during the same session (answered questions, re-assessment). Same session → same file. Two concurrent sessions → two different files (timestamp differs by at least one second; slug deduplication handles the edge case).

## File Index

| Subdirectory / File | Source Command | Contents |
|---------------------|----------------|----------|
| `intakes/*.md` | `/0-uldf-ldis-intake` | Task perception, specification analysis, calibration, engagement strategy, collaboration assessment |
| `plans/*.md` | `/0-uldf-ldis-plan`, `/0-uldf-ltads-start` | Execution strategy, decomposition, agent assignments, sync points |
| `ideations/*.md` | `/0-uldf-ldis-ideate` | Exploration summary, feature domains, scope thinking, early decisions |

**Note**: Specification artifacts live in `docs/specs/` (managed by `/0-uldf-ldis-spec`), not here. That directory holds the project's canonical living spec (state), not per-invocation snapshots (events).

## Cross-Command State Chain

Each command reads predecessors' crystallized output automatically via the resolution rule:

```
/0-uldf-ldis-ideate  → writes ideations/{ts}-{slug}.md
/0-uldf-ldis-intake  → reads latest ideation, writes intakes/{ts}-{slug}.md
/0-uldf-ldis-spec    → reads latest intake + ideation, writes docs/specs/*
/0-uldf-ldis-plan    → reads latest intake + specs, writes plans/{ts}-{slug}.md
# Implementation → reads latest plan + specs
/0-uldf-finalize     → reads latest planning artifacts, migrates content to permanent Decision Logs
```

## Design Rationale

### Why per-invocation snapshots?

The pre-subdirectory design used a single fixed path per artifact type (`intake-assessment.md`, `execution-plan.md`, `ideation-notes.md`). This caused a concurrency/history bug:

- Two Claude sessions running `/0-uldf-ldis-intake` on the same project would clobber each other's assessments.
- Re-running intake to refine a task silently destroyed the prior assessment with no history.
- A session's Phase 0 "read prior planning" could accidentally pick up another session's in-progress work and treat it as its own prior context.

Per-invocation snapshots solve all three: each invocation owns its own file, history is preserved, and "the latest" is resolved at read time, not write time.

### Why timestamps in filenames (not content only)?

- Lexical sort on filename = chronological sort. No parsing required at read time.
- Works on Windows (no symlinks) and with any file browser.
- Cheap collision detection: same-second + same-slug is the only failure mode, handled by a numeric suffix.

### Why slugs?

Human browsing. When accumulated artifacts grow, `intakes/20260409T143022-add-user-auth.md` is immediately meaningful; `intakes/20260409T143022.md` is not.

### Why no INDEX.md?

A separate index file would be another stale-state risk. The directory listing IS the index.

## Contexturgy Guarantees

Per FOUNDATIONS Core Concepts: Contexturgy is the primary enabling mechanism for Soveredelity. Planning output — decisions, reasoning, analytical context — is critical state with the highest regeneration cost.

Per the Ephemeral Planning anti-pattern (FOUNDATIONS 5.2): Commands that produce planning output must perform Contexturgy as a structural step, not as optional behavior. With per-invocation snapshots, the Contexturgy guarantee is strengthened: no prior session's work can be silently destroyed by a concurrent or subsequent invocation.

## Notes

- This directory is **project-specific** — created in each project's `docs/planning/`.
- LTADS reads from here but does not own it — planning works with or without LTADS active.
- After `/0-uldf-finalize` migrates content, files may be moved to `archive/` subdirectory.
- **Legacy fixed-path files** (`intake-assessment.md`, `execution-plan.md`, `ideation-notes.md` directly in `docs/planning/`) are still readable by the resolution rule for backward compatibility. New writes always go into subdirectories.
