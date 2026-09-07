#!/usr/bin/env python3
"""Self-test for the `project-index` oracle -- a C6 cell module.

One cell per ISF-6 fail shape, one per warn kind, and one for a clean index. Every `red=` names
the mutation of `run.py` the cell was observed red against, so "was this ever able to fail?" is
answerable from the cell itself rather than from a driver beside it.
"""

from __future__ import annotations

import importlib.util
import os
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))
if not (pathlib.Path(sys.path[0]) / "uldf").is_dir():
    sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[3] / "scripts" / "lib"))

from uldf import limits, oracle  # noqa: E402
from uldf.cells import T, cell  # noqa: E402

from pathlib import Path  # noqa: E402

HERE = Path(__file__).resolve().parent


def _load_run():
    spec = importlib.util.spec_from_file_location("project_index_run", HERE / "run.py")
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


RUN = _load_run()

#: A rendered global file the restatement shape can be measured against. Every cell that is not
#: ABOUT that shape gets one, so an absent-global warn never rides along and hides the verdict
#: the cell is actually asserting.
GLOBAL_TEXT = ("# CLAUDE.md\n\n"
               "Consent to commit and push is any one of: invoking a finalize command, the words\n"
               "commit or push, or the propagate switch set to auto. None of them is consent to\n"
               "finalize work you have not verified, and that guardrail is the whole point.\n")

CLEAN = ("# CLAUDE.md\n\n"
         "This project renders invoices. Python 3.12, `src/billing/` and `tests/`.\n\n"
         "## Build, run, test\n\n"
         "`python -m pytest`. The dev server takes port 5391 and never binds another.\n\n"
         "## Pending Follow-Ups\n\n"
         "- **When the ledger migration lands** — re-check the rounding rule.\n"
         "  `docs/pending/rounding.md`.\n")

#: The paths CLEAN names. A fixture whose own pointers are dead warns in every cell, and a cell
#: asserting `pass` would then be asserting it around a warn it never meant to create.
CLEAN_PATHS = ("src/billing/__init__.py", "tests/__init__.py", "docs/pending/rounding.md")


def index(root: Path, text: str) -> None:
    (root / "CLAUDE.md").write_text(text, encoding="utf-8")


def clean_root(t: T, root: Path | None = None) -> Path:
    """A project holding CLEAN and the files it points at."""
    root = t.tmpdir() if root is None else root
    for rel in CLEAN_PATHS:
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("x", encoding="utf-8")
    index(root, CLEAN)
    return root


def extra(body: str) -> str:
    """CLEAN plus `body` under a heading of its own.

    The follow-up section is CLEAN's last, so text appended without a heading lands INSIDE it and
    is read -- correctly -- as an inlined body. Only the shape-2 cells want that.
    """
    return CLEAN + "\n## Notes\n\n" + body


def sh(t: T, root: Path, *argv: str) -> None:
    result = t.run(["git", *argv], cwd=str(root), timeout_s=30)
    t.ok(result.rc == 0, f"git {' '.join(argv)} failed: {result.err}")


#: ONE seed repository per process, copied per cell. `git init` plus two `git config` is three
#: process spawns, ~0.5-2.7 s of `SUITE_MS` on Windows against ~34 ms for a directory copy. The
#: seed is built once and never written to again, so nothing MUTABLE is shared and C6 § 6.1 holds.
#: Written out here rather than imported: a starter oracle is installed into a consumer project
#: on its own, where `tests/_seedrepo.py` does not exist.
_SEED: list = []


def repo(t: T) -> Path:
    if not _SEED:
        import atexit
        import shutil
        import tempfile

        seed = Path(tempfile.mkdtemp(prefix="uldf-pi-seed-")) / "repo"
        seed.mkdir(parents=True)
        atexit.register(shutil.rmtree, seed.parent, True)
        sh(t, seed, "init", "-q")
        sh(t, seed, "config", "user.email", "a@example.com")
        sh(t, seed, "config", "user.name", "a")
        for sample in (seed / ".git" / "hooks").glob("*.sample"):
            sample.unlink()
        _SEED.append(seed)

    import shutil

    root = t.tmpdir() / "repo"
    shutil.copytree(_SEED[0], root)
    config = (root / ".git" / "config").read_text(encoding="utf-8", errors="replace")
    t.ok((root / ".git").is_dir(), "the seeded fixture is a git repository")
    t.ok("a@example.com" in config, f"and carries the cell identity email:\n{config}")
    return root


