# markdown-link-validity Oracle

## Synopsis

Verification Oracle (`kind: "verification"` per Oraculurgy Part 11) that catches broken internal markdown links across `claude-template/`, `docs/`, `FOUNDATIONS/`, root `README.md`, and root `CLAUDE.md`. Fires reflexively in the develop/test/fix inner loop and at `/0-uldf-finalize` Phase 1a; broken-link entries carry source path + line number + resolved-path so an agent can open exactly the right edit point. Don't come here for external URL liveness, anchor-existence-within-target checks, reference-style links (`[text][label]`), or HTML `<a>` tags — all out of scope by design. <!-- path-ok: oracle scans framework source tree -->

> **Category**: documentation | **Kind**: verification | **Strategy**: always-fresh

Checks that every internal markdown link of the form `[text](path)` in the project's tracked documentation files resolves to an existing target on disk. Returns a structured report listing any broken links with file paths, line numbers, and the resolved-path that failed.

This is the project's first **Verification Oracle** — an oracle that answers *"did anything break?"* rather than *"what is in this project?"*. See `FOUNDATIONS/ORACULURGY_DESIGN.md` § Verification Oracles for the full category specification.

## Why this oracle exists

ULDF has many cross-document links (FOUNDATIONS → docs → claude-template → commands → segments). A broken link silently degrades agent navigation: the agent reads a doc, follows a link, finds nothing, and burns tokens reverse-engineering where the referenced material actually lives. Catching the break the moment it happens is far cheaper than the cumulative cost of every future session that hits the dead link.

## Scope

The oracle scans:

- All `*.md` files under `claude-template/`, `docs/`, `FOUNDATIONS/` (recursive) <!-- path-ok: oracle scans framework source tree -->
- Root-level `CLAUDE.md` and `README.md`

The scope is set in two places that must stay aligned: the `config.scan_directories` and `config.scan_root_files` arrays in `oracle.json` (advisory; documents the contract), and the `SCAN_DIRS` / `SCAN_ROOT_FILES` arrays at the top of `run.sh` and `run.ps1` (authoritative; what actually runs). Edit all three when changing scope.

## What it checks

For every `[text](dest)` and `![alt](dest)` match, the oracle:

1. Strips a trailing `"title"` from the destination if present (per markdown spec)
2. Skips destinations that are external (`http://`, `https://`, `ftp://`, `mailto:`, `tel:`) or same-page anchors (`#…`)
3. Strips `?query` and `#fragment` from the remainder
4. Resolves the result relative to the source file's directory (absolute paths starting with `/` are kept as-is)
5. Reports the link as broken if the resolved path does not exist on disk

## What it asserts vs. what it measures (OVALID-02)

The validity declaration is authoritative in `oracle.json` → `assertion`; this is the prose version. Read it before trusting a verdict in either direction.

| | |
|---|---|
| **Asserts (P)** | every internal markdown citation in the scanned docs names a real target, so a reader following it lands somewhere |
| **Measures (Q)** | for each `[text](dest)` outside code fences and code spans, whether the path resolved against the **citing file's directory** exists in the **working tree right now** |

The gap between them is where this oracle can lie, so it is written down rather than left to be rediscovered:

- **Working tree, not `HEAD` — handled, not accepted (OVALID-05).** ULDF's default topology shares one working tree between live sibling sessions, so another session's *uncommitted* deletion of a tracked target used to make a **correct** citation report broken — the file is present in `HEAD`, the doc is fine, and nothing the committing agent could do would clear it. Since v1.2.0 such targets are detected via `git ls-files --deleted` and routed to an informational `uncommitted_deletions` bucket that does **not** set `status`. **The gate is deferred, not weakened**: commit the deletion and the path leaves that set and fails as a genuine broken link. Not a blanket amnesty either — a genuinely absent target still fails in the same run (self-test T6a/T6b/T6c, each demonstrated RED against a blanket-amnesty mutant). No git, or not a work tree → the set is empty and the oracle is strict exactly as before.
- **Existence, not correctness.** A target that still exists but no longer says what the citing sentence claims **passes**. This oracle cannot see content, and a clean report is not a claim that the citation is *apt*.
- **Anchors are stripped**, so a citation of a heading that has since been renamed passes (see the NOT-checked list below).
- **Citing-file-relative resolution only.** A repo-root-relative path written without the relative prefix resolves to the wrong place and is reported broken. That is the oracle being right about Q and wrong about P in the *other* direction, and the fix is to the citation.
- **Scope-bounded.** A citation in a directory outside `config.scan_directories` is *invisible*, not passing.

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
    "broken": [],
    "uncommitted_deletion_count": 0,
    "uncommitted_deletions": []
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
    ],
    "uncommitted_deletion_count": 0,
    "uncommitted_deletions": []
  }
}
```

Each broken entry is **agent-actionable**: the source file + line number lets an agent open exactly the right edit point, and `resolved_path` shows what was checked (so the agent can decide whether to fix the link or create the missing target).

`uncommitted_deletions[]` carries the same entry shape but is **informational and never sets `status`** — the citation is correct against `HEAD` and the target's absence is somebody's in-flight WIP. It is emitted rather than dropped so the fact is visible, not so it becomes a fix-me list. `broken_count` and `uncommitted_deletion_count` are disjoint: an entry appears in exactly one bucket.

## Speed contract

The oracle's freshness contract is `always-fresh`; declared cost is `expected_runtime_ms: 1500`. The Verification Oracle category requires `<2s` runtime on a typical ULDF-sized project. If this oracle ever exceeds 2s on the ULDF repo, scope it down (limit to recently-changed files via Phase 3 git diff input) before shipping.

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

`validate.sh` / `validate.ps1` — behaviour-equivalent twins, seven cells each, run from hermetic sandboxes under `TMPDIR` (never against this repo's own docs):

| Cell | Asserts |
|---|---|
| T1 clean-tree | `pass`, `broken_count: 0`, external links excluded from `checked` |
| T2 one-broken | `fail`, the broken link named with correct source/line/resolved_path, **and the good link NOT reported** |
| T3 exclusions-only | `http`/`https`/`ftp`/`mailto`/`tel`/`#anchor` are never checked |
| T4 relative-nesting | `../../` resolution from a nested dir; good vs broken discriminated |
| T5 empty-scope | graceful absence — `pass`, `scanned_files: 0` |
| T6a/T6b | uncommitted deletions informational (**count 2, one of them parent-relative** — that cell is what pins the path canonicalization) **while a genuinely absent target still fails in the same run** |
| T6c | committing the deletions restores the failures — deferred, not weakened |

⚠ **A passing self-test here establishes correctness, not validity** (OVALID-03). The cells feed synthetic inputs to *the measurement* and assert *the measurement's* outputs; they cannot tell you the measurement was the right one to make. That is what the § "What it asserts vs. what it measures" table above is for, and what `/0-uldf-oracle --validity` reviews.

**Falsifiability**: T6a/T6b was demonstrated RED against two mutants before shipping — a blanket-amnesty classifier (`if true` in place of the deletion test → T6b red) and a neutered path canonicalizer (identity `norm_path` → T6a red on the parent-relative leg only). Each mutant isolates to the cell that names it, so neither cell is over-determined (the DISC-TEST-03 lesson).
