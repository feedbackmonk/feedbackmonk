"""workflow-position (C2, RB-21/22): where a project sits in ideate -> spec -> plan -> execute.

Answered from files that exist: `docs/specs/*.md` presence, the newest `docs/planning/plans/*.md`,
and `ltads/arc-state.json` (C5). Absence of every artifact is a valid `pass` ("no ULDF artifacts")
-- never `unknown`, and never a label invented for a state nothing on disk supports.
"""

from __future__ import annotations

import os
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))

from uldf import Error, jsonio, oracle, paths  # noqa: E402

_ARC_STATUSES = ("active", "paused", "concluded")


def run(ctx: oracle.Context) -> dict:
    root = ctx.project_root

    specs_dir = root / "docs" / "specs"
    spec_files = sorted(p.name for p in specs_dir.glob("*.md")) if specs_dir.is_dir() else []
    has_spec = bool(spec_files)

    plans_dir = root / "docs" / "planning" / "plans"
    plan_files = sorted(plans_dir.glob("*.md")) if plans_dir.is_dir() else []
    latest_plan = None
    if plan_files:
        newest = max(plan_files, key=lambda p: p.stat().st_mtime)
        latest_plan = paths.rel(newest, root)

    arc_path = root / "ltads" / "arc-state.json"
    arc_state = "none"
    arc_id = None
    arc_progress = None
    try:
        record = jsonio.read(arc_path, default=None)
    except Error:
        record = "unparsable"

    if record == "unparsable":
        arc_state = "unknown"
    elif isinstance(record, dict) and str(record.get("schema", "")).startswith("arc-state/"):
        arc = record.get("arc")
        if arc is None:
            arc_state = "none"
        elif isinstance(arc, dict) and arc.get("status") in _ARC_STATUSES:
            arc_state = arc["status"]
            arc_id = arc.get("id")
            arc_progress = arc.get("progress")
        else:
            arc_state = "unknown"
    elif record is not None:
        arc_state = "unknown"

    if arc_state in ("active", "paused"):
        position = "in-execution"
    elif arc_state == "concluded":
        position = "post-implementation"
    elif latest_plan is not None:
        position = "post-plan"
    elif has_spec:
        position = "post-spec"
    else:
        position = "none"

    data = {
        "position": position,
        "has_spec": has_spec,
        "spec_file_count": len(spec_files),
        "latest_plan": latest_plan,
        "arc_state": arc_state,
        "arc_id": arc_id,
        "arc_progress": arc_progress,
    }
    if position == "none":
        return oracle.result("pass", "no ULDF artifacts", data=data)
    return oracle.result("pass", f"position={position}", data=data)


if __name__ == "__main__":
    sys.exit(oracle.main(run))
