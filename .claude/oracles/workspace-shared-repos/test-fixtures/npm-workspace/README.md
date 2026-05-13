# Fixture: npm-workspace

## Synopsis

Test fixture: minimal `package.json` with array-form `workspaces` (two valid sibling git repos + one non-git filtered out). Used by the `workspace-shared-repos` oracle's T3 validate scenario; the harness also exercises the object form (`{packages: [...]}`) inline within T3. Don't edit without re-running `validate.sh` / `validate.ps1`.

## Layout

`package.json` `workspaces` field declares three siblings:

- `../sibling-a` — present, has `.git/` → expected in output
- `../sibling-b` — present, has `.git/` → expected in output
- `../not-a-git-checkout` — present but lacks `.git/` → filtered out

The fixture exercises the array form of `workspaces`. The harness also tests the object form (`{packages: [...]}`) inline as part of T3.

The validate harness assembles a sandbox at runtime mirroring this directory structure. The oracle's CWD is `<sandbox>/project/`, where this fixture's `package.json` is placed.
