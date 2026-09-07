"""markdown-link-validity (C2, RB-21/22): do internal markdown links resolve?

Scope is every git-TRACKED `*.md` file (`uldf.git.ls_files`) -- no config, unlike the old
starter oracle's `scan_directories` list. Links inside fenced code blocks and inline code spans
are ignored; the `#fragment` is stripped before resolution; an uncommitted deletion of a tracked
target is reported separately and never reds the verdict (see `oracle.json`'s `known_gaps`).
"""

from __future__ import annotations

import os
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))

from uldf import Error, git, oracle, paths  # noqa: E402

_LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
_EXTERNAL_PREFIXES = ("http://", "https://", "mailto:", "ftp://", "tel:")


def _is_external_or_bare_anchor(dest: str) -> bool:
    lowered = dest.strip().lower()
    return lowered == "" or lowered.startswith("#") or lowered.startswith(_EXTERNAL_PREFIXES)


def _iter_lines_outside_code(text: str):
    """Yield `(lineno, line)` for every line NOT inside a fenced block, code spans stripped."""
    in_fence = False
    fence_marker = None
    for lineno, raw_line in enumerate(text.splitlines(), start=1):
        stripped = raw_line.strip()
        if stripped[:3] in ("```", "~~~"):
            marker = stripped[:3]
            if not in_fence:
                in_fence, fence_marker = True, marker
            elif marker == fence_marker:
                in_fence, fence_marker = False, None
            continue
        if in_fence:
            continue
        yield lineno, re.sub(r"`[^`]*`", "", raw_line)


def run(ctx: oracle.Context) -> dict:
    root = ctx.project_root
    try:
        md_files = git.ls_files(cwd=root, paths=["*.md"])
    except Error as exc:
        return oracle.result("unknown", "could not list tracked markdown files", reason=str(exc))

    deleted: set[str] = set()
    try:
        deleted_result = git.run(["ls-files", "--deleted"], cwd=root, timeout_s=15)
        if deleted_result.rc == 0:
            deleted = {p for p in deleted_result.out.split("\n") if p}
    except Error:
        pass

    checked = 0
    broken: list[dict] = []
    uncommitted_deletions: list[dict] = []

    for rel in sorted(md_files):
        source = root / rel
        if not source.is_file():
            continue
        try:
            text = source.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for lineno, line in _iter_lines_outside_code(text):
            for match in _LINK.finditer(line):
                dest = match.group(1).strip()
                if _is_external_or_bare_anchor(dest):
                    continue
                target = dest.split("#", 1)[0].strip()
                if not target:
                    continue
                checked += 1
                resolved = (source.parent / target)
                try:
                    resolved_rel = paths.rel(resolved, root)
                except Error:
                    resolved_rel = None
                if resolved.exists():
                    continue
                row = {"source": rel, "line": lineno, "link": dest,
                       "resolved_path": resolved_rel if resolved_rel is not None
                       else resolved.as_posix()}
                if resolved_rel is not None and resolved_rel in deleted:
                    uncommitted_deletions.append(row)
                else:
                    broken.append(row)

    data = {
        "checked": checked, "broken_count": len(broken), "scanned_files": len(md_files),
        "broken": broken, "uncommitted_deletion_count": len(uncommitted_deletions),
        "uncommitted_deletions": uncommitted_deletions,
    }
    if broken:
        return oracle.result("fail", f"{len(broken)} of {checked} link(s) broken", data=data)
    return oracle.result("pass", f"{checked} link(s) checked across {len(md_files)} file(s)",
                          data=data)


if __name__ == "__main__":
    sys.exit(oracle.main(run))
