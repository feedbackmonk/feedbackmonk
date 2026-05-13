# Fixture: cargo-workspace

## Synopsis

Test fixture: minimal `Cargo.toml` with `[workspace] members` (two valid sibling git repos + one non-git filtered out + a `[workspace.dependencies]` sub-section to verify the parser doesn't bail). Used by the `workspace-shared-repos` oracle's T2 validate scenario. Don't edit without re-running `validate.sh` / `validate.ps1`.

## Layout

`Cargo.toml` declares three workspace members:

- `../sibling-a` — present, has `.git/` → expected in output
- `../sibling-b` — present, has `.git/` → expected in output
- `../not-a-git-checkout` — present but lacks `.git/` → filtered out

Includes a `[workspace.dependencies]` sub-section to verify the parser doesn't bail on it (the parser must stay in `[workspace]` scope across recognized sub-sections like `[workspace.dependencies]` and `[workspace.metadata.*]`).

The validate harness assembles a sandbox at runtime mirroring this directory structure. The oracle's CWD is `<sandbox>/project/`, where this fixture's `Cargo.toml` is placed.
