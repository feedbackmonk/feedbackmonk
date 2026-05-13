# Fixture: explicit-list

## Synopsis

Test fixture: minimal `.claude/config.json` `sharedRepos` array exercising both shapes (bare-string and `{path, role}` object) plus a non-git entry to prove filtering. Used by the `workspace-shared-repos` oracle's T4 validate scenario; also the canonical example of the resolution rule that paths in `.claude/config.json` resolve against the project root, NOT `.claude/`. Don't edit without re-running `validate.sh` / `validate.ps1`.

## Layout

`.claude/config.json` `sharedRepos` field declares three siblings via two shapes:

- `{"path": "../sibling-a", "role": "shared-utility"}` — object shape with role
- `"../sibling-b"` — bare string shape
- `{"path": "../not-a-git-checkout", "role": "should-be-filtered"}` — present but lacks `.git/` → filtered out

The validate harness places this fixture's `config.json` at `<sandbox>/project/.claude/config.json`. Paths in `.claude/config.json` resolve against the project root (NOT the `.claude/` directory) — `../sibling-a` → `<sandbox>/sibling-a`.
