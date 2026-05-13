# markdown-link-validity Oracle

## Synopsis

Verification Oracle (`kind: "verification"` per Oraculurgy Part 11) that catches broken internal markdown links across `claude-template/`, `docs/`, `FOUNDATIONS/`, root `README.md`, and root `CLAUDE.md`. Fires reflexively in the develop/test/fix inner loop and at `/0-uldf-finalize` Phase 1a; broken-link entries carry source path + line number + resolved-path so an agent can open exactly the right edit point. Don't come here for external URL liveness, anchor-existence-within-target checks, reference-style links (`[text][label]`), or HTML `<a>` tags — all out of scope by design.

> **Category**: documentation | **Kind**: verification | **Strategy**: always-fresh

Checks that every internal markdown link of the form `[text](path)` in the project's tracked documentation files resolves to an existing target on disk. Returns a structured report listing any broken links with file paths, line numbers, and the resolved-path that failed.

This is the project's first **Verification Oracle** — an oracle that answers *"did anything break?"* rather than *"what is in this project?"*. See `FOUNDATIONS/ORACULURGY_DESIGN.md` § Verification Oracles for the full category specification.

## Why this oracle exists

ULDF has many cross-document links (FOUNDATIONS → docs → claude-template → commands → segments). A broken link silently degrades agent navigation: the agent reads a doc, follows a link, finds nothing, and burns tokens reverse-engineering where the referenced material actually lives. Catching the break the moment it happens is far cheaper than the cumulative cost of every future session that hits the dead link.

## Scope

The oracle scans:

- All `*.md` files under `claude-template/`, `docs/`, `FOUNDATIONS/` (recursive)
- Root-level `CLAUDE.md` and `README.md`

The scope is set in two places that must stay aligned: the `config.scan_directories` and `config.scan_root_files` arrays in `oracle.json` (advisory; documents the contract), and the `SCAN_DIRS` / `SCAN_ROOT_FILES` arrays at the top of `run.sh` and `run.ps1` (authoritative; what actually runs). Edit all three when changing scope.

## What it checks

For every `[text](dest)` and `![alt](dest)` match, the oracle:

1. Strips a trailing `"title"` from the destination if present (per markdown spec)
2. Skips destinations that are external (`http://`, `https://`, `ftp://`, `mailto:`, `tel:`) or same-page anchors (`#…`)
3. Strips `?query` and `#fragment` from the remainder
4. Resolves the result relative to the source file's directory (absolute paths starting with `/` are kept as-is)
5. Reports the link as broken if the resolved path does not exist on disk

## What it deliberately does NOT check

- **Reference-style links** (`[text][label]` with a separate `[label]: path`) — not yet supported. Add an issue if encountered.
- **Bare backtick file references** (e.g., `` `docs/specs/SPECIFICATION.md` ``) — these are *prose references*, not links, and would require a different oracle (`doc-reference-validity`) with very different semantics.
- **HTML `<a href="">`** — markdown is the focus; HTML anchors in markdown files are out of scope.
- **External URL liveness** — out of scope by design (would violate the always-fresh + read-only contract for any project on a slow network).
- **Anchor existence within target file** — only file existence is checked. A link to `foo.md#section-three` passes if `foo.md` exists, even if the anchor doesn't.

## Output

```json
{
  "status": "pass",
  "details": {
    "checked": 87,
    "broken_count": 0,
    "scanned_files": 234,
    "scan_duration_ms": 380,
    "broken": []
  }
}
```

When broken links are found:

```json
{
  "status": "fail",
  "details": {
    "checked": 87,
    "broken_count": 1,
    "scanned_files": 234,
    "scan_duration_ms": 410,
    "broken": [
      {
        "source": "FOUNDATIONS/EXAMPLE.md",
        "line": 42,
        "link": "../docs/missing.md#section",
        "resolved_path": "FOUNDATIONS/../docs/missing.md"
      }
    ]
  }
}
```

Each broken entry is **agent-actionable**: the source file + line number lets an agent open exactly the right edit point, and `resolved_path` shows what was checked (so the agent can decide whether to fix the link or create the missing target).

## Speed contract

The oracle's freshness contract is `always-fresh` with `compute_cost_ms: 500`. The Verification Oracle category requires `<2s` runtime on a typical ULDF-sized project. If this oracle ever exceeds 2s on the ULDF repo, scope it down (limit to recently-changed files via Phase 3 git diff input) before shipping.

Measured on the ULDF repo as of authoring: ~400ms for ~230 files / ~90 links.

## Idempotence

The oracle is read-only. It never modifies the filesystem (including no `cache/` writes). Calling it twice in a row returns the same answer for the same project state.

## Invocation

```bash
# Unix / Git Bash
bash .claude/oracles/markdown-link-validity/run.sh

# Windows PowerShell
powershell -NoProfile -File .claude/oracles/markdown-link-validity/run.ps1
```

Both must produce structurally identical JSON output; only field ordering may differ.

## When to consult this oracle

- **Reflexively** after any documentation refactor (renames, moves, deletions of `*.md` files)
- **Before committing** a change that touches many markdown files (the `/0-uldf-finalize` Phase 11 audit will revalidate automatically)
- **Paired with an Agent UI Fixture** for documentation: if a fixture documents the expected structure of a doc set, this oracle is the drift-detection partner per `FOUNDATIONS/ORACULURGY_DESIGN.md` § Verification Oracles → Composition with Project-State Oracles

## Fallback (Graceful Absence)

If this oracle is missing or broken, the agent can manually grep for `\[[^]]*\]\([^)]+\)`, filter out `http(s)://` / `mailto:` / `tel:` / `#…`, strip anchors and queries, and `test -f` (or `Test-Path`) each resolved path. Slow and error-prone vs. the oracle, but the workflow continues.

## Self-test

(Not yet authored — `validate.{sh,ps1}` is referenced in the manifest but stubbed. Future work: a fixture directory containing one good link and one deliberately broken link, with assertions that the oracle returns exactly the broken one.)
