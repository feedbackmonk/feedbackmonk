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
#: A token that could be a file NAME at all: it holds only what a filename holds, and it does not
#: OPEN with a character no filename opens with. `_` and `.` lead real files -- `__init__.py`,
#: `_helper.ps1`, `.gitignore` -- so they are admitted here and `.await` is left to the tests below;
#: `-Json`, `$derived`, `#[ignore]`, `""`, `<table>` and `page.evaluate(` are refused on shape.
_FILENAME = re.compile(r"^[A-Za-z0-9_.][A-Za-z0-9._-]*$")
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


def _indexed_names(section: str, directory: pathlib.Path, real: set[str],
                   suffixes: set[str]) -> set[str]:
    """The backticked tokens in a File Index that are *claims about a file in this directory*.

    A File Index describes its module, so its prose backticks identifiers, literals, symbols and
    call sites alongside the filenames. Treating every one of them as a filename is how this check
    reported `""`, `0`, `=`, `#[ignore]`, `.await`, `CELL_IDS` and `page.goto` as "files that are
    gone" -- 24 of them from `hooks/README.md` alone, and enough across five instrumented projects
    to hold proof (3) red on nothing.

    Three tests, in order, and a token passes on the FIRST that applies:

      * it names a tracked file that is really here -- the index is right about it;
      * it names a directory that is really here -- a subdirectory is a legitimate index entry and
        is not a file claim, so it is dropped rather than reported;
      * its extension is one this directory's own tracked files actually use -- the claim is about
        a file that is gone, which is the defect this check exists for.

    The third test is what keeps a stale entry detectable, and it is deliberately narrow: an
    extension no sibling uses (`.goto`, `.await`, `.modalities`) is prose, not a file. Two shapes
    of stale entry are the gap that buys it, both in `known_gaps`: the last file of its extension
    in a directory, and a name with no extension at all (`Makefile`, `LICENSE`). A file that is
    THERE is never missed either way -- the first test sees it whatever it is called.
    """
    names = set()
    for token in _BACKTICK.findall(section):
        token = token.strip().rstrip("/")
        if not _FILENAME.match(token):
            continue
        if token in real:
            names.add(token)
        elif (directory / token).is_dir():
            continue
        elif os.path.splitext(token)[1].lower() in suffixes:
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

        real = {n for n in names if n.lower() != "readme.md"}
        # Suffixes come from EVERY tracked child, README.md included: in a directory whose only
        # markdown file is the README, dropping it would take `.md` out of the set and make a
        # stale `notes.md` entry invisible.
        suffixes = {os.path.splitext(n)[1].lower() for n in names} - {""}
        indexed = _indexed_names(section, readme_path.parent, real, suffixes)

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
