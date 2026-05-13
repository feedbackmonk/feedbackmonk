# workspace-shared-repos Oracle

## Synopsis

Project-state discovery oracle (SHARED-CSI-01) that enumerates sibling git repos this project consumes via `pnpm-workspace.yaml`, `Cargo.toml [workspace] members`, `package.json workspaces`, or `.claude/config.json sharedRepos`. Output is the **frozen output schema** consumed by SHARED-CSI-02..06 + SHARED-FINALIZE-01..06; this README also defines the **frozen snapshot schema** for `.claude/session-state/shared-repo-snapshots/<sha256-hex-12>.json` (filename hash-derived for collision-free writer/reader contract). Don't come here for within-project session listing (that's `dispatchable-sessions`, CSI-05), for submodule discovery (out of scope per DEC-35 — use the explicit-list escape hatch), or for `--gc` modes (this is a discovery oracle, not a cleanup oracle).

## Identity

**Question answered**: Which sibling git repos does this project consume via workspace declarations?

**Category**: discovery
**Kind**: `project-state`
**Spec**: `docs/specs/SPECIFICATION.md` § SHARED-CSI-01
**Decision**: `docs/specs/DECISIONS.md` DEC-35 (discovery format scope)
**Design lineage**: `claude-template/oracles/dispatchable-sessions/` (CSI-05) and `claude-template/oracles/archive-retention/` (RETENTION-01..06), per DISC-CSI-09's transferable-skeleton finding.

## 1. Purpose & Responsibilities

Stage 1 foundation deliverable for the cross-project visibility arc (CSI Phase 1.5). Produces the load-bearing input that downstream Stage 2 mechanisms (SHARED-CSI-02..06, SHARED-FINALIZE-01..06) consume:

- **SHARED-CSI-02** reads the discovered set at session-start to register this session's identity in each shared-repo registry, and to capture per-shared-repo dirty-state snapshots.
- **SHARED-CSI-03** reads the same set to cross-correlate with each shared-repo's `active-sessions.json` for the `[shared-repo-coordination]` briefing line.
- **SHARED-CSI-04** reads it at arc terminus to close this session's entry in each shared-repo registry.
- **SHARED-CSI-06** reads it to extend `--gc-cheap` registry hygiene to shared-repo registries.
- **SHARED-FINALIZE-01..06** reads it during `/0-uldf-finalize`'s setup phase to drive the per-repo finalize loop.

The oracle exists because every one of these consumers would otherwise have to re-derive the shared-repo set from scratch by parsing four declaration formats (pnpm, Cargo, npm, explicit) and applying four filtering rules (require `.git/`, skip self, skip nested, dedup). Pre-answering the question once per session-start is the entire point.

## 2. File Index

| File | Purpose |
|---|---|
| `oracle.json` | Manifest — frozen output schema, freshness contract, snapshot contract, provenance |
| `run.sh` | Bash entry point (Unix + Git Bash on Windows) |
| `run.ps1` | PowerShell entry point (Windows native) |
| `validate.sh` | Sandbox self-test (Unix) — T1-T8 |
| `validate.ps1` | Sandbox self-test (Windows) — T1-T8 |
| `README.md` | This file — ULADP module README + frozen schema contracts |
| `test-fixtures/pnpm-workspace/` | Synthetic `pnpm-workspace.yaml` for T1 |
| `test-fixtures/cargo-workspace/` | Synthetic `Cargo.toml` for T2 |
| `test-fixtures/npm-workspace/` | Synthetic `package.json` for T3 |
| `test-fixtures/explicit-list/` | Synthetic `.claude/config.json` for T4 |

## 3. Public API & Usage

### Invocation

```bash
# Unix / Git Bash
bash .claude/oracles/workspace-shared-repos/run.sh

# Windows PowerShell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .claude/oracles/workspace-shared-repos/run.ps1
```

No flags. This oracle has no `--gc` / `--gc-cheap` modes — it does not sweep state.

### **FROZEN OUTPUT SCHEMA — load-bearing contract**

This schema is **frozen at first commit**. Stage 2 consumers (CLAUDE-B's hook extension, CLAUDE-C's `/0-uldf-finalize` integration) parse this output. Schema changes require an explicit decision-channel entry plus LD approval — never silent.

