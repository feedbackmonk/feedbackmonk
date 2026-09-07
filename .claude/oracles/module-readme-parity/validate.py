#!/usr/bin/env python3
"""Self-test for the `module-readme-parity` oracle -- a C6 cell module."""

from __future__ import annotations

import importlib.util
import os
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))
if not (pathlib.Path(sys.path[0]) / "uldf").is_dir():
    sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[3] / "scripts" / "lib"))

from uldf import cells, oracle, paths  # noqa: E402
from uldf.cells import T, cell  # noqa: E402

from pathlib import Path  # noqa: E402

HERE = Path(__file__).resolve().parent


def _load_run():
    spec = importlib.util.spec_from_file_location("module_readme_parity_run", HERE / "run.py")
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


RUN = _load_run()


def measure(root: Path) -> dict:
    ctx = oracle.Context(project_root=root, anchor_root=root, home=paths.home())
    return RUN.run(ctx)


def sh(t: T, root: Path, *argv: str) -> None:
    result = t.run(["git", *argv], cwd=str(root), timeout_s=30)
    t.ok(result.rc == 0, f"git {' '.join(argv)} failed: {result.err}")


#: ONE seed repository per process, copied per cell. `git init` plus two `git config` is three
#: process spawns, and on Windows that is ~0.5-2.7 s of `SUITE_MS` paid by every cell below
#: against ~34 ms for a directory copy (measured 2026-09-07). The seed is built once and never
#: written to again, so nothing MUTABLE is shared between cells and C6 § 6.1 is untouched -- each
#: cell still gets its own repository, byte-identical to the one it used to build for itself.
#:
#: Written out here rather than imported: a starter oracle is installed into a consumer project on
#: its own, where `tests/_seedrepo.py` does not exist. The duplication is the price of the oracle
#: being self-contained, which is a C2 property, not an accident.
_SEED: list = []


def repo(t: T) -> Path:
    """A throwaway repo with the cell identity configured. Never this project."""
    if not _SEED:
        import atexit
        import shutil
        import tempfile

        seed = Path(tempfile.mkdtemp(prefix="uldf-oracle-seed-")) / "repo"
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
    # The three `sh` calls this replaced asserted `rc == 0`, i.e. that the three commands did not
    # fail. These assert the POSTCONDITION they existed to produce, which an exit code does not
    # prove: the repository is here and carries the identity a commit needs.
    config = (root / ".git" / "config").read_text(encoding="utf-8", errors="replace")
    t.ok((root / ".git").is_dir(), "the seeded fixture is a git repository")
    t.ok("a@example.com" in config, f"and carries the cell identity email:\n{config}")
    t.ok("name = a" in config, f"and the cell identity name:\n{config}")
    return root


def write(path: Path, text: str = "x") -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def commit_all(t: T, root: Path) -> None:
    sh(t, root, "add", "-A")
    sh(t, root, "commit", "-q", "-m", "commit")


@cell(red="pre-implementation: run() did not exist")
def cell_empty_repo_is_pass(t: T) -> None:
    root = repo(t)
    write(root / "f.txt")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "pass")


@cell(red="pre-implementation: File Index section parsing did not exist")
def cell_matching_index_is_pass(t: T) -> None:
    root = repo(t)
    write(root / "mod" / "run.py", "x")
    write(root / "mod" / "helper.py", "x")
    write(root / "mod" / "README.md",
          "# mod\n\n## 2. File Index\n\n| File | Holds |\n|---|---|\n"
          "| `run.py` | entry point |\n| `helper.py` | helpers |\n\n## 3. Public API\n\nprose\n")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "pass")
    t.eq(r["data"]["parity_mismatches"], [])


@cell(red="pre-implementation: a file missing from the index was never detected")
def cell_file_missing_from_index_fails(t: T) -> None:
    root = repo(t)
    write(root / "mod" / "run.py", "x")
    write(root / "mod" / "untracked_in_index.py", "x")
    write(root / "mod" / "README.md",
          "# mod\n\n## File Index\n\n| `run.py` | entry point |\n\n## Next\n\nprose\n")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "fail")
    m = r["data"]["parity_mismatches"][0]
    t.eq(m["missing_from_index"], ["untracked_in_index.py"])
    t.eq(m["missing_from_directory"], [])


@cell(red="pre-implementation: an index entry with no matching file was never detected")
def cell_index_entry_with_no_file_fails(t: T) -> None:
    root = repo(t)
    write(root / "mod" / "run.py", "x")
    write(root / "mod" / "README.md",
          "# mod\n\n## File Index\n\n| `run.py` | x |\n| `ghost.py` | doesn't exist |\n")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "fail")
    m = r["data"]["parity_mismatches"][0]
    t.eq(m["missing_from_directory"], ["ghost.py"])


@cell(red="pre-implementation: the '## 2. File Index' numbered form was not recognized")
def cell_numbered_heading_form_is_recognized(t: T) -> None:
    root = repo(t)
    write(root / "mod" / "x.py", "x")
    write(root / "mod" / "README.md", "# mod\n\n## 2. File Index\n\n`x.py`\n")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "pass")