def home_with_global(t: T, text: str = GLOBAL_TEXT) -> Path:
    home = t.tmpdir()
    (home / "CLAUDE.md").write_text(text, encoding="utf-8")
    return home


def measure(root: Path, home: Path) -> dict:
    ctx = oracle.Context(project_root=root, anchor_root=root, home=home)
    return RUN.run(ctx)


def padding(times: int) -> str:
    return "A line of prose about this project's layout and its build.\n" * times


# --- the clean baseline -------------------------------------------------------------------------

@cell(red="mutant: verdict forced to 'fail' -- a clean index reported fail")
def cell_a_clean_index_passes(t: T) -> None:
    """The control. Every other cell asserts a verdict CHANGED by one defect, and that is only
    evidence if the same fixture without the defect passes."""
    r = measure(clean_root(t), home_with_global(t))
    t.eq(r["verdict"], "pass", f"a clean index must pass: {r['summary']}")
    t.eq(r["data"]["finished_work_openings"], [])
    t.eq(r["data"]["inlined_followup_bodies"], [])
    t.eq(r["data"]["oversized_stubs"], [])
    t.eq(r["data"]["restated_global_paragraphs"], [])
    t.eq(r["data"]["unresolvable_paths"], [])
    t.eq(r["data"]["followup_entries"], 1)
    t.ok(r["data"]["global_compared"], "the restatement shape must have been measured")


@cell(red="mutant: the absent-file guard dropped -- no CLAUDE.md raised and became unknown")
def cell_no_index_is_pass_not_unknown(t: T) -> None:
    """No index is not a broken index. `unknown` fails every gate closed, so a project that has
    not written one yet would be unable to finalize until it did."""
    r = measure(t.tmpdir(), home_with_global(t))
    t.eq(r["verdict"], "pass")
    t.eq(r["data"]["bytes"], None)
    t.eq(r["data"]["graded"], False)


# --- fail shape 1: a block opening with a finished-work token ------------------------------------

@cell(red="mutant: `if word in FINISHED_WORK_TOKENS` -> `if False`; a pasted changelog passed")
def cell_fail_a_block_opening_with_a_finished_work_token(t: T) -> None:
    root = clean_root(t)
    index(root, extra("- Migrated the ledger to the v2 schema and deleted the shim.\n"))
    r = measure(root, home_with_global(t))
    t.eq(r["verdict"], "fail", r["summary"])
    found = r["data"]["finished_work_openings"]
    t.eq(len(found), 1, str(found))
    t.eq(found[0]["word"], "migrated")


@cell(red="mutant: the token was matched anywhere in a block, not at its first word -- "
          "'Files added under src/' failed")
def cell_the_token_must_open_the_block_not_appear_in_it(t: T) -> None:
    """Shape 1 is about an OPENING, and the difference is the whole claim to determinism. A rule
    firing on `added` anywhere would flag half of every honest constraints section."""
    root = clean_root(t)
    index(root, extra("Files added under src/generated/ are ignored and never committed.\n"))
    r = measure(root, home_with_global(t))
    t.eq(r["data"]["finished_work_openings"], [], r["summary"])
    t.eq(r["verdict"], "pass", r["summary"])


@cell(red="mutant: blocks() split on blank lines only -- the third bullet of a pasted "
          "changelog list was invisible and it passed")
def cell_every_bullet_is_its_own_block(t: T) -> None:
    """A changelog pasted in as a list is one blank-line-delimited run, so a splitter that reads
    only its first line grades one entry out of five."""
    root = clean_root(t)
    index(root, extra("- The build takes port 5391.\n"
                      "- Secrets live in an untracked env file, never in the tree.\n"
                      "- Shipped the retry loop behind a flag.\n"))
    r = measure(root, home_with_global(t))
    found = r["data"]["finished_work_openings"]
    t.eq(len(found), 1, str(found))
    t.eq(found[0]["word"], "shipped")


