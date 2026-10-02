---
status: done
commissioned_by: ULDF, session 34404efe-79ee-4649-a852-c9a5c3f91e99 (S: drive investigation, as lead)
origin: S: ran out of space; a whole-drive measurement named this project among the largest/fastest-growing
harm: witnessed
beneficiary: product
deferred_because: another-project
---

# Reclaim this project's disk space on S: and stop it regrowing

## Context (from the ULDF lead session that measured the whole S: drive, 2026-10-01 ~23:45)

S: (1 TB) ran out of space completely at least once in the last few days (0 bytes free; ~117 GB
lost in 3 days). Consequences already witnessed: Docker Desktop's data disk will not mount, and the
Gitea server's data filesystem went read-only, so pushes to the git server fail machine-wide. The
lead session is handling Docker/Gitea recovery — **do not touch Docker, WSL or `S:/DockerDesktopWSL`.**
Note that pushes may fail until Gitea is back; commit locally and say so.

## What this project holds (measured: total GB / GB written in the last 5 days)

- `target/debug` 59 GB (deps 30, incremental 28); 30 GB of it written in the last 5 days.

## The task

1. Re-measure (the numbers above are a snapshot). Find what is regenerable build output, what is
   residue (worktrees whose session is gone and whose branch is merged or pushed), and what is real.
2. Reclaim the space. Regenerable output (`target/`, `incremental/`, `.next`, `.turbo`, android
   `build/`, stale `node_modules` in a dormant tree) may be deleted. A worktree is removed only when
   no live session owns it (`ListAgents`, the project's registry/collaboration state — an mtime is
   not a check) **and** it holds no uncommitted or unpushed work; when in doubt, commit or copy aside
   first, then remove. Never delete anything a live session is using.
3. **Stop it recurring** — this is the part only this project can know. Why does it grow this fast?
   (e.g. every worktree building its own full Rust `target/` — a shared `CARGO_TARGET_DIR`, sccache,
   `cargo clean -p`/`cargo sweep` after merge, worktree disposal that deletes build output, debuginfo
   settings in `[profile.dev]`). Land the durable fix in this project if it is clearly yours; if the
   cause is in the framework (worktree lifecycle), say so in your closing block so the lead can route it.
4. Report: GB before, GB after, what you deleted, what you changed to prevent recurrence.

## Outcome (2026-10-02)

- Before: `target/` 54 GB (deps 30 GB, of which 21 GB were `.pdb` files at ~100 MB per test
  binary; incremental 24 GB in 545 session dirs). No linked worktrees, no live peers.
- Deleted `target/` whole (regenerable). Nothing else removed.
- Recurrence fix `fa2358a`: `[profile.dev] debug = "line-tables-only"` and
  `[profile.dev.package."*"] debug = false`. A full clippy + test build is now 4.3 GB, ~19 MB per
  `.pdb`. Release/Docker builds unchanged. Full finalize gate green, critic PASS, pushed.
- Not fixed here: stale incremental session dirs still accumulate (smaller now); an occasional
  `cargo clean` reclaims them. Worktree build output is the framework's, filed by the lead as
  `S:/ULDF/docs/planning/deferred/20261001-2358-worktree-build-output-fills-the-drive.md`.