```json
{
  "count": 2,
  "repos": [
    {
      "path": "/abs/path/to/sibling-repo-1",
      "declarationSource": "pnpm",
      "hasGit": true,
      "hasClaudeDir": false
    },
    {
      "path": "/abs/path/to/sibling-repo-2",
      "declarationSource": "explicit",
      "hasGit": true,
      "hasClaudeDir": true
    }
  ],
  "discoveryMethod": "pnpm,explicit",
  "_meta": {
    "oracleVersion": "1.0",
    "computeMs": 47,
    "schemaVersion": 1
  }
}
```

**Field invariants** (every consumer relies on these — do not loosen without bumping `_meta.schemaVersion`):

- `count` — non-negative integer; **always equals `repos.length`** (no exceptions).
- `repos[]` — array; possibly empty.
- `repos[].path` — **always absolute**, normalized to forward slashes regardless of platform (`C:/...` on Windows, `/...` on POSIX, `/c/...` on Git Bash). Relative paths in declaration files are resolved against the project root.
- `repos[].declarationSource` — one of `"explicit"`, `"pnpm"`, `"cargo"`, `"npm"`. Reflects the **winner after multi-source dedup**, not necessarily every source the path was declared in.
- `repos[].hasGit` — **always `true`**. Entries without a `<path>/.git/` directory are filtered before this output. Field is retained for future-proofing in case the filter weakens (e.g., to support submodules in v2).
- `repos[].hasClaudeDir` — `true` if `<path>/.claude/` exists; `false` otherwise. Informational signal that SHARED-CSI-05 auto-creation is unnecessary for this repo.
- `discoveryMethod` — comma-separated list of sources that contributed at least one **surviving** entry (post-filter, post-dedup), in priority order (`explicit,pnpm,cargo,npm`). Special value `"none"` when zero entries survived.
- `_meta.oracleVersion` — `"1.0"` for this arc.
- `_meta.computeMs` — non-negative integer; wall-clock millis from invocation to emit.
- `_meta.schemaVersion` — `1` for this arc.

**Empty-result form** (no declaration files, all declarations malformed, or all entries filtered out):

```json
{
  "count": 0,
  "repos": [],
  "discoveryMethod": "none",
  "_meta": {"oracleVersion": "1.0", "computeMs": 12, "schemaVersion": 1}
}
```

This is the **graceful-absence contract** — consumers detect "no shared repos to coordinate with" by `count == 0`, not by the oracle erroring.

### **FROZEN SNAPSHOT SCHEMA — load-bearing contract**

This schema is consumed by SHARED-CSI-02 (writer) and SHARED-FINALIZE-02 (reader). It governs the per-shared-repo dirty-state snapshot artifact captured at session-start and consumed at `/0-uldf-finalize` time. Defined here in the discovery oracle's README because both Stage 2 workers need a single source of truth — colocating it with the discovery contract means consumers don't have to chase down two separate frozen-schema documents.

**Snapshot location**: `.claude/session-state/shared-repo-snapshots/<sha256-hex-12>.json`

- The file lives in the **LOCAL** project's `.claude/session-state/`, not in the shared repo.
- Gitignored (`.claude/session-state/` is gitignored by `/0-uldf-setup-project`-installed `.gitignore`).
- Session-scoped: written at session-start, consumed at `/0-uldf-finalize` time, overwritten by the next session-start.
- **Filename derivation (FROZEN — load-bearing for both writer and reader)**: `sha256(<absolute_repo_path>)`, then take the **first 12 hex characters** as the filename stem. Append `.json`. Example: `/abs/path/sibling-a` → sha256 hex starts with `b3a91c4f7e02...` → snapshot filename is `b3a91c4f7e02.json`.
- **Why hash-derived**: collision-free by construction. Earlier basename-based naming with parent-dir disambiguation had a writer/reader contract gap — writer disambiguated, reader did not, producing wrong-snapshot reads when two shared repos shared a basename (e.g., `apps/foo` + `libs/foo`). Hash derivation eliminates the entire collision concept; both sides compute the filename identically from the absolute path alone (C-002 fix per Stage 3 critic of the CSI Phase 1.5 implementation arc).
- **Reference implementations**: `_csi_shared_snapshot_filename` in `claude-template/scripts/lib/shared-repos.sh`; `Get-CsiSharedSnapshotFilename` in `claude-template/scripts/lib/shared-repos.ps1`. Both fall back through sha256sum / shasum / openssl / python; on total hash-tool failure, fall back to a deterministic basename-derivation that is also collision-free per pair (path-encoded).

**Schema**:

```json
{
  "schemaVersion": 1,
  "repoPath": "/abs/path/to/sibling-repo",
  "capturedAt": "2026-04-30T14:23:45Z",
  "sessionId": "claude-shell-<id>-from-this-session.json",
  "dirtyFiles": [
    {
      "path": "src/lib/foo.ts",
      "status": "M",
      "contentHash": "a3f5c8...64-hex-chars..."
    },
    {
      "path": "newfile.ts",
      "status": "??",
      "contentHash": "b8e2f1...64-hex-chars..."
    }
  ]
}
```

**Field invariants**:

- `schemaVersion` — always `1` in this arc. Future bumps require schemaVersion increment + decision-channel entry; consumers MUST check this field and refuse on mismatch.
- `repoPath` — absolute path of the shared repo (matches `repos[].path` from the discovery output).
- `capturedAt` — ISO-8601 UTC timestamp (`yyyy-MM-ddTHH:mm:ssZ`) of snapshot.
- `sessionId` — this session's id from `.claude/session-state/this-session.json` (CSI-01 contract).
- `dirtyFiles[]` — array; possibly empty. Empty means the shared repo was clean at session start.
- `dirtyFiles[].path` — **relative to the SHARED REPO's root**, NOT the local project root. SHARED-CSI-02 captures via `git -C <sharedRepo> status --porcelain`, which already emits repo-relative paths.
- `dirtyFiles[].status` — git two-letter porcelain status code (`M`, `A`, `D`, `R`, `C`, `??`, ` M`, etc.). For snapshot-comparison purposes, the **first character (working-tree status)** is what matters (e.g., `??` for untracked, `M` for modified, `A` for added).
- `dirtyFiles[].contentHash` — **SHA-256 hex** (64 lowercase hex chars) of the file content at snapshot time. SHARED-FINALIZE-02 compares this against the same file's hash at `/0-uldf-finalize` time:
  - Hash unchanged → file is **pre-existing dirty** (Set 2; not session-authored)
  - Hash changed → file is **session-authored** (Set 1)
  - File not in snapshot, dirty at finalize → **session-authored**
  - File in snapshot, clean at finalize → **session reverted/restored** (no action needed)

**Hash algorithm is mandatory**: SHA-256. No alternative algorithms in v1. Future bumps require schemaVersion increment.

**Empty `dirtyFiles[]` is valid**: Consumer interpretation is "no pre-existing baseline; every dirty file at finalize is session-authored." This is the common case (clean shared repo at session-start).

**Snapshot lifetime**: Session-scoped. The next session-start overwrites; the snapshot is not preserved across sessions. Consumers MUST NOT rely on stale snapshots — SHARED-CSI-02 always re-captures at every session-start invocation.

## 4. Constraints & Business Rules

### Discovery format scope (DEC-35)

Four supported sources, in priority order:

1. **`.claude/config.json` `sharedRepos`** — always wins; escape hatch for non-standard layouts. Two shapes: array of strings, OR array of `{path, role?}` objects.
2. **`pnpm-workspace.yaml` `packages:`** — list of literal paths or glob patterns.
3. **`Cargo.toml` `[workspace] members`** — array of literal paths or glob patterns.
4. **`package.json` `workspaces`** — array of strings, OR object with `packages: [...]`.

**Out of scope for v1** (DEC-35):

- **Submodules** (`.gitmodules`) — different lifecycle (detached HEAD, ref pinning, separate push targets). Submodule consumers use the explicit-list escape hatch pointing at the submodule's checkout path.
- **Lerna/Nx/Bazel** — custom workspace runners; explicit-list escape hatch covers them.
- **Nested workspace packages** — packages within the local working tree are filtered out. They are not separate repos; their files belong to the local working tree and are handled by the existing local-finalize logic.

### Filter contract

A discovered candidate path becomes a `repos[]` entry only if **all** of:

1. `<path>/.git/` directory exists (filter out non-git checkouts and nested workspace packages without their own git repo).
2. Path is not the local working tree itself (degenerate self-reference dropped).
3. Path is not strictly inside the local working tree (nested workspace package, not a sibling).

### Dedup contract

When the same absolute path appears in multiple declaration sources:

- The **highest-priority source wins** (`explicit > pnpm > cargo > npm`).
- The entry's `declarationSource` reflects the winning source.
- The losing source still contributes to `discoveryMethod` ONLY IF some path it declared survived as the winner of its own dedup. A source that contributed nothing surviving is omitted from `discoveryMethod`.

Concrete example: if `../sibling-a` is in BOTH `pnpm-workspace.yaml` AND `.claude/config.json`, and that's the only path discovered:

- `repos[0].declarationSource` = `"explicit"` (explicit beats pnpm)
- `discoveryMethod` = `"explicit"` (pnpm contributed no surviving entry post-dedup)