# --- fail shape 2: a follow-up body inlined under a stub -----------------------------------------

@cell(red="mutant: the `seen_blank` branch dropped -- a paragraph under a stub passed")
def cell_fail_a_paragraph_inlined_under_a_stub(t: T) -> None:
    root = clean_root(t)
    index(root, CLEAN + "\n  The body of the follow-up, which belongs in a pending file.\n")
    r = measure(root, home_with_global(t))
    t.eq(r["verdict"], "fail", r["summary"])
    bodies = r["data"]["inlined_followup_bodies"]
    t.eq(len(bodies), 1, str(bodies))
    t.eq(bodies[0]["why"], "a paragraph under a stub")


@cell(red="mutant: the nested-bullet branch dropped -- a sub-bullet list under a stub passed")
def cell_fail_a_nested_bullet_under_a_stub(t: T) -> None:
    root = clean_root(t)
    index(root, CLEAN + "  - the first thing to check\n  - the second\n")
    r = measure(root, home_with_global(t))
    t.eq(r["verdict"], "fail", r["summary"])
    bodies = r["data"]["inlined_followup_bodies"]
    t.eq(len(bodies), 2, str(bodies))
    t.eq(bodies[0]["why"], "a nested bullet under a stub")


@cell(red="mutant: any indented line counted as an inlined body -- every wrapped stub failed")
def cell_a_wrapped_stub_is_not_an_inlined_body(t: T) -> None:
    """The shape has to survive a 100-column file. A one-line entry wraps onto two or three
    indented lines, and calling that a body makes ISF-10 unsatisfiable rather than enforced."""
    r = measure(clean_root(t), home_with_global(t))
    t.eq(r["data"]["inlined_followup_bodies"], [], r["summary"])
    t.eq(r["data"]["followup_entries"], 1)


@cell(red="mutant: followup_section() read to end of file -- the section after it was graded "
          "as follow-up bodies")
def cell_the_section_ends_at_the_next_heading(t: T) -> None:
    root = clean_root(t)
    index(root, extra("Ordinary prose that is not a follow-up body.\n"))
    r = measure(root, home_with_global(t))
    t.eq(r["data"]["inlined_followup_bodies"], [], r["summary"])
    t.eq(r["verdict"], "pass", r["summary"])


# --- fail shape 3: a stub over FOLLOWUP_STUB_BYTES -----------------------------------------------

@cell(red="mutant: `if size > limits.FOLLOWUP_STUB_BYTES` -> `if False`; a 700 B stub passed")
def cell_fail_a_stub_over_the_byte_cap(t: T) -> None:
    root = clean_root(t)
    filler = ("- **When the migration lands** — "
              + "and then a great deal more text explaining exactly why. " * 12).rstrip() + "\n"
    t.ok(len(filler.encode("utf-8")) > limits.FOLLOWUP_STUB_BYTES,
         "the fixture must actually exceed the cap it is testing")
    index(root, CLEAN + filler)
    r = measure(root, home_with_global(t))
    t.eq(r["verdict"], "fail", r["summary"])
    stubs = r["data"]["oversized_stubs"]
    t.eq(len(stubs), 1, str(stubs))
    t.eq(stubs[0]["limit"], limits.FOLLOWUP_STUB_BYTES)


# --- fail shape 4: a paragraph restating the rendered global file --------------------------------

@cell(red="mutant: `if overlap >= SHINGLE_OVERLAP` -> `if False`; a verbatim paste of a global "
          "paragraph passed")
def cell_fail_a_paragraph_restating_the_global_file(t: T) -> None:
    root = clean_root(t)
    pasted = "\n".join(GLOBAL_TEXT.splitlines()[2:])
    index(root, extra(pasted + "\n"))
    r = measure(root, home_with_global(t))
    t.eq(r["verdict"], "fail", r["summary"])
    restated = r["data"]["restated_global_paragraphs"]
    t.eq(len(restated), 1, str(restated))
    t.ok(restated[0]["overlap"] >= RUN.SHINGLE_OVERLAP, str(restated))


