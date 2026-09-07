#!/usr/bin/env python3
"""`project-index` (ISF-6, ISF-7): the mechanical half of the instruction-surface standard.

The judgment half -- ISF-2's two questions -- reaches whoever edits `CLAUDE.md` as the delivered
segment `segments/-global/project-index.md`. This file grades only what a program can recognise
without judgment: four shapes that are `fail`, and everything else -- size included -- `warn`.

**Nothing here fails on size** (DEC-472). A wall at any height is cut-to-fit at that height; the
byte FAIL this replaces produced three files within 330 B of it in one afternoon, one of them
reporting 12 KB of its own content unreachable. The alarm rides every session-start briefing
instead, with the last delta beside it, so the trend is continuous and free.

Verdict vocabulary is C2 Table B: `fail` beats `unknown` beats `warn` beats `pass`.
"""

from __future__ import annotations

import os
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))

from uldf import Error, git, jsonio, limits, oracle  # noqa: E402

from pathlib import Path  # noqa: E402

INDEX_NAME = "CLAUDE.md"
FOLLOWUP_HEADING = re.compile(r"^##\s+pending\s+follow[- ]?ups\s*$", re.IGNORECASE)

#: ISF-6 shape 1. Past-tense openers -- the form a changelog entry takes when it is pasted into an
#: index. Matched at the FIRST word of a block only: `Added the retry loop` is narration, while
#: `Add the retry loop` is an instruction and `Files added under src/ are ...` is a fact.
FINISHED_WORK_TOKENS = (
    "added", "completed", "delivered", "deleted", "done", "finished", "fixed", "implemented",
    "introduced", "landed", "merged", "migrated", "refactored", "removed", "renamed", "replaced",
    "resolved", "rewrote", "rewritten", "shipped", "formerly", "historically", "originally",
    "previously",
)

#: Shape 4. Ten words, because a run of ten consecutive words shared with the global file is a
#: restatement rather than a coincidence of vocabulary; 25% of a paragraph's shingles, because a
#: paragraph that merely cites a global rule shares its noun phrases and not its sentences.
SHINGLE_WORDS = 10
SHINGLE_OVERLAP = 0.25

#: A backticked token is graded for existence only when it is unambiguously a repo-relative path.
_PATH_LIKE = re.compile(r"^[A-Za-z0-9._][A-Za-z0-9._/-]*$")
_PATH_EXTENSIONS = (".md", ".py", ".json", ".jsonl", ".txt", ".yml", ".yaml", ".toml", ".cfg",
                    ".ini", ".ts", ".tsx", ".js", ".jsx", ".cs", ".rs", ".go", ".sh", ".ps1")
_BACKTICKED = re.compile(r"`([^`\n]+)`")
_WORD = re.compile(r"[a-z0-9]+")
_FENCE = re.compile(r"^\s*(```|~~~)")


# --- reading the file ------------------------------------------------------------------------

class Block:
    """One block of the index: a paragraph, or a single top-level bullet with its wrapping.

    A bullet is its own block rather than one line of a list's block, because the narration the
    diet reports found came as list items -- a changelog pasted in as bullets, whose third entry
    is as much an opening as its first. `line` is 1-based and names the block's first line, so a
    finding points at something a reader can open. Fenced code is excluded entirely: a code
    sample is not prose and its first word is not an author's opening.
    """

    __slots__ = ("line", "lines")

    def __init__(self, line: int, lines: list[str]) -> None:
        self.line = line
        self.lines = lines

    @property
    def text(self) -> str:
        return "\n".join(self.lines)


_BULLET = re.compile(r"^([-*+]|\d+\.)\s")


def blocks(text: str) -> list[Block]:
    out: list[Block] = []
    current: list[str] = []
    start = 1
    in_fence = False

    def flush() -> None:
        nonlocal current
        if current:
            out.append(Block(start, current))
            current = []

    for i, raw in enumerate(text.splitlines(), start=1):
        if _FENCE.match(raw):
            in_fence = not in_fence
            flush()
            continue
        if in_fence:
            continue
        if not raw.strip():
            flush()
            continue
        if _BULLET.match(raw) or not current:
            flush()
            start = i
        current.append(raw)
    flush()
    return out