@cell(red="pre-implementation: >=3-source-file directories with no README were never flagged")
def cell_presence_gap_for_three_plus_source_files_no_readme(t: T) -> None:
    root = repo(t)
    write(root / "nomod" / "a.py", "x")
    write(root / "nomod" / "b.py", "x")
    write(root / "nomod" / "c.py", "x")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "fail")
    gap = r["data"]["presence_gaps"][0]
    t.eq(gap["directory"], "nomod")
    t.eq(gap["source_file_count"], 3)


@cell(red="pre-implementation: fewer than 3 source files with no README was never exempted")
def cell_two_source_files_no_readme_is_not_flagged(t: T) -> None:
    root = repo(t)
    write(root / "smallmod" / "a.py", "x")
    write(root / "smallmod" / "b.py", "x")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "pass")
    t.eq(r["data"]["presence_gaps"], [])


@cell(red="every one of these tokens was reported as a file that is gone: 12 false hits")
def cell_prose_backticks_are_not_filename_claims(t: T) -> None:
    """A File Index describes its module, so it backticks identifiers and literals too.

    Measured before the fix, over five instrumented projects: `hooks/README.md` alone produced 24
    of these, and the storm held proof (3) red on nothing. Each token below is one this check
    really reported, copied from `S:/Apps/Table`'s run of 2026-09-07.
    """
    root = repo(t)
    write(root / "mod" / "run.py", "x")
    write(root / "mod" / "sub" / "nested.py", "x")
    noise = ('`""`', "`0`", "`=`", "`<table>`", "`#[ignore]`", "`.await`", "`$derived`",
             "`-Json`", "`CELL_IDS`", "`page.goto`", "`window.__aor_cmd_core`", "`sub`")
    write(root / "mod" / "README.md",
          "# mod\n\n## File Index\n\n| `run.py` | entry point, which returns "
          + ", ".join(noise) + " |\n")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "pass",
         f"prose backticks were graded as filenames: {r['data']['parity_mismatches']}")
    t.eq(r["data"]["parity_mismatches"], [])


@cell(red="`^[A-Za-z0-9]` first char: a correctly indexed __init__.py and .gitignore both "
          "reported missing_from_index")
def cell_a_leading_underscore_or_dot_still_names_a_file(t: T) -> None:
    """The names real files open with, which a narrowed filename shape is quickest to lose.

    Caught by the judge on the first attempt at this fix, and it was live rather than theoretical:
    the oracle reported `scripts/lib/uldf: missing_from_index: ['__init__.py']` against a README
    that indexes it on line 23, and `['_force-clean-worktrees.ps1', '_rewrite-paths.ps1']` in a
    consumer project -- a false hit no README edit can clear, in the direction the first fix was
    not looking. Both new cells there asserted only that prose is dropped, and every filename in
    their fixtures began with a lowercase letter, so the suite was green over it.
    """
    root = repo(t)
    write(root / "mod" / "__init__.py", "x")
    write(root / "mod" / "_helper.ps1", "x")
    write(root / "mod" / ".gitignore", "x")
    write(root / "mod" / "run.py", "x")
    write(root / "mod" / "README.md",
          "# mod\n\n## File Index\n\n| `__init__.py` | the package |\n"
          "| `_helper.ps1` | a helper |\n| `.gitignore` | ignores |\n| `run.py` | entry |\n")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "pass",
         f"a correctly indexed dot- or underscore-file was graded wrong: "
         f"{r['data']['parity_mismatches']}")
    t.eq(r["data"]["parity_mismatches"], [])


@cell(red="the shape test alone passed `page.goto`; only a used extension separates it")
def cell_a_stale_entry_is_still_caught_by_its_extension(t: T) -> None:
    """The fix must not buy quiet by grading nothing: an entry naming a real-looking file that
    is not there stays a mismatch, and it is the extension its siblings use that says so."""
    root = repo(t)
    write(root / "mod" / "run.py", "x")
    write(root / "mod" / "README.md",
          "# mod\n\n## File Index\n\n| `run.py` | x |\n| `ghost.py` | deleted last week |\n"
          "| `page.goto` | a call site, not a file |\n")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "fail")
    m = r["data"]["parity_mismatches"][0]
    t.eq(m["missing_from_directory"], ["ghost.py"])


@cell(red="pre-implementation: an untracked file was never excluded from either check")
def cell_untracked_files_are_invisible_to_both_checks(t: T) -> None:
    root = repo(t)
    write(root / "mod" / "run.py", "x")
    write(root / "mod" / "README.md", "# mod\n\n## File Index\n\n`run.py`\n")
    commit_all(t, root)
    write(root / "mod" / "untracked.py")  # never git-added
    r = measure(root)
    t.eq(r["verdict"], "pass", "an untracked file must not create a parity mismatch")


if __name__ == "__main__":
    sys.exit(cells.main())