@cell(red="mutant: SHINGLE_WORDS lowered to 3 -- prose merely sharing the global file's "
          "vocabulary failed")
def cell_shared_vocabulary_is_not_a_restatement(t: T) -> None:
    """Ten words, because a run of ten consecutive words is a paste and three is a coincidence.
    A project that says `commit or push` in its own sentence has not restated anything."""
    root = clean_root(t)
    index(root, extra(
        "Consent to commit and push is any one of the release manager's word, a signed tag, or\n"
        "the propagate switch set to auto; none of them releases the vendor's nightly build.\n"))
    r = measure(root, home_with_global(t))
    # Calibrated, not asserted by eye: this paragraph overlaps the global file by 48% at
    # three-word shingles and 4% at ten. It is the fixture that tells the two apart.
    t.eq(r["data"]["restated_global_paragraphs"], [], r["summary"])
    t.eq(r["verdict"], "pass", r["summary"])


# --- warn: size over the alarm line --------------------------------------------------------------

@cell(red="mutant: `if size > alarm` -> `if size > alarm * 100`; a 30 KB index passed")
def cell_warn_size_over_the_alarm_line(t: T) -> None:
    root = clean_root(t)
    index(root, extra(padding(500)))
    r = measure(root, home_with_global(t))
    t.eq(r["verdict"], "warn", r["summary"])
    t.ok(r["data"]["bytes"] > limits.PROJECT_INDEX_ALARM_BYTES, str(r["data"]["bytes"]))
    t.ok("alarm" in r["summary"], r["summary"])


@cell(red="mutant: alarm_bytes() ignored .claude/config.json -- the project's own line "
          "went unread and the index still warned")
def cell_a_project_sets_its_own_alarm_line(t: T) -> None:
    """ISF-8: a review that admits every block raises the line in TRACKED config, so the
    judgment is in git history instead of in a cut nobody can see."""
    root = clean_root(t)
    index(root, extra(padding(500)))
    (root / ".claude").mkdir(exist_ok=True)
    (root / ".claude" / "config.json").write_text(
        '{"limits": {"project_index_alarm_bytes": 65536}}', encoding="utf-8")
    r = measure(root, home_with_global(t))
    t.eq(r["data"]["alarm_bytes"], 65_536)
    t.eq(r["data"]["alarm_source"], "project")
    t.eq(r["verdict"], "pass", r["summary"])


@cell(red="mutant: alarm_bytes() accepted any JSON value -- an override of `true` set the "
          "alarm line to 1 and every index warned")
def cell_a_bad_override_falls_back_to_the_framework_line(t: T) -> None:
    root = clean_root(t)
    (root / ".claude").mkdir(exist_ok=True)
    (root / ".claude" / "config.json").write_text(
        '{"limits": {"project_index_alarm_bytes": true}}', encoding="utf-8")
    r = measure(root, home_with_global(t))
    t.eq(r["data"]["alarm_bytes"], limits.PROJECT_INDEX_ALARM_BYTES)
    t.eq(r["data"]["alarm_source"], "framework")
    t.eq(r["verdict"], "pass", r["summary"])


# --- warn: growth in one change ------------------------------------------------------------------

@cell(red="mutant: growth() always returned (None, ...) -- a 2 KB uncommitted addition "
          "reported no delta and passed")
def cell_warn_growth_against_head_when_the_tree_is_dirty(t: T) -> None:
    """At a finalize the index is dirty and the growth about to be committed is the question."""
    root = clean_root(t, repo(t))
    sh(t, root, "add", "-A")
    sh(t, root, "commit", "-q", "-m", "index")
    index(root, extra(padding(60)))
    r = measure(root, home_with_global(t))
    t.eq(r["data"]["delta_basis"], "working tree vs HEAD")
    t.ok(r["data"]["delta_bytes"] > limits.PROJECT_INDEX_GROWTH_BYTES, str(r["data"]))
    t.eq(r["verdict"], "warn", r["summary"])