### Path resolution

- Paths in `pnpm-workspace.yaml`, `Cargo.toml`, and `package.json` resolve **relative to the file's directory** (which is the project root for all three).
- Paths in `.claude/config.json` `sharedRepos` resolve **relative to the project root**, NOT the `.claude/` directory. This is convention — users expect `"../sibling"` in config.json to mean a sibling of the project root, not a sibling of `.claude/`.

### Glob expansion

Patterns containing `*`, `?`, or `[...]` are expanded against the filesystem at the resolution-base directory. Used by pnpm + npm primarily; Cargo's `members` rarely uses globs but is handled identically.

Empty glob expansion (no matches) is silently no-op — the pattern contributes zero entries.

### Compute budget

Target: **80ms** per invocation (manifest `compute_cost_ms: 80`).

Achievable via simple file reads + glob expansion + `[ -d ... ]` checks. Worst case (3 declaration files + 5 glob patterns expanding to 10 candidates each) stays well under 200ms on observed Windows PowerShell + Git Bash runs.

PowerShell process-spawn overhead inflates the wall-clock timing of the validate harness's recorded `computeMs` (~100-150ms in T1-T8 runs) but the inside-script work itself is fast — the budget refers to the inner work, not the cold-start cost.

## 5. Relationships & Dependencies

### Upstream

- **`.claude/config.json`** — optional; consumed for explicit list and `csi.registryHygieneThreshold` configuration in sibling oracles (this oracle does not consume threshold config).
- **`pnpm-workspace.yaml`, `Cargo.toml`, `package.json`** — optional; consumed for workspace-runner discovery sources.

### Downstream consumers

| Consumer | Where | What it reads |
|---|---|---|
| Session-start hook (CLAUDE-B; SHARED-CSI-02) | `claude-template/hooks/session-start.{sh,ps1}` | Full output; iterates `repos[]` for cross-repo registration + snapshot capture |
| Session-start briefing (CLAUDE-B; SHARED-CSI-03) | Same hook | `repos[]` paths used to read each shared-repo `active-sessions.json` for cross-correlation |
| Session-start hygiene sweep (CLAUDE-B; SHARED-CSI-06) | `dispatchable-sessions --gc-cheap` extension | `repos[]` paths drive the per-shared-repo `--gc-cheap` extension loop |
| `/0-uldf-finalize` setup (CLAUDE-C; SHARED-FINALIZE-01) | `claude-template/segments/0-uldf-finalize/setup.md` | Full output; cached for the duration of the finalize invocation |
| `/0-uldf-finalize` per-repo flow (CLAUDE-C; SHARED-FINALIZE-04) | `claude-template/segments/0-uldf-finalize/shared-repo-flow.md` | `repos[]` paths drive the per-repo finalize loop |
| `/0-uldf-ltads-stop` Phase 4.5 (CLAUDE-C; SHARED-CSI-04) | `claude-template/segments/-ltads/stop_phases.md` | `repos[]` paths drive cross-repo arc-terminus close |
| `/0-uldf-pods-converge` Phase 7 (CLAUDE-C; SHARED-CSI-04) | `claude-template/segments/-pods/converge_phases.md` | `repos[]` paths drive cross-repo arc-terminus close + opportunistic archive-retention sweep |

### Sibling oracles

- **`dispatchable-sessions`** — different question (live peers vs. workspace declarations) but identical structural skeleton; CSI-05 was the primary precedent for this oracle's design (see DISC-CSI-09).
- **`archive-retention`** — different question, same skeleton, different substrate (filesystem dirs); secondary precedent.

This oracle does **not** expose `--gc` / `--gc-cheap` modes. Discovery oracles do not sweep state. The shared cleanup-oracle skeleton (sweep-then-summarize, time-budget loop, atomic-append summary) is irrelevant to discovery — it's a `cleanup` category pattern, not a `discovery` category pattern.

## 6. Decision Log

- **No `--gc` / `--gc-cheap` modes** — discovery oracles report state; they don't sweep it. Stage 2 cleanup mechanisms (SHARED-CSI-04 arc-terminus close, SHARED-CSI-06 cross-repo `--gc-cheap`) live in the consumers (hook + `/0-uldf-finalize` segments), not in the discovery oracle. Keeping discovery and cleanup separate matches the existing `discovery` vs. `cleanup` category split in INDEX.md.

