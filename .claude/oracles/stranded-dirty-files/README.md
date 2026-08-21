# stranded-dirty-files Oracle

## Synopsis

Project-state visibility oracle (CSI-15, Phase 1.7) that reports dirty files older than the most-recent-finalize boundary AND not claimed by any live peer. Output is the **frozen output schema** consumed by the session-start hook (briefing-line emission with empty-result silence) and by `/0-uldf-finalize --include-stranded` (FINALIZE-04 partition-Set-2 input mirroring SHARED-FINALIZE-02). Don't come here for live-peer enumeration (that's `dispatchable-sessions`, CSI-05), for cleanup (this oracle is visibility-only — `/0-uldf-finalize --include-stranded` is the cleanup surface), or for state-vs-registry consistency checks (that's `stale-ltads-state`, CSI-14).

## Identity

**Question answered**: Which dirty files in this project have no live owner and predate the most-recent-finalize boundary?

**Category**: workflow
**Kind**: `project-state`
**Spec**: `docs/specs/SPECIFICATION.md` § CSI-15
**Discovery**: `docs/specs/DISCOVERIES.md` DISC-HYGIENE-01 (SessionHelm 2026-05-06 — 200+ uncommitted files surfaced after `/0-uldf-finalize` accumulated cross-session-conservative scope decisions over weeks)
**Design lineage**: `~/.claude/oracles/stale-ltads-state/` (CSI-14) — same briefing-line silence pattern; `~/.claude/oracles/dispatchable-sessions/` (CSI-05) — registry-aware project-state oracle skeleton; `~/.claude/oracles/workspace-shared-repos/` (SHARED-CSI-01) — frozen-schema discipline + Set 1/Set 2 partition pattern that FINALIZE-04 mirrors.

## 1. Purpose & Responsibilities

The Phase 1.7 visibility leg of the post-CSI-1.6 hygiene arc. Surfaces the failure mode that triggered the arc: dirty files accumulating *between* finalizes, owned by nothing live, escaping the cross-session-conservative scope decisions `/0-uldf-finalize` makes by default.

- **Session-start visibility**: emits a `[stranded-dirty-files]` line in the ORACLE BRIEFING when this project has stranded files. Empty-briefing case suppresses the line (parallel to `stale-ltads-state`).
- **Cleanup driver**: the briefing line points the agent at `/0-uldf-finalize --include-stranded` (FINALIZE-04, separate spec entry — same Arc 2). The oracle never mutates state.
- **Frozen output schema**: the `/0-uldf-finalize --include-stranded` flag handler (FINALIZE-04) reads this oracle's output to drive its partition-Set-2 logic. Schema changes require an explicit decision-channel entry plus LD approval.

The oracle exists because every consumer (session-start hook + `/0-uldf-finalize --include-stranded`) would otherwise re-derive the answer by manually combining `git status --porcelain` + `git log -1 --format=%aI HEAD` + the registry. Pre-answering once per session-start is the entire point.

## 2. File Index

| File | Purpose |
|---|---|
| `oracle.json` | Manifest — frozen output schema, freshness contract, scope guard, provenance |
| `run.sh` | Bash entry point (Unix + Git Bash on Windows) |
| `run.ps1` | PowerShell entry point (Windows native) |
| `validate.sh` | Sandbox self-test (Unix) — T1-T5 |
| `validate.ps1` | Sandbox self-test (Windows) — T1-T5 |
| `README.md` | This file — ULADP module README + frozen schema contract |
| `test-fixtures/` | Reference fixture data (registry JSONs + per-case README) for the validate harnesses + smoke tests |

## 3. Public API & Usage

### Invocation

```bash
# Unix / Git Bash
bash .claude/oracles/stranded-dirty-files/run.sh

# Windows PowerShell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .claude/oracles/stranded-dirty-files/run.ps1
```

No flags. This oracle has no `--gc` / `--gc-cheap` modes — visibility-only.

### **FROZEN OUTPUT SCHEMA — load-bearing contract**

This schema is **frozen at first commit**. The session-start hook reads `.briefing` literally; `/0-uldf-finalize --include-stranded` parses `.sample[].path` and `.count` for partition logic. Schema changes require an explicit decision-channel entry plus LD approval — never silent.

```json
{
  "has_stranded": false,
  "count": 0,
  "oldest_mtime": null,
  "sample": [],
  "live_peer_count": 0,
  "provably_unowned": 0,
  "attribution": "no-peers",
  "last_finalize_at": null,
  "briefing": ""
}
```

**Field invariants**:

- `has_stranded` (bool): `true` iff `count > 0`. `false` when `count == 0` OR `count == -1` (detection-skipped sentinel).
- `count` (int): number of stranded files detected. `-1` means "detection skipped — too many dirty files (>2000)" per spec acceptance.
- `oldest_mtime` (ISO-8601 UTC string OR `null`): mtime of the oldest stranded file. `null` when `count == 0` OR detection skipped. Format: `"yyyy-MM-ddTHH:mm:ssZ"`.
- `sample` (array, capped at 10): each entry `{path: string, mtime: ISO-8601 UTC string, age_days: int}`. Path is repo-relative (forward slashes regardless of platform). `age_days` = floor((now − mtime) / 86400). Empty when `count == 0` OR detection skipped.
- `live_peer_count` (int): number of live peers (per `dispatchable-sessions` registry filter — `status: "active"` AND PID alive AND `workDir` matches this project root in POSIX or Windows form, case-insensitive) consulted for ownership. `0` when registry missing.
- `provably_unowned` (integer — **additive in 1.3.0**, DEFER-193/DEC-372): of the `count` reported strands, how many the STRAND-02 mtime-ordering test proves un-owned (mtime older than the earliest live peer's `spawnedAt`). Equals `count` when `live_peer_count == 0`; `0` when the proof could not run at all (a live peer with an absent or unparseable `spawnedAt` disables it for the whole run). A *withheld* run reports it so the refusal says how much of the list IS proved rather than refusing opaquely.
- `attribution` (string enum — **additive in 1.2.0**, DEFER-039/DEFER-190/DEC-366; `predates-peers` added in **1.3.0**, DEFER-193/DEC-372): the evidence grade behind the "no live owner" clause. `measured` — a live peer published a `dirtyFiles[]` array (complete by construction; absence from it IS evidence). **RESERVED AND UNPRODUCED**: nothing in the framework writes that field (DISC-ORA-12) and nothing is planned to (DEC-372) — kept because it is authoritative if a producer ever appears and because deployed consumers map it. `predates-peers` — **the reachable top of the ladder**: every reported file's mtime is older than the earliest live peer's session start, so no visible live peer can have written any of them (a peer that had would have set the mtime at or after its own start). All-or-nothing over the reported set, because the sweep it gates is all-or-nothing. `journal-partial` — attribution came only from live peers' write journals, which record tool calls and are **positive-only** (a hit filters, a miss proves nothing) → sweep recommendation withheld. `unavailable` — live peers exist and supplied neither → withheld. `no-peers` — no live peer, no owner to miss. `not-measured` — detection skipped. Consumers gate sweep behavior through `~/.claude/scripts/lib/stranded-evidence-gate.{sh,ps1}` (STRAND-01) and treat an **absent** field as unknown evidence (withhold), never as license to sweep.
- `last_finalize_at` (ISO-8601 UTC string OR `null`): commit timestamp of `HEAD` (the most-recent-finalize boundary per spec §3 acceptance). `null` when not in a git repo OR no commits.
- `briefing` (string): empty `""` when no stranded files. Populated when `count > 0` OR `count == -1`.

**Briefing-line forms** (the hook reads `.briefing` literally; withholding forms take precedence over the small/large split):

- `count == 0` → `""` (line suppressed by hook — parallel to `stale-ltads-state`)
- `attribution == "journal-partial"` → `"stranded-dirty-files: <N> pre-finalize dirty file(s) (oldest <D> days) - OWNERSHIP UNPROVEN: attribution came only from <J> write journal(s), ... Do NOT sweep; inspect with /0-uldf-oracle stranded-dirty-files"`
- `attribution == "unavailable"` → `"stranded-dirty-files: <N> pre-finalize dirty file(s) (oldest <D> days) - OWNERSHIP UNKNOWN: <P> live peer(s) and none publishes attribution, ... Do NOT sweep; inspect with /0-uldf-oracle stranded-dirty-files"`
- `0 < count < 50`, `attribution == "predates-peers"` → `"stranded-dirty-files: <N> files (oldest <D> days; no live owner — every one predates all <P> live peer(s)' session start) — run /0-uldf-finalize --include-stranded for cleanup"`
- Both withholding forms carry an extra sentence when the proof could run: `" <P> of <N> predate every live peer's session start; <U> do not."`
- `0 < count < 50`, `attribution == "measured"` → `"stranded-dirty-files: <N> files (oldest <D> days; no live owner per <P> attributing peer(s)) — run /0-uldf-finalize --include-stranded for cleanup"`
- `0 < count < 50`, `attribution == "no-peers"` → `"stranded-dirty-files: <N> files (oldest <D> days; no live owner — no live peers in this project) — run /0-uldf-finalize --include-stranded for cleanup"`
- `count >= 50` (attribution `measured`/`no-peers`) → `"stranded-dirty-files: <N> files (oldest <D> days) — significant accumulation; see /0-uldf-oracle stranded-dirty-files for full sample"`
- `count == -1` → `"stranded-dirty-files: detection skipped — too many dirty files (>2000); run /0-uldf-finalize --include-stranded for full audit"`

**Empty-result form** (graceful absence — clean tree, no commits, or not in a git repo):

```json
{
  "has_stranded": false,
  "count": 0,
  "oldest_mtime": null,
  "sample": [],
  "live_peer_count": 0,
  "last_finalize_at": null,
  "briefing": ""
}
```

This is the **graceful-absence contract** — consumers detect "no strands to surface" by `count == 0` and `briefing == ""`, not by the oracle erroring.

## 4. Constraints & Business Rules

### Detection algorithm (per spec CSI-15 §3 acceptance)

1. `git status --porcelain` → dirty file set; non-git repo or no dirty files → emit `count: 0` (graceful empty).
2. `last_finalize_at = git log -1 --format=%aI HEAD` (commit timestamp of HEAD).
3. For each dirty file: stat mtime; if `mtime < last_finalize_at` AND `file_owned_by_live_peer == false` → strand candidate.
4. Live-peer ownership: a registry entry exists with `status: "active"` AND `workDir` matches this project root AND PID alive AND a `dirtyFiles[]` registry field declares the file.

### Ownership attribution — three sources, graded (1.3.0; DEFER-039/DEFER-190/DEFER-193)

The `dirtyFiles[]` registry field is authoritative when present (complete by construction → grade
`measured`) — but **nothing writes it** (measured GitCellar 2026-08-12, ULDF 2026-08-16;
DISC-ORA-12), so on its own the owned-set was empty on every run and "no live owner" was asserted
over zero evidence — the DEFER-039 witnessed wrong verdict, whose briefing string invited sweeping
a live sibling's in-flight working set. Since 1.2.0 the oracle ALSO reads each live peer's write
journal (`.claude/session-state/write-journal/<sessionId>.jsonl` — the same input
`lib/file-owner.sh` resolves ownership from). The journal is **positive-only** evidence: it records
tool calls, so a script write or an external editor leaves no record — a journal hit rescues a file
from the strand list, a miss proves nothing. The `attribution` field grades which source actually
supplied evidence, and the briefing withholds the sweep recommendation whenever ownership is
unproven (`journal-partial` / `unavailable`).

**Third source (1.3.0, STRAND-02 / DEC-372) — the mtime-ordering proof, which needs no writer at
all.** A live peer that had written file F would have set F's mtime at or after its own start, so
`mtime(F) < min over live peers of spawnedAt` *proves* F is not any visible live peer's in-flight
work. That is the proposition `measured` was defined to deliver, from data every registry already
carries. It is sound because `spawnedAt` is preserved across re-registration (`hooks/session-start.sh`),
because a PODS roster row is written before its worker boots (erring early, the conservative
direction), and because the framework already trusts this anchor for the same class of reasoning
(`commit-scoped.sh` `_sw_session_ts`). It fails **closed**: one live peer with an absent or
unparseable `spawnedAt` disables the proof for the whole run, since dropping that peer from the
minimum would *raise* the floor and prove more. A future registry writer would restore `measured` with no
schema change.

### Boundary policy (the "most-recent-finalize" definition)

The oracle uses `git log -1 --format=%aI HEAD` (commit timestamp of HEAD) as the finalize boundary. The reasoning per spec acceptance §3: the project's last commit IS the effective finalize boundary in normal usage (`/0-uldf-finalize` Phase 8 commits, then Phase 8 pushes; the next session sees that commit as the last finalize). Refined detection (parsing commit messages for `/0-uldf-finalize` markers, or reading a `.claude/finalize-state/last-finalize.json` artifact if FINALIZE-01..03 produces one) is deferred to Phase 1.7.1 if the simple boundary proves insufficient.

### Performance ceiling (per spec acceptance)

| Dirty count | Target | Action |
|---|---|---|
| ≤500 | ≤250ms | Full scan |
| ≤2000 | ≤500ms ceiling | Bounded scan (per-file stat dominated; bash + PowerShell both well-bounded) |
| >2000 | — | Detection skipped; `count: -1`; briefing emits the detection-skipped form pointing at `/0-uldf-finalize --include-stranded` |

The `>2000` scope guard exists to protect the briefing budget. A 2000+ dirty file project is itself a discoverability symptom — the briefing line surfacing this fact is the right outcome.

### Composability with other partitions (per spec FINALIZE-04 acceptance #3)

`/0-uldf-finalize` operates on four independent partitions:

1. **Local-session** — files touched during this session (Phase 1.5 default scope)
2. **Local-stranded** — files this oracle reports (gated by `--include-stranded`)
3. **Shared-session** — Set 1 in shared repos (SHARED-FINALIZE-02 default; auto-process)
4. **Shared-pre-existing** — Set 2 in shared repos (gated by `--include-shared`)

Each partition is independently composable; `--include-stranded` and `--include-shared` are NOT mutually exclusive. Both can be present, opting into both pre-existing partitions.

### Suppression (graceful absence)

When `briefing == ""`, the session-start hook MUST NOT emit a `[stranded-dirty-files]` line. This mirrors the `stale-ltads-state` and `dispatchable-sessions` no-strand silence patterns. Briefing budget is preserved on healthy projects.

## 5. Relationships & Dependencies

### Upstream

- **Git** — `git rev-parse`, `git status --porcelain`, `git log -1 --format=%aI HEAD`. Not-a-git-repo → graceful empty.
- **`.claude/collaboration/active-sessions.json`** — optional; the registry consumed for live-peer ownership query. Missing → `live_peer_count: 0` (no peers consulted; default-to-strand).

### Downstream consumers

| Consumer | Where | What it reads |
|---|---|---|
| Session-start hook briefing | `~/.claude/hooks/session-start.{sh,ps1}` | `.briefing` field — emitted as-is when non-empty |
| `/0-uldf-finalize --include-stranded` | `~/.claude/segments/-finalize/_stranded-flag-handling.md` + `_stranded-partition.md` | `.sample[].path` (preview); on opt-in, oracle re-invocation collects the full set; `.count` and `.last_finalize_at` for summary block; **`.attribution` through `~/.claude/scripts/lib/stranded-evidence-gate.{sh,ps1}`** — Set 2 is staged only on a `sweep` verdict (STRAND-01; absent field withholds) |
| Spec reconciliation | `/0-uldf-finalize` Phase 4.5 | Indirect — when stranded files include spec docs, finalize surfaces them for explicit user decision |

### Sibling oracles

- **`stale-ltads-state`** (CSI-14) — different question (state-vs-registry consistency vs. dirty-vs-finalize boundary) but identical structural skeleton: standalone oracle, frozen output schema, briefing-line emission with empty-result silence.
- **`dispatchable-sessions`** (CSI-05) — primary precedent for the registry-aware project-state oracle skeleton (parser fallback chain, JSON output).
- **`workspace-shared-repos`** (SHARED-CSI-01) — primary precedent for frozen-schema discipline + Set 1/Set 2 partition pattern (FINALIZE-04 mirrors this).

This oracle does **not** expose `--gc` / `--gc-cheap` modes. Visibility oracles report state; they don't sweep it.

## 6. Decision Log

- **Briefing-line silence on empty-result** — same pattern as `stale-ltads-state` and `dispatchable-sessions`. Healthy-project sessions should not see a "stranded: 0 files" line; the absence of the line IS the signal. Implements via empty `briefing: ""` field that the hook checks before emitting.

- **`count: -1` sentinel for detection-skipped** — distinct from `count: 0` so consumers can tell "detection didn't run" from "detection ran and found nothing". `has_stranded` stays `false` in both cases (the user-facing "is there a strand problem?" question is correctly answered "no actionable signal"). The detection-skipped briefing form points the user at the on-demand audit path.

- **Forward-compatible `dirtyFiles[]` registry field with default-to-strand** — the field is reserved in the schema today; peers will populate it when CSI Phase 2 ships claim mechanisms. Until then, default behavior is "no live peer claims this file", which yields the correct default-to-strand classification given the trigger pattern. Schema requires NO change when peers begin publishing.

- **Boundary = HEAD's commit timestamp, not a finalize-marker scan** — simplest unambiguous boundary that handles the common case (project's last commit is the last finalize). Refined detection (parsing `/0-uldf-finalize` markers, reading a finalize-state JSON) is Phase 1.7.1 territory if the simple boundary proves insufficient. Deferring keeps Phase 1.7 to a single oracle + flag.

- **Skeleton transferred from `stale-ltads-state` per the CSI Phase 1.6 lineage** — same structural shape: standalone oracle, frozen output schema, briefing-line emission with empty-result silence, ~250ms compute budget. Net new code: dirty-file enumeration + mtime-vs-finalize gate + live-peer ownership cross-reference. Roughly 50/50 transfer/new.

- **Test-fixtures directory holds reference data + per-case README, not static fixture trees** — the inputs for this oracle are dirty git working trees + registry JSONs, which can't easily be checked in as static tree state. The `validate.{sh,ps1}` scripts build sandboxes inline using the patterns documented in `test-fixtures/README.md`.

## 7. Cross-References

- **Manifest**: `oracle.json`
- **Spec**: `docs/specs/SPECIFICATION.md` § CSI-15 (acceptance criteria); § FINALIZE-04 (consumer flag)
- **Discovery (trigger)**: `docs/specs/DISCOVERIES.md` DISC-HYGIENE-01
- **Plan**: `docs/planning/plans/20260507T065531-post-csi-1-6-framework-hygiene-arc.md` § Worker B1
- **Skeleton precedents**: `~/.claude/oracles/stale-ltads-state/`, `~/.claude/oracles/dispatchable-sessions/`, `~/.claude/oracles/workspace-shared-repos/`
- **Foundation**: `FOUNDATIONS/CSI_DESIGN.md` § Phase 1.7 (this oracle); `FOUNDATIONS/ORACULURGY_DESIGN.md` (project-state oracle category) and `FOUNDATIONS/PRINCIPLES_OF_LLM_AGENT_ORCHESTRATION.md` § 2.13 (Probandurgy — CSI is mechanism 8)
- **Consumer flag**: `~/.claude/skills/0-uldf-finalize/SKILL.md` `--include-stranded` flag table entry; `~/.claude/segments/-finalize/_stranded-flag-handling.md`; `~/.claude/segments/-finalize/_stranded-partition.md`

## 8. Testing

```bash
bash .claude/oracles/stranded-dirty-files/validate.sh                                          # Unix / Git Bash
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .claude/oracles/stranded-dirty-files/validate.ps1   # Windows
```

Validates eight scenarios:

| Test | What it asserts |
|---|---|
| **T1** | no-stranded — clean of pre-finalize dirty files; `count==0`, `briefing==""` |
| **T2** | small-stranded, no registry — `count==3`, `attribution=="no-peers"`, sweep still recommended |
| **T3** | large-stranded — 55 pre-finalize dirty files; `count==55`, briefing references "significant accumulation" |
| **T4** | detection-skipped-too-many — 2001+ dirty files; `count==-1`, briefing references "detection skipped" |
| **T5** | live-peer-owns-file — peer claims one of two via `dirtyFiles[]`; only the unclaimed file counts; `attribution=="measured"`; sweep advice survives (anti-vacuity control) |
| **T6** | live peer publishing nothing — `attribution=="unavailable"`; briefing says OWNERSHIP UNKNOWN and the sweep recommendation is WITHHELD (sh cell also pins the two-form workDir match — DEFER-190's Git Bash zero-peers defect) |
| **T7** | falsification pair (from GitCellar's `selftest.ps1`) — a write journal claiming one of two files filters exactly that one; `attribution=="journal-partial"`; still withheld |
| **T8** | STRAND-02 — the live peer started AFTER the files were written; `attribution=="predates-peers"`, `provably_unowned==2`, sweep RECOMMENDED. Pre-1.3.0 this fixture graded `unavailable` and withheld forever |
| **T8b** | anti-vacuity + the tool-calls-only caveat — one more file written after the peer's start by a route the journal cannot see; the grade drops back to `unavailable` and the WHOLE stage is withheld, while `provably_unowned` still reports 2 of 3 |
| **T8c** | fail-closed — a live peer with **no** `spawnedAt` disables the proof for the whole run (never silently dropped from the earliest-start floor, which would prove *more*) |
| **T9** | the identity-keying caveat (FINALIZE-SCOPE-11 / DEC-337) — a journal keyed under a different id than the registry `sessionId` misses, and the verdict degrades toward withholding, never toward the sweep. **T9b** re-keys the identical journal and the file filters, proving T9 measured the key |
| **T7b** | inverted control — journal deleted → both files stranded again, grade drops to `unavailable`; proves the filter was doing the work, not the arithmetic agreeing |

The full hygiene smoke harness at `~/.claude/scripts/hygiene-tests/hygiene-csi15-stranded-smoke.{sh,ps1}` adds three flag-wiring smoke cases (without-flag, with-flag, composable-with-shared) on top of the oracle T1-T5 set.
