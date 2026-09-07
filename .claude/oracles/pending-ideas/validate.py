#!/usr/bin/env python3
"""Self-test for the `pending-ideas` oracle -- a C6 cell module."""

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
    spec = importlib.util.spec_from_file_location("pending_ideas_run", HERE / "run.py")
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


RUN = _load_run()


def measure(root: Path) -> dict:
    ctx = oracle.Context(project_root=root, anchor_root=root, home=paths.home())
    return RUN.run(ctx)


def write(root: Path, rel: str, text: str = "x") -> None:
    p = root / "docs" / "planning" / "deferred" / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding="utf-8")


@cell(red="pre-implementation: run() did not exist")
def cell_no_deferred_dir_is_pass_zero(t: T) -> None:
    root = t.tmpdir()
    r = measure(root)
    t.eq(r["verdict"], "pass")
    t.eq(r["data"], {"count": 0, "files": []})


@cell(red="pre-implementation: filename listing did not exist")
def cell_lists_md_filenames_sorted(t: T) -> None:
    root = t.tmpdir()
    write(root, "DEFER-002_second.md")
    write(root, "DEFER-001_first.md")
    write(root, "slug-named-brief.md")
    r = measure(root)
    t.eq(r["data"]["count"], 3)
    t.eq(r["data"]["files"],
         ["DEFER-001_first.md", "DEFER-002_second.md", "slug-named-brief.md"])


@cell(red="pre-implementation: a DEFER-*.md-only glob is the DEFER-239 defect RB-12 rules out")
def cell_non_defer_named_briefs_are_listed_too(t: T) -> None:
    root = t.tmpdir()
    write(root, "topic-2026-09-06.md")
    t.eq(measure(root)["data"]["files"], ["topic-2026-09-06.md"])


@cell(red="pre-implementation: README.md exclusion did not exist")
def cell_readme_excluded_case_insensitively(t: T) -> None:
    root = t.tmpdir()
    write(root, "README.md")
    write(root, "readme.md")
    write(root, "DEFER-001_x.md")
    r = measure(root)
    t.eq(r["data"]["files"], ["DEFER-001_x.md"])


@cell(red="pre-implementation: front matter / status parsing did not exist to NOT exist")
def cell_front_matter_and_status_are_never_read(t: T) -> None:
    """RB-12: filename only. A DISMISSED/COMPLETED brief is still LISTED -- this oracle makes
    no triage judgment at all, unlike its predecessor."""
    root = t.tmpdir()
    write(root, "DEFER-009_done.md", "---\nstatus: COMPLETED\n---\nbody\n")
    r = measure(root)
    t.eq(r["data"]["files"], ["DEFER-009_done.md"])
    t.ok("status" not in r["data"], "no status field exists in this contract's output at all")


if __name__ == "__main__":
    sys.exit(cells.main())
