# cloud-sync-coordination-hazard

**Question**: Is this project's coordination store inside a cloud-synced folder, where a background sync client can silently discard a write the lock believed it had serialized?

**Kind**: `project-state` · **Lane**: `fast` · **Runs**: every session (advisory line, suppressed when clean)

## Why this exists

`registry-write.{sh,ps1}`'s mkdir-based lock serializes processes against **one machine's local filesystem view**. It has no leverage over what happens *after* a write returns success: on synced storage, two near-concurrent writers can each write and read back their own version locally, and the sync client then picks one version as canonical and discards the other — after the fact, below the lock.

This is not theoretical. **2026-07-12 (DISC-CSI-25)**: two writers appended to `grant-requests.jsonl` about two minutes apart. Both were independently verified written and readable at write-time. The final file contained only one writer's record. A read-visibility lag of over a minute was observed on the same file.

Exposed surfaces are the whole CSI/PODS coordination substrate — `active-sessions.json` (a lost write can orphan a session identity or drop a driver claim), the grant/deferral JSONLs (a gate's "durable grant recorded" claim becomes false), PODS `touches.json` (two workers believe they don't conflict when they do), and turn-state stamps.

## What it does — and deliberately does not do

It **detects and reports**. It does not relocate the store, does not block, and does not attempt to defeat sync reconciliation.

That scoping is the decision recorded in DEC-188. Relocating coordination state out of the synced tree would kill the hazard class outright, but it is a large, invasive change to load-bearing plumbing across two shells — and the confirmed footprint at the time was one project on one machine. Per `ENFORCEMENT_PLACEMENT_PRINCIPLE.md`, state-corruption with a logged live failure is on the *proactive* side of "build the code", but cost-proportionality (EPP-02) puts detection first. This oracle converts a silently latent hazard into a per-session decision, and generates exactly the evidence that would justify escalating to relocation later.

Note the prior art it deliberately diverges from: `segments/-ldis/spec_lock.md` documents this same hazard class for the spec-edit lock and accepts a **document-only** mitigation. Document-only is weaker here because the blast radius is worse — a lost `touches.json` write silently degrades a conflict-detection tier that *other automated mechanisms* trust, whereas a spec-lock collision just makes a human retry.

## Output

Single-line JSON. **Empty `briefing` suppresses the session-start line** — the normal case.

```json
{"hosted":false,"provider":null,"root":"S:\\SourceControlled\\ULDF","matched_segment":null,"at_risk_paths":[],"briefing":""}
```

When hosted, `briefing` names the provider and the risk, and `at_risk_paths` lists only the coordination surfaces that actually exist under that root.

## Silence is not a safety guarantee

An empty briefing means **"no recognized provider segment"** — never "verified safe". An unresolvable project root emits `hosted:false` with an empty briefing rather than guessing, because a fabricated verdict in either direction is worse than silence. Providers not in the list (or mounted by a client that does not put its name in the path) will not be detected.

## Configuration

| Source | Key |
|---|---|
| env | `ULDF_CLOUD_SYNC_PROVIDERS` (comma-separated segment list) |
| project | `.claude/config.json` → `cloudSyncHazard.providers` |
| default | `OneDrive`, `Dropbox`, `Google Drive`, `GoogleDrive`, `iCloud Drive`, `iCloudDrive` |

Matching is **segment-aware and case-insensitive**: a project named `OneDriveTools` does not trip. That false-positive guard is deliberate and tested (T4) — an advisory that cries wolf gets ignored, which is worse than no advisory.

## Self-test

```bash
bash validate.sh          # 13 legs
powershell -File validate.ps1   # same 13, cross-shell parity
```

Both twins are diffed against each other on real output, not just read side by side — that is how a double-escaping bug in the PowerShell JSON writer was caught (it emitted `\\\\` where the shell emitted `\\`, so `root` parsed back wrong).

## Lineage

DISC-CSI-25 (the live incident) → DEFER-016 → **DEC-188**. Design shape follows `disk-artifact-bloat` (cheap every-session environment tripwire, empty-briefing discipline) and `gitignore-template-drift` (session-start advisory nudge).
