#!/usr/bin/env python3
"""Self-test for the `arc-state` oracle -- a C6 cell module."""

from __future__ import annotations

import importlib.util
import json
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
    spec = importlib.util.spec_from_file_location("arc_state_run", HERE / "run.py")
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


RUN = _load_run()


def measure(root: Path) -> dict:
    ctx = oracle.Context(project_root=root, anchor_root=root, home=paths.home())
    return RUN.run(ctx)


def write(root: Path, obj) -> None:
    p = root / "ltads" / "arc-state.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(obj, str):
        p.write_text(obj, encoding="utf-8")
    else:
        p.write_text(json.dumps(obj), encoding="utf-8")


CONFORMING = {
    "schema": "arc-state/2", "last_arc_id": "A007",
    "arc": {"id": "A007", "status": "active", "driver_session": "sess-1",
            "started_at": "2026-09-06T00:00:00Z", "concluded_at": None,
            "consent": {"stop_policy": "run", "propagate": "auto",
                        "expires_at": "2026-09-13T00:00:00Z", "granted_by": "user"},
            "progress": "building the runner", "anchor": "x"},
}


@cell(red="pre-implementation: run() did not exist")
def cell_absent_file_is_pass_no_arc(t: T) -> None:
    root = t.tmpdir()
    r = measure(root)
    t.eq(r["verdict"], "pass")
    t.eq(r["data"]["exists"], False)
    t.eq(r["data"]["status"], None)


@cell(red="pre-implementation: the conforming shape was never rendered at all")
def cell_conforming_arc_renders_status_driver_consent_progress(t: T) -> None:
    root = t.tmpdir()
    write(root, CONFORMING)
    r = measure(root)
    t.eq(r["verdict"], "pass")
    d = r["data"]
    t.eq(d["status"], "active")
    t.eq(d["driver_session"], "sess-1")
    t.eq(d["progress"], "building the runner")
    t.eq(d["consent_expires_at"], "2026-09-13T00:00:00Z")
    t.eq(d["arc_id"], "A007")


@cell(red="pre-implementation: a null `arc` field was not distinguished from an active one")
def cell_null_arc_is_pass_no_active_arc(t: T) -> None:
    root = t.tmpdir()
    write(root, {"schema": "arc-state/2", "last_arc_id": "A001", "arc": None})
    r = measure(root)
    t.eq(r["verdict"], "pass")
    t.eq(r["data"]["exists"], True)
    t.eq(r["data"]["status"], None)


@cell(red="pre-implementation: an unrecognized schema major was never checked")
def cell_unrecognized_major_is_unknown(t: T) -> None:
    root = t.tmpdir()
    write(root, {"schema": "arc-state/99", "arc": None})
    r = measure(root)
    t.eq(r["verdict"], "unknown")
    t.ok(r["reason"], "an unknown verdict must carry a reason")


@cell(red="pre-implementation: the pre-rebuild sessions-array shape was not refused")
def cell_pre_rebuild_shape_is_unknown_not_a_crash(t: T) -> None:
    root = t.tmpdir()
    write(root, {"sessions": []})
    r = measure(root)
    t.eq(r["verdict"], "unknown")


@cell(red="pre-implementation: unparsable JSON crashed the oracle instead of yielding unknown")
def cell_unparsable_json_is_unknown(t: T) -> None:
    root = t.tmpdir()
    write(root, "not json {{{")
    r = measure(root)
    t.eq(r["verdict"], "unknown")
    t.ok(r["reason"], r)


@cell(red="pre-implementation: an out-of-vocabulary status was never validated against Table E")
def cell_status_outside_table_e_is_unknown(t: T) -> None:
    root = t.tmpdir()
    write(root, {"schema": "arc-state/2", "arc": {"id": "A1", "status": "in-progress"}})
    r = measure(root)
    t.eq(r["verdict"], "unknown")


if __name__ == "__main__":
    sys.exit(cells.main())
