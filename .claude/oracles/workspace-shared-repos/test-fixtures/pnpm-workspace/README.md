# Fixture: pnpm-workspace

## Synopsis

Test fixture: minimal `pnpm-workspace.yaml` with literal sibling paths plus a `../glob-*` pattern to exercise glob expansion (T1 + T6 in the `workspace-shared-repos` oracle's validate harness). Don't edit without re-running `validate.sh` / `validate.ps1`.

## Layout

`pnpm-workspace.yaml` declares four packages:

- `../sibling-a` — present, has `.git/` → expected in output
- `../sibling-b` — present, has `.git/` → expected in output
- `../not-a-git-checkout` — present but lacks `.git/` → filtered out (T6)
- `../glob-*` — glob expansion; matches `../glob-foo` (with `.git/`) → expected in output

The validate harness assembles a sandbox at runtime mirroring this directory structure. The oracle's CWD is `<sandbox>/project/`, where this fixture's `pnpm-workspace.yaml` is placed.
