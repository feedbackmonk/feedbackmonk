#!/usr/bin/env python3
"""Self-test for the `pending-followups` oracle -- a C6 cell module."""

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
    spec = importlib.util.spec_from_file_location("pending_followups_run", HERE / "run.py")
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


RUN = _load_run()


def measure(root: Path) -> dict:
    ctx = oracle.Context(project_root=root, anchor_root=root, home=paths.home())
    return RUN.run(ctx)


def write(root: Path, text: str) -> None:
    (root / "CLAUDE.md").write_text(text, encoding="utf-8")


@cell(red="pre-implementation: run() did not exist")
def cell_no_claude_md_is_pass_zero(t: T) -> None:
    root = t.tmpdir()
    r = measure(root)
    t.eq(r["verdict"], "pass")
    t.eq(r["data"], {"count": 0, "items": []})


@cell(red="pre-implementation: section detection did not exist")
def cell_no_section_is_zero(t: T) -> None:
    root = t.tmpdir()
    write(root, "# CLAUDE.md\n\n## Some Other Section\n\n- not a followup\n")
    t.eq(measure(root)["data"]["count"], 0)


@cell(red="pre-implementation: bullet extraction did not exist")
def cell_three_bullets_are_three_items(t: T) -> None:
    root = t.tmpdir()
    write(root, "# CLAUDE.md\n\n## Pending Follow-Ups\n\n"
                "- **Trigger: A** -- do X.\n"
                "- **Trigger: B** -- do Y.\n"
                "- **Trigger: C** -- do Z.\n\n## Next Section\n\nprose\n")
    r = measure(root)
    t.eq(r["data"]["count"], 3)
    t.eq(r["data"]["items"][0], "**Trigger: A** -- do X.")
    t.ok("3 pending follow-up" in r["summary"], r["summary"])


@cell(red="pre-implementation: the section-end boundary at the next heading did not exist")
def cell_stops_at_the_next_heading(t: T) -> None:
    root = t.tmpdir()
    write(root, "## Pending Follow-Ups\n\n- one\n\n## Later\n\n- two\n")
    t.eq(measure(root)["data"]["items"], ["one"])


@cell(red="pre-implementation: indented continuation prose was counted as a second stub")
def cell_indented_continuation_is_not_a_second_item(t: T) -> None:
    root = t.tmpdir()
    write(root, "## Pending Follow-Ups\n\n"
                "- **Trigger: A** -- do X.\n\n"
                "  Remove this entry once X happens.\n")
    d = measure(root)["data"]
    t.eq(d["count"], 1, "the indented paragraph must not be read as a bullet")


@cell(red="pre-implementation: an indented sub-bullet was counted as top-level")
def cell_indented_sub_bullet_does_not_count(t: T) -> None:
    root = t.tmpdir()
    write(root, "## Pending Follow-Ups\n\n- top\n  - nested, not a stub\n")
    t.eq(measure(root)["data"]["count"], 1)


if __name__ == "__main__":
    sys.exit(cells.main())