- **Frozen schemas at first commit** — both the output schema and the snapshot schema are load-bearing contracts that Stage 2 workers consume in parallel. Locking them in this README means CLAUDE-B and CLAUDE-C can read one document and start writing consumer code without waiting for additional schema design. Drift detection: validate harness asserts the output schema explicitly (T8: `_meta` block present, schemaVersion=1).

- **Snapshot schema lives in this README, not the SPECIFICATION** — colocation reasoning: SHARED-CSI-02 captures the snapshot, SHARED-FINALIZE-02 consumes it. Both reference this oracle's output schema regardless. Putting the snapshot schema next to the output schema means consumers read one document, not two. The SPECIFICATION still references SHARED-CSI-01 acceptance criteria (output schema presence), and the snapshot schema is referenced in SHARED-CSI-02 acceptance.

- **`.claude/config.json` paths resolve to project root, not `.claude/`** — convention. Users expect `"../sibling"` to mean a project-root sibling, not a `.claude/`-sibling. Documented explicitly above so future contributors don't change it silently.

- **Pure age + git filter, no auto-detect via directory walking (DEC-35)** — auto-detecting "any sibling dir with `.git/` is shared" produces too many false positives in deep monorepos. Workspace-declaration-file presence is the user's explicit declaration of intent.

- **Empty result is a JSON object with `count: 0`, NOT exit 1** — graceful absence is the consumer contract. Stage 2 workers detect "no shared repos to coordinate with" by parsing JSON and reading `count == 0`. Erroring would force every consumer to handle a separate "no shared repos" code path, which would scale poorly across 4 consumers (hook + 3 finalize phases) and 2 platforms.

- **Skeleton transferred from `dispatchable-sessions` per DISC-CSI-09** — manifest shape, mode-dispatch arg parsing, JSON-parser fallback chain (jq → python → degrade), millisecond timer, validate-harness organization. Net new code: workspace-declaration parsing (pnpm YAML, Cargo TOML, npm/explicit JSON), dedup with priority, glob expansion, self/nested filter. Roughly 70/30 transfer/new per DISC-CSI-09.

## 7. Cross-References

- **Manifest**: `oracle.json`
- **Spec**: `docs/specs/SPECIFICATION.md` § SHARED-CSI-01 (acceptance criteria)
- **Decision (discovery scope)**: `docs/specs/DECISIONS.md` DEC-35
- **Discovery (skeleton lineage)**: `docs/specs/DISCOVERIES.md` DISC-CSI-09
- **Plan**: `docs/planning/plans/20260430T131500-shared-repo-aware-finalize-cross-repo-cs.md` § Stage 1, § Frozen Output Schema, § Frozen Snapshot Schema, § Acceptance gating
- **Skeleton precedents**: `claude-template/oracles/dispatchable-sessions/`, `claude-template/oracles/archive-retention/`
- **Foundation**: `FOUNDATIONS/ORACULURGY_DESIGN.md` (project-state oracle category) and `FOUNDATIONS/PRINCIPLES_OF_LLM_AGENT_ORCHESTRATION.md` § 2.12 (Oraculurgy)
- **CSI Phase 1.5 framing**: `FOUNDATIONS/CSI_DESIGN.md` (Stage 4 will extend with § 6 Cross-Repo Phase 1.5 — pending LD authorship)

## 8. Testing

```bash
bash .claude/oracles/workspace-shared-repos/validate.sh   # Unix / Git Bash
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .claude/oracles/workspace-shared-repos/validate.ps1  # Windows
```

Validates eight scenarios:

| Test | What it asserts |
|---|---|
| **T1** | `pnpm-workspace.yaml` discovery — literal paths + glob expansion both work; non-git filtered out |
| **T2** | `Cargo.toml` `[workspace] members` discovery — multi-line bracket form parses; `[workspace.dependencies]` sub-section doesn't bail the parser |
| **T3** | `package.json` `workspaces` discovery — both array form (`["pkg1"]`) and object form (`{"packages":["pkg1"]}`) |
| **T4** | `.claude/config.json` `sharedRepos` discovery — both bare-string and `{path, role}` object shapes |
| **T5** | Multi-source dedup — same path in pnpm AND explicit; explicit wins; `discoveryMethod` reflects winner only |
| **T6** | Skip non-git — declared path without `.git/` is filtered out |
| **T7** | Skip self — declaration pointing back at the project itself is dropped |
| **T8** | Graceful empty — no declaration files → `{count:0, repos:[], discoveryMethod:"none", _meta:{...}}` |

Both harnesses run identically — 37 PASS / 0 FAIL on Git Bash and Windows PowerShell at first commit.
