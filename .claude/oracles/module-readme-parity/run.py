"""module-readme-parity (C2, RB-21/22, RB-44): README File Index vs. the real directory.

RB-44's parity half of today's `module-index`. Scope is git-TRACKED files only
(`uldf.git.ls_files`) -- a build artifact directory is normally gitignored and so never reaches
either the parity check or the presence check below.
"""

from __future__ import annotations

import os
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))

from uldf import Error, git, oracle  # noqa: E402

_HEADING = re.compile(r"^#{1,6}\s*(?:\d+\.\s*)?File Index\s*$", re.IGNORECASE)
_ANY_HEADING = re.compile(r"^#{1,6}\s")
_BACKTICK = re.compile(r"`([^`]+)`")
_SOURCE_EXTENSIONS = {".py", ".sh", ".ps1", ".js", ".ts", ".go", ".rs", ".cs", ".java",
                       ".cpp", ".c", ".rb"}


def _index_section(text: str) -> str | None:
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if _HEADING.match(line.strip()):
            body = []
            for later in lines[i + 1:]:
                if _ANY_HEADING.match(later.strip()):
                    break
                body.append(later)
            return "\n".join(body)
    return None


def _indexed_names(section: str) -> set[str]:
    names = set()
    for token in _BACKTICK.findall(section):
        token = token.strip().rstrip("/")
        if not token or "/" in token or "*" in token or " " in token:
            continue
        names.add(token)
    return names


def run(ctx: oracle.Context) -> dict:
    root = ctx.project_root
    try:
        tracked = git.ls_files(cwd=root)
    except Error as exc:
        return oracle.result("unknown", "could not list tracked files", reason=str(exc))

    by_dir: dict[str, list[str]] = {}
    for rel in tracked:
        d, _, name = rel.rpartition("/")
        by_dir.setdefault(d, []).append(name)

    parity_mismatches = []
    presence_gaps = []

    for d, names in sorted(by_dir.items()):
        readme_name = next((n for n in names if n.lower() == "readme.md"), None)
        source_count = sum(1 for n in names
                            if os.path.splitext(n)[1].lower() in _SOURCE_EXTENSIONS)

        if readme_name is None:
            if source_count >= 3:
                presence_gaps.append({"directory": d or ".", "source_file_count": source_count})
            continue

        readme_path = root / d / readme_name if d else root / readme_name
        try:
            text = readme_path.read_text(encoding="utf-8", errors="replace")
        except OSError as exc:
            parity_mismatches.append({"directory": d or ".", "reason": f"unreadable README: {exc}"})
            continue

        section = _index_section(text)
        if section is None:
            continue  # no recognized File Index heading -- outside this check (known_gaps)

        indexed = _indexed_names(section)
        real = {n for n in names if n.lower() != "readme.md"}

        missing_from_index = sorted(real - indexed)
        missing_from_dir = sorted(indexed - real)
        if missing_from_index or missing_from_dir:
            parity_mismatches.append({
                "directory": d or ".", "missing_from_index": missing_from_index,
                "missing_from_directory": missing_from_dir,
            })

    data = {
        "directories_scanned": len(by_dir),
        "parity_mismatches": parity_mismatches,
        "presence_gaps": presence_gaps,
    }
    if parity_mismatches or presence_gaps:
        return oracle.result(
            "fail",
            f"{len(parity_mismatches)} parity mismatch(es), {len(presence_gaps)} presence gap(s)",
            data=data,
        )
    return oracle.result("pass", f"{len(by_dir)} director(ies) scanned, no gaps", data=data)


if __name__ == "__main__":
    sys.exit(oracle.main(run))