def _opening_word(line: str) -> str:
    """The first prose word of a block, with markdown decoration stripped.

    A heading, a bullet, a table row and a bold run all decorate the same opening; the token
    check is about what the author wrote, not how they marked it up.
    """
    stripped = line.strip()
    stripped = re.sub(r"^[#>\s]*", "", stripped)
    stripped = re.sub(r"^([-*+]|\d+\.)\s+", "", stripped)
    stripped = re.sub(r"^\|\s*", "", stripped)
    stripped = stripped.lstrip("*_`\"'([")
    match = re.match(r"[A-Za-z']+", stripped)
    return match.group(0).lower() if match else ""


# --- shape 1: a block opening with a finished-work token -------------------------------------

def finished_work_openings(text: str) -> list[dict]:
    return [{"line": b.line, "word": word, "excerpt": b.lines[0].strip()[:120]}
            for b in blocks(text)
            for word in [_opening_word(b.lines[0])]
            if word in FINISHED_WORK_TOKENS]


# --- shapes 2 and 3: the follow-up section ----------------------------------------------------

def followup_section(text: str) -> tuple[int, list[str]]:
    """`(first_line_number, lines)` of the `## Pending Follow-Ups` body; `(0, [])` when absent."""
    lines = text.splitlines()
    start = None
    for i, line in enumerate(lines):
        if start is None:
            if FOLLOWUP_HEADING.match(line):
                start = i + 1
            continue
        if line.startswith("## "):
            return start + 1, lines[start:i]
    if start is None:
        return 0, []
    return start + 1, lines[start:]


def followup_findings(text: str) -> tuple[list[dict], list[dict], int]:
    """`(inlined_bodies, oversized_stubs, entry_count)` over the follow-up section.

    A stub is a top-level `- ` line plus the indented lines that WRAP it -- wrapping is how a
    one-line entry looks in an 100-column file, and calling it a body would make the shape
    unsatisfiable. An inlined body is what wrapping is not: a nested bullet, a paragraph after a
    blank line, a sub-heading, or a fenced block.
    """
    first_line, body = followup_section(text)
    inlined: list[dict] = []
    oversized: list[dict] = []
    if not body:
        return inlined, oversized, 0

    entries: list[tuple[int, list[str]]] = []
    current: list[str] | None = None
    current_line = 0
    seen_blank = False
    in_fence = False

    for offset, raw in enumerate(body):
        line_no = first_line + offset
        if _FENCE.match(raw):
            in_fence = not in_fence
            inlined.append({"line": line_no, "why": "a fenced block under a stub",
                            "excerpt": raw.strip()[:120]})
            continue
        if in_fence:
            continue
        if not raw.strip():
            seen_blank = True
            continue
        if re.match(r"^[-*+]\s", raw):
            if current is not None:
                entries.append((current_line, current))
            current, current_line, seen_blank = [raw], line_no, False
            continue
        if raw.startswith("#"):
            inlined.append({"line": line_no, "why": "a heading inside the follow-up section",
                            "excerpt": raw.strip()[:120]})
            if current is not None:
                entries.append((current_line, current))
                current = None
            continue
        if current is None:
            continue
        if re.match(r"^\s+[-*+]\s", raw):
            inlined.append({"line": line_no, "why": "a nested bullet under a stub",
                            "excerpt": raw.strip()[:120]})
            continue
        if seen_blank:
            inlined.append({"line": line_no, "why": "a paragraph under a stub",
                            "excerpt": raw.strip()[:120]})
            continue
        current.append(raw)

    if current is not None:
        entries.append((current_line, current))

    for line_no, entry_lines in entries:
        size = len("\n".join(entry_lines).encode("utf-8"))
        if size > limits.FOLLOWUP_STUB_BYTES:
            oversized.append({"line": line_no, "bytes": size,
                              "limit": limits.FOLLOWUP_STUB_BYTES,
                              "excerpt": entry_lines[0].strip()[:120]})
    return inlined, oversized, len(entries)