@cell(red="observed: on a CRLF checkout a clean tree read 'working tree vs HEAD' and "
          "reported a phantom +75 B")
def cell_growth_falls_back_to_the_last_commit_when_clean(t: T) -> None:
    """Also the CRLF regression. `git show` hands back LF whatever is on disk, so an unnormalised
    comparison makes every clean Windows checkout look dirty by one byte per line -- and the
    delta the briefing shows is then noise nobody can act on."""
    root = clean_root(t, repo(t))
    sh(t, root, "add", "-A")
    sh(t, root, "commit", "-q", "-m", "one")
    index(root, extra(padding(60)))
    sh(t, root, "add", "CLAUDE.md")
    sh(t, root, "commit", "-q", "-m", "two")
    r = measure(root, home_with_global(t))
    t.eq(r["data"]["delta_basis"], "HEAD vs HEAD~1")
    t.ok(r["data"]["delta_bytes"] > limits.PROJECT_INDEX_GROWTH_BYTES, str(r["data"]))
    t.eq(r["verdict"], "warn", r["summary"])


@cell(red="mutant: _blob_bytes() returned 0 for an absent blob -- a first commit read as "
          "growth of the whole file")
def cell_an_absent_prior_size_is_not_zero(t: T) -> None:
    root = clean_root(t, repo(t))
    r = measure(root, home_with_global(t))
    t.eq(r["data"]["delta_bytes"], None)
    t.eq(r["data"]["delta_basis"], "no committed CLAUDE.md")
    t.eq(r["verdict"], "pass", r["summary"])


# --- warn: the restatement shape could not be measured -------------------------------------------

@cell(red="mutant: an absent <home>/CLAUDE.md skipped shape 4 silently and passed -- a "
          "check that did not run read as coverage")
def cell_warn_an_absent_global_file_is_named(t: T) -> None:
    r = measure(clean_root(t), t.tmpdir())
    t.eq(r["verdict"], "warn", r["summary"])
    t.eq(r["data"]["global_compared"], False)
    t.ok("restatement" in r["summary"], r["summary"])


# --- warn: a backticked path that resolves to nothing --------------------------------------------

@cell(red="mutant: `if not (project_root / token).exists()` -> `if False`; a pointer to a "
          "deleted file passed")
def cell_warn_a_backticked_path_that_resolves_to_nothing(t: T) -> None:
    root = clean_root(t)
    index(root, extra("The layout is described in `docs/architecture/overview.md`.\n"))
    r = measure(root, home_with_global(t))
    t.eq(r["verdict"], "warn", r["summary"])
    t.eq([u["path"] for u in r["data"]["unresolvable_paths"]],
         ["docs/architecture/overview.md"])


@cell(red="mutant: _is_gradable_path() accepted every backticked token -- a command, a "
          "glob, `~/.claude` and a URL all warned")
def cell_only_an_unambiguous_relative_path_is_graded(t: T) -> None:
    """Every token here names something real relative to a context the sentence supplies, or is
    not a path at all. A warn an author cannot act on is worse than no warn."""
    root = clean_root(t)
    index(root, extra(
        "Run `python -m pytest`. `sync.py` deploys; `deferred/` holds briefs;\n"
        "the home is `~/.claude` and the template is `<home>/segments/`; docs are at\n"
        "`https://example.invalid/x`; globs like `docs/**/*.md` are not resolved.\n"))
    r = measure(root, home_with_global(t))
    t.eq(r["data"]["unresolvable_paths"], [], r["summary"])
    t.eq(r["verdict"], "pass", r["summary"])


@cell(red="mutant: every backticked path was reported -- a correct pointer warned")
def cell_a_path_that_exists_is_not_reported(t: T) -> None:
    root = clean_root(t)
    (root / "docs" / "overview.md").write_text("x", encoding="utf-8")
    index(root, extra("The layout is described in `docs/overview.md`.\n"))
    r = measure(root, home_with_global(t))
    t.eq(r["data"]["unresolvable_paths"], [], r["summary"])
    t.eq(r["verdict"], "pass", r["summary"])


