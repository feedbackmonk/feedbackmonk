"""pending-followups (C2, RB-21/22): the project CLAUDE.md `## Pending Follow-Ups` bullet lines.

Today's contract is deliberately narrow (RB-12): one line per stub is the record; bodies
externalized to `docs/pending/` are never read here. Only TOP-LEVEL bullets (`- `, no leading
whitespace) count -- an indented continuation paragraph under an entry is prose, not a second
stub, and an indented sub-bullet is not this contract's shape either.
"""

from __future__ import annotations

import os
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))

from uldf import oracle  # noqa: E402

SECTION_HEADING = "## Pending Follow-Ups"


def run(ctx: oracle.Context) -> dict:
    path = ctx.project_root / "CLAUDE.md"
    if not path.is_file():
        return oracle.result("pass", "no CLAUDE.md", data={"count": 0, "items": []})

    text = path.read_text(encoding="utf-8", errors="replace")
    items: list[str] = []
    in_section = False
    for line in text.splitlines():
        if line.startswith("## "):
            if in_section:
                break
            in_section = line.rstrip() == SECTION_HEADING
            continue
        if in_section and line.startswith("- "):
            items.append(line[2:].strip())

    data = {"count": len(items), "items": items}
    if not items:
        return oracle.result("pass", "no pending follow-ups", data=data)
    return oracle.result("pass", f"{len(items)} pending follow-up(s)", data=data)


if __name__ == "__main__":
    sys.exit(oracle.main(run))