# --- shape 4: a paragraph restating the rendered global file ----------------------------------

def shingles(text: str, n: int = SHINGLE_WORDS) -> set[tuple[str, ...]]:
    words = _WORD.findall(text.lower())
    return {tuple(words[i:i + n]) for i in range(len(words) - n + 1)}


def restated_paragraphs(text: str, global_text: str) -> list[dict]:
    reference = shingles(global_text)
    if not reference:
        return []
    out: list[dict] = []
    for block in blocks(text):
        if block.lines[0].lstrip().startswith("#"):
            continue
        own = shingles(block.text)
        if not own:
            continue
        overlap = len(own & reference) / len(own)
        if overlap >= SHINGLE_OVERLAP:
            out.append({"line": block.line, "overlap": round(overlap, 3),
                        "shingles": len(own), "excerpt": block.lines[0].strip()[:120]})
    return out


# --- warn: a backticked path that resolves to nothing -----------------------------------------

def unresolvable_paths(text: str, project_root: Path) -> list[dict]:
    seen: dict[str, int] = {}
    for block in blocks(text):
        for offset, line in enumerate(block.lines):
            for token in _BACKTICKED.findall(line):
                token = token.strip()
                if token in seen or not _is_gradable_path(token):
                    continue
                if not (project_root / token).exists():
                    seen[token] = block.line + offset
    return [{"path": p, "line": n} for p, n in sorted(seen.items(), key=lambda kv: kv[1])]


def _is_gradable_path(token: str) -> bool:
    """Only an unambiguously repo-relative path is graded; everything else is left alone.

    The token must hold a real separator -- two non-empty segments -- because a bare `sync.py`
    and a bare `deferred/` are named relative to a context the sentence supplies, not to the
    repository root, and grading them warns about files that are exactly where they belong. A
    glob, a placeholder, a home-relative or absolute path, a URL and a command line are each a
    thing this oracle cannot resolve, and guessing at one produces a warn an author cannot act on.
    """
    if not _PATH_LIKE.match(token) or token.startswith(("http", "~", "/")):
        return False
    if any(ch in token for ch in " *?<>|"):
        return False
    if "://" in token:
        return False
    return bool(re.search(r"[^/]/[^/]", token))


# --- size and growth ---------------------------------------------------------------------------

def alarm_bytes(project_root: Path) -> tuple[int, str]:
    """`(line, source)`. A project raises its own line in TRACKED config, so the judgment that
    an index deserves more room is in git history rather than in a cut nobody can see."""
    config = jsonio.read(project_root / ".claude" / "config.json", default=None)
    if isinstance(config, dict):
        value = (config.get("limits") or {}).get("project_index_alarm_bytes")
        if isinstance(value, int) and not isinstance(value, bool) and value > 0:
            return value, "project"
    return limits.PROJECT_INDEX_ALARM_BYTES, "framework"


def _lf_bytes(text: str) -> int:
    """Byte length with line endings normalised to LF.

    Every size that enters a COMPARISON is measured this way. `git show` hands back LF whatever
    the file on disk holds, so on a CRLF checkout the raw working-tree size exceeds the blob's by
    one byte per line -- which read as a clean tree being dirty, and reported a phantom `+75 B`
    delta over an index nobody had touched. The alarm still reports the file's real size on disk;
    only the delta is normalised, because a delta between two differently-encoded numbers is not
    a delta at all.
    """
    return len(text.replace("\r\n", "\n").encode("utf-8"))


def _blob_bytes(project_root: Path, ref: str) -> int | None:
    """The index's LF-normalised size at `ref`, or `None` when it does not exist there (an
    unborn HEAD, a first commit, a tree that is not a repository at all). `None` is not zero:
    absence of a prior size is not a delta of the whole file."""
    try:
        return _lf_bytes(git.show(ref, INDEX_NAME, cwd=project_root).decode("utf-8", "replace"))
    except Error:
        return None