# --- the summary contract ------------------------------------------------------------------------

@cell(red="mutant: _fit() removed -- a fail carrying all four shapes raised out of "
          "oracle.result and became one `unknown`")
def cell_a_crowded_summary_stays_within_c2s_200_characters(t: T) -> None:
    """`oracle.result` RAISES over 200 characters, and the runner turns a raise into `unknown`.
    The one index most in need of a report is the one that would lose it."""
    root = clean_root(t)
    pasted = "\n".join(GLOBAL_TEXT.splitlines()[2:])
    filler = ("- **When the migration lands** — "
              + "and then a great deal more text explaining exactly why. " * 12).rstrip() + "\n"
    index(root, CLEAN + filler + "  - a nested bullet\n\n## Notes\n\n"
          "- Migrated the ledger to the v2 schema.\n\n" + pasted + "\n")
    r = measure(root, home_with_global(t))
    t.eq(r["verdict"], "fail", r["summary"])
    t.ok(len(r["summary"]) <= 200, f"{len(r['summary'])} chars: {r['summary']}")
    for key in ("finished_work_openings", "inlined_followup_bodies", "oversized_stubs",
                "restated_global_paragraphs"):
        t.ok(r["data"][key], f"all four shapes must be reported in data: {key} is empty")

    # The warn path is where the cap is actually crossed: a warn embeds the offending path, and
    # a deep one plus the alarm and the growth line runs past 200 characters on its own.
    deep = "docs/architecture/decisions/2026/09/the-ledger-migration-and-its-fallout/notes.md"
    root2 = clean_root(t, repo(t))
    sh(t, root2, "add", "-A")
    sh(t, root2, "commit", "-q", "-m", "index")
    index(root2, extra(padding(500) + f"See `{deep}`.\n"))
    r2 = measure(root2, home_with_global(t))
    t.eq(r2["verdict"], "warn", r2["summary"])
    t.ok(len(r2["summary"]) <= 200, f"{len(r2['summary'])} chars: {r2['summary']}")
    t.ok(r2["summary"].endswith("…"),
         f"the fixture must actually produce a summary needing the cut: {r2['summary']}")
    t.eq([u["path"] for u in r2["data"]["unresolvable_paths"]], [deep],
         "and the finding the summary had to truncate is still whole in data")


@cell(red="mutant: the trend dropped from the summary -- the briefing showed a verdict with "
          "no bytes, no alarm line and no delta")
def cell_the_summary_always_carries_the_trend(t: T) -> None:
    """ISF-7: the trend rides every session-start briefing, which is the periodic gate this
    replaced made continuous. A summary carrying it only on a warn is not a trend."""
    r = measure(clean_root(t), home_with_global(t))
    t.ok(str(r["data"]["bytes"]) in r["summary"], r["summary"])
    t.ok(str(limits.PROJECT_INDEX_ALARM_BYTES) in r["summary"], r["summary"])
    t.ok("alarm" in r["summary"], r["summary"])


@cell(red="pre-implementation: oracle.json did not exist; kind/install_when/gaps ungraded")
def cell_the_manifest_conforms_to_c2(t: T) -> None:
    import json

    manifest = json.loads((HERE / "oracle.json").read_text(encoding="utf-8"))
    t.eq(manifest["schema"], "oracle/2")
    t.eq(manifest["name"], HERE.name)
    t.eq(manifest["kind"], "verification")
    t.eq(manifest["install_when"], {"any_glob": ["CLAUDE.md"]})
    assertion = manifest["assertion"]
    for key in ("asserts", "measures", "known_gaps"):
        t.ok(assertion.get(key), f"a verification oracle owes {key} (C2)")
    gaps = " ".join(assertion["known_gaps"]).lower()
    t.ok("proxy for narration" in gaps, "the token proxy must be named in known_gaps (ISF-6)")
    t.ok("paraphrase passes" in gaps, "the paraphrase gap must be named in known_gaps (ISF-6)")
