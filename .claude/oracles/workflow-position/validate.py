#!/usr/bin/env python3
"""Self-test for the `workflow-position` oracle -- a C6 cell module."""

from __future__ import annotations

import importlib.util
import json
import os
import pathlib
import sys
import time

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))
if not (pathlib.Path(sys.path[0]) / "uldf").is_dir():
    sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[3] / "scripts" / "lib"))

from uldf import cells, oracle, paths  # noqa: E402
from uldf.cells import T, cell  # noqa: E402

from pathlib import Path  # noqa: E402

HERE = Path(__file__).resolve().parent


def _load_run():
    spec = importlib.util.spec_from_file_location("workflow_position_run", HERE / "run.py")
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


RUN = _load_run()


def measure(root: Path) -> dict:
    ctx = oracle.Context(project_root=root, anchor_root=root, home=paths.home())
    return RUN.run(ctx)


def write(path: Path, text: str = "x") -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


@cell(red="pre-implementation: run() did not exist")
def cell_no_artifacts_is_pass_position_none(t: T) -> None:
    root = t.tmpdir() / "empty"
    root.mkdir()
    r = measure(root)
    t.eq(r["verdict"], "pass")
    d = r["data"]
    t.eq(d["position"], "none")
    t.eq(d["has_spec"], False)
    t.eq(d["latest_plan"], None)
    t.eq(d["arc_state"], "none")


@cell(red="pre-implementation: docs/specs/*.md presence check did not exist")
def cell_spec_files_report_post_spec(t: T) -> None:
    root = t.tmpdir() / "spec"
    write(root / "docs" / "specs" / "SPECIFICATION.md")
    d = measure(root)["data"]
    t.eq(d["has_spec"], True)
    t.eq(d["spec_file_count"], 1)
    t.eq(d["position"], "post-spec")


@cell(red="pre-implementation: newest-plan selection did not exist")
def cell_latest_plan_is_the_newest_file_and_wins_over_spec(t: T) -> None:
    root = t.tmpdir() / "plan"
    write(root / "docs" / "specs" / "SPECIFICATION.md")
    write(root / "docs" / "planning" / "plans" / "20260101T000000-old.md")
    time.sleep(0.02)
    write(root / "docs" / "planning" / "plans" / "20260202T000000-new.md")
    d = measure(root)["data"]
    t.eq(d["latest_plan"], "docs/planning/plans/20260202T000000-new.md")
    t.eq(d["position"], "post-plan", "a plan outranks a bare spec")


@cell(red="pre-implementation: arc-state.json (C5) was never read")
def cell_active_arc_reports_in_execution(t: T) -> None:
    root = t.tmpdir() / "arc"
    write(root / "docs" / "planning" / "plans" / "p.md")
    write(root / "ltads" / "arc-state.json", json.dumps({
        "schema": "arc-state/2", "last_arc_id": "A001",
        "arc": {"id": "A001", "status": "active", "driver_session": "s1",
                "started_at": "2026-01-01T00:00:00Z", "concluded_at": None,
                "consent": None, "progress": "building", "anchor": ""},
    }))
    d = measure(root)["data"]
    t.eq(d["arc_state"], "active")
    t.eq(d["arc_id"], "A001")
    t.eq(d["arc_progress"], "building")
    t.eq(d["position"], "in-execution", "an active arc outranks a plan")


@cell(red="pre-implementation: a concluded arc and a null arc were not distinguished")
def cell_concluded_arc_and_null_arc_field(t: T) -> None:
    root = t.tmpdir() / "concluded"
    write(root / "ltads" / "arc-state.json", json.dumps({
        "schema": "arc-state/2", "last_arc_id": "A001",
        "arc": {"id": "A001", "status": "concluded", "driver_session": "s1",
                "started_at": "2026-01-01T00:00:00Z", "concluded_at": "2026-01-02T00:00:00Z",
                "consent": None, "progress": "done", "anchor": ""},
    }))
    t.eq(measure(root)["data"]["position"], "post-implementation")

    root2 = t.tmpdir() / "null-arc"
    write(root2 / "ltads" / "arc-state.json",
          json.dumps({"schema": "arc-state/2", "last_arc_id": "A001", "arc": None}))
    d2 = measure(root2)["data"]
    t.eq(d2["arc_state"], "none")
    t.eq(d2["position"], "none")


@cell(red="pre-implementation: an unparsable/foreign-schema arc-state.json was never handled")
def cell_unreadable_or_foreign_schema_is_unknown_never_a_crash(t: T) -> None:
    root = t.tmpdir() / "bad-json"
    write(root / "ltads" / "arc-state.json", "not json {{{")
    r = measure(root)
    t.eq(r["verdict"], "pass", "the oracle still measured -- it just could not read this ONE file")
    t.eq(r["data"]["arc_state"], "unknown")

    root2 = t.tmpdir() / "old-shape"
    write(root2 / "ltads" / "arc-state.json", json.dumps({"sessions": []}))
    t.eq(measure(root2)["data"]["arc_state"], "unknown", "the pre-rebuild registry shape is unknown")


if __name__ == "__main__":
    sys.exit(cells.main())
