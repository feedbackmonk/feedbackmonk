#!/usr/bin/env python3
"""Self-test for the `markdown-link-validity` oracle -- a C6 cell module."""

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
    spec = importlib.util.spec_from_file_location("markdown_link_validity_run", HERE / "run.py")
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


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def commit_all(t: T, root: Path) -> None:
    sh(t, root, "add", "-A")
    sh(t, root, "commit", "-q", "-m", "commit")


@cell(red="pre-implementation: run() did not exist")
def cell_no_tracked_markdown_is_pass(t: T) -> None:
    root = repo(t)
    write(root / "f.txt", "x")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "pass")
    t.eq(r["data"]["scanned_files"], 0)


@cell(red="pre-implementation: a resolving link was never checked at all")
def cell_resolving_link_passes(t: T) -> None:
    root = repo(t)
    write(root / "target.md", "# target\n")
    write(root / "index.md", "see [target](target.md)\n")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "pass")
    t.eq(r["data"]["checked"], 1)
    t.eq(r["data"]["broken_count"], 0)


@cell(red="pre-implementation: a broken link was never detected")
def cell_broken_link_fails_and_names_it(t: T) -> None:
    root = repo(t)
    write(root / "index.md", "see [gone](does-not-exist.md)\n")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "fail")
    t.eq(r["data"]["broken_count"], 1)
    row = r["data"]["broken"][0]
    t.eq(row["source"], "index.md")
    t.eq(row["line"], 1)
    t.eq(row["link"], "does-not-exist.md")


@cell(red="pre-implementation: resolution was never relative to the CITING file's directory")
def cell_resolution_is_citing_file_relative(t: T) -> None:
    root = repo(t)
    write(root / "docs" / "target.md", "# t\n")
    write(root / "docs" / "sub" / "index.md", "see [t](../target.md)\n")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "pass", r["data"]["broken"])


@cell(red="pre-implementation: fenced code and inline spans were not excluded from scanning")
def cell_links_in_code_are_ignored(t: T) -> None:
    root = repo(t)
    write(root / "index.md",
          "before\n\n```\n[gone](nowhere.md)\n```\n\nafter `[also-gone](nowhere2.md)` end\n"
          "\n[real-gone](nowhere3.md)\n")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "fail")
    t.eq(r["data"]["checked"], 1, "only the link outside code should be counted")
    t.eq(r["data"]["broken"][0]["link"], "nowhere3.md")


@cell(red="pre-implementation: anchors and external schemes were not excluded")
def cell_bare_anchors_and_external_links_are_skipped(t: T) -> None:
    root = repo(t)
    write(root / "index.md",
          "[section](#section) [site](https://example.com/x) [mail](mailto:a@b.com)\n")
    commit_all(t, root)
    r = measure(root)
    t.eq(r["verdict"], "pass")
    t.eq(r["data"]["checked"], 0)


@cell(red="pre-implementation: the OVALID-05 uncommitted-deletion bucket did not exist")
def cell_uncommitted_deletion_is_informational_not_broken(t: T) -> None:
    root = repo(t)
    write(root / "target.md", "# t\n")
    write(root / "index.md", "see [t](target.md)\n")
    commit_all(t, root)

    (root / "target.md").unlink()  # deleted in the working tree, NOT committed
    r = measure(root)
    t.eq(r["verdict"], "pass", "an uncommitted deletion must not turn a correct citation red")
    t.eq(r["data"]["broken_count"], 0)
    t.eq(r["data"]["uncommitted_deletion_count"], 1)
    t.eq(r["data"]["uncommitted_deletions"][0]["link"], "target.md")

    sh(t, root, "add", "-A")
    sh(t, root, "commit", "-q", "-m", "commit the deletion")
    r2 = measure(root)
    t.eq(r2["verdict"], "fail", "once committed, the same citation is genuinely broken")
    t.eq(r2["data"]["broken_count"], 1)


if __name__ == "__main__":
    sys.exit(cells.main())
