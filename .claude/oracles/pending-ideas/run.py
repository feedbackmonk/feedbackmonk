"""pending-ideas (C2, RB-21/22, RB-12): open briefs in `docs/planning/deferred/`.

RB-12, verbatim: "lists open briefs by filename and nothing else". The old starter oracle read
front matter, a status vocabulary, and a done-word census (DEFER-239's whole history); this
contract deliberately carries none of that -- a brief's front matter can lie about being done and
a census goes stale, but a FILENAME cannot. Every `*.md` except `README.md` (case-insensitive),
sorted, is the whole answer.
"""

from __future__ import annotations

import os
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))

from uldf import oracle  # noqa: E402

DEFERRED_REL = ("docs", "planning", "deferred")


def run(ctx: oracle.Context) -> dict:
    deferred_dir = ctx.project_root.joinpath(*DEFERRED_REL)
    if not deferred_dir.is_dir():
        return oracle.result("pass", "no docs/planning/deferred/ directory",
                              data={"count": 0, "files": []})

    files = sorted(
        p.name for p in deferred_dir.glob("*.md") if p.name.lower() != "readme.md"
    )
    data = {"count": len(files), "files": files}
    if not files:
        return oracle.result("pass", "no open briefs", data=data)
    return oracle.result("pass", f"{len(files)} open brief(s)", data=data)


if __name__ == "__main__":
    sys.exit(oracle.main(run))
