#!/usr/bin/env python3
"""Self-test for the `git-state` oracle -- a C6 cell module."""

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
    spec = importlib.util.spec_from_file_location("git_state_run", HERE / "run.py")
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


@cell(red="M-G1: a fresh zero-commit repo reported is_git_repo:false (measured live, sandbox proof)")
def cell_freshly_initialized_repo_with_no_commits_is_still_a_repo(t: T) -> None:
    root = t.tmpdir() / "fresh"
    root.mkdir()
    sh(t, root, "init", "-q")
    d = measure(root)["data"]
    t.eq(d["is_git_repo"], True, "a real repo with zero commits must not read as not-a-repo")
    t.eq(d["branch"] in ("main", "master"), True, d["branch"])
    t.eq(d["head"], None, "no commits yet -- there is no HEAD sha")


@cell(red="pre-implementation: run() did not exist")
def cell_non_repo_is_pass_not_a_repo(t: T) -> None:
    root = t.tmpdir() / "not-a-repo"
    root.mkdir()
    r = measure(root)
    t.eq(r["verdict"], "pass")
    t.eq(r["data"]["is_git_repo"], False)
    t.eq(r["data"]["branch"], None)
    t.eq(r["data"]["head"], None)


@cell(red="pre-implementation: no commit/branch/head parsing existed")
def cell_clean_repo_reports_branch_and_head(t: T) -> None:
    root = repo(t)
    (root / "f.txt").write_text("x", encoding="utf-8")
    sh(t, root, "add", "f.txt")
    sh(t, root, "commit", "-q", "-m", "init")
    r = measure(root)
    t.eq(r["verdict"], "pass")
    d = r["data"]
    t.eq(d["is_git_repo"], True)
    t.ok(d["branch"] in ("main", "master"), d["branch"])
    t.eq(len(d["head"] or ""), 40, f"head must be a full sha: {d['head']!r}")
    t.eq(d["dirty"], False)
    t.eq(d["has_upstream"], False)
    t.eq(d["ahead"], None)
    t.eq(d["behind"], None)


@cell(red="pre-implementation: dirty-count derivation from status rows did not exist")
def cell_dirty_counts_untracked_and_staged(t: T) -> None:
    root = repo(t)
    (root / "committed.txt").write_text("x", encoding="utf-8")
    sh(t, root, "add", "committed.txt")
    sh(t, root, "commit", "-q", "-m", "init")

    (root / "new.txt").write_text("untracked", encoding="utf-8")
    d = measure(root)["data"]
    t.eq(d["dirty"], True)
    t.eq(d["untracked"], 1)
    t.eq(d["staged"], 0)

    sh(t, root, "add", "new.txt")
    d = measure(root)["data"]
    t.eq(d["untracked"], 0)
    t.eq(d["staged"], 1, "a newly staged file counts as staged")


@cell(red="pre-implementation: modified/deleted counts did not distinguish X/Y columns")
def cell_modified_and_deleted_counts(t: T) -> None:
    root = repo(t)
    (root / "a.txt").write_text("1", encoding="utf-8")
    (root / "b.txt").write_text("1", encoding="utf-8")
    sh(t, root, "add", "-A")
    sh(t, root, "commit", "-q", "-m", "init")

    (root / "a.txt").write_text("2", encoding="utf-8")
    (root / "b.txt").unlink()
    d = measure(root)["data"]
    t.eq(d["modified"], 1, "a.txt has an unstaged content change")
    t.eq(d["deleted"], 1, "b.txt was removed from the working tree")


@cell(red="pre-implementation: detached HEAD was not distinguished from a branch name")
def cell_detached_head_reports_no_branch(t: T) -> None:
    root = repo(t)
    (root / "f.txt").write_text("x", encoding="utf-8")
    sh(t, root, "add", "f.txt")
    sh(t, root, "commit", "-q", "-m", "init")
    head = measure(root)["data"]["head"]
    sh(t, root, "checkout", "-q", head)
    d = measure(root)["data"]
    t.eq(d["branch"], None, "a detached checkout must not report a branch name")
    t.eq(d["head"], head)


@cell(red="pre-implementation: ahead/behind against an upstream was never measured")
def cell_ahead_behind_against_an_upstream(t: T) -> None:
    upstream = t.tmpdir() / "upstream"
    upstream.mkdir()
    sh(t, upstream, "init", "-q", "--bare")

    origin_seed = repo(t)
    (origin_seed / "f.txt").write_text("1", encoding="utf-8")
    sh(t, origin_seed, "add", "f.txt")
    sh(t, origin_seed, "commit", "-q", "-m", "init")
    sh(t, origin_seed, "remote", "add", "origin", str(upstream))
    sh(t, origin_seed, "push", "-q", "-u", "origin", "HEAD")

    clone = t.tmpdir() / "clone"
    result = t.run(["git", "clone", "-q", str(upstream), str(clone)], timeout_s=60)
    t.ok(result.rc == 0, f"clone failed: {result.err}")
    sh(t, clone, "config", "user.email", "a@example.com")
    sh(t, clone, "config", "user.name", "a")

    d = measure(clone)["data"]
    t.eq(d["has_upstream"], True)
    t.eq(d["ahead"], 0)
    t.eq(d["behind"], 0)

    (clone / "g.txt").write_text("2", encoding="utf-8")
    sh(t, clone, "add", "g.txt")
    sh(t, clone, "commit", "-q", "-m", "local-only")
    d = measure(clone)["data"]
    t.eq(d["ahead"], 1)
    t.eq(d["behind"], 0)

    (origin_seed / "h.txt").write_text("3", encoding="utf-8")
    sh(t, origin_seed, "add", "h.txt")
    sh(t, origin_seed, "commit", "-q", "-m", "remote-only")
    sh(t, origin_seed, "push", "-q", "origin", "HEAD")
    sh(t, clone, "fetch", "-q", "origin")
    d = measure(clone)["data"]
    t.eq(d["ahead"], 1)
    t.eq(d["behind"], 1)


if __name__ == "__main__":
    sys.exit(cells.main())