def growth(project_root: Path, now: int) -> tuple[int | None, str]:
    """`(delta, basis)` -- this change's growth when the file is dirty, else the last commit's.

    Both readings matter and only one can be the number: at a finalize the file is dirty and the
    growth that is about to be committed is the question; at a session start it is clean and the
    last commit's jump is. Naming the basis is what keeps the two from being confused. `now` is
    the LF-normalised size, for the reason `_lf_bytes` gives.
    """
    head = _blob_bytes(project_root, "HEAD")
    if head is None:
        return None, "no committed CLAUDE.md"
    if head != now:
        return now - head, "working tree vs HEAD"
    prev = _blob_bytes(project_root, "HEAD~1")
    if prev is None:
        return None, "no HEAD~1"
    return head - prev, "HEAD vs HEAD~1"


# --- the oracle ---------------------------------------------------------------------------------

def run(ctx: oracle.Context) -> dict:
    path = ctx.project_root / INDEX_NAME
    if not path.is_file():
        return oracle.result("pass", f"no {INDEX_NAME}", data={"bytes": None, "graded": False})

    raw = path.read_bytes()
    text = raw.decode("utf-8", errors="replace")
    size = len(raw)
    alarm, alarm_source = alarm_bytes(ctx.project_root)
    delta, basis = growth(ctx.project_root, _lf_bytes(text))

    openings = finished_work_openings(text)
    inlined, oversized, entries = followup_findings(text)

    global_path = ctx.home / INDEX_NAME
    global_text = global_path.read_text(encoding="utf-8", errors="replace") \
        if global_path.is_file() else ""
    restated = restated_paragraphs(text, global_text) if global_text else []

    unresolved = unresolvable_paths(text, ctx.project_root)

    data = {
        "bytes": size, "alarm_bytes": alarm, "alarm_source": alarm_source,
        "delta_bytes": delta, "delta_basis": basis, "followup_entries": entries,
        "finished_work_openings": openings, "inlined_followup_bodies": inlined,
        "oversized_stubs": oversized, "restated_global_paragraphs": restated,
        "unresolvable_paths": unresolved,
        "global_compared": bool(global_text), "global_file": global_path.as_posix(),
    }

    fails: list[str] = []
    if openings:
        fails.append(f"{len(openings)} finished-work opening(s) @{openings[0]['line']}")
    if inlined:
        fails.append(f"{len(inlined)} inlined follow-up body/bodies @{inlined[0]['line']}")
    if oversized:
        fails.append(f"{len(oversized)} stub(s) over {limits.FOLLOWUP_STUB_BYTES} B "
                     f"@{oversized[0]['line']}")
    if restated:
        fails.append(f"{len(restated)} restated global paragraph(s) @{restated[0]['line']}")

    warns: list[str] = []
    if size > alarm:
        warns.append(f"over the {alarm} B alarm ({alarm_source})")
    if delta is not None and delta > limits.PROJECT_INDEX_GROWTH_BYTES:
        warns.append(f"grew past {limits.PROJECT_INDEX_GROWTH_BYTES} B in one change")
    if not global_text:
        warns.append("no rendered global file: the restatement shape was not measured")
    if unresolved:
        warns.append(f"{len(unresolved)} dead backticked path(s) ({unresolved[0]['path']})")

    trend = (f"{size} B / {alarm} B alarm, "
             + (f"{delta:+d} B {basis}" if delta is not None else f"delta unknown ({basis})"))
    verdict, notes = ("fail", fails) if fails else ("warn", warns) if warns else ("pass", [])
    summary = f"{trend}; {'; '.join(notes)}" if notes else f"{trend}; shape clean"
    return oracle.result(verdict, _fit(summary), data=data)


def _fit(summary: str, cap: int = 200) -> str:
    """C2 caps a summary at 200 characters and `oracle.result` RAISES over it, which would turn a
    finding into an `unknown`. A finding is worth more truncated than lost."""
    return summary if len(summary) <= cap else summary[:cap - 1] + "…"


if __name__ == "__main__":
    sys.exit(oracle.main(run))
