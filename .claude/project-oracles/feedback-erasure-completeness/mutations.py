#!/usr/bin/env python3
"""Mutation runner for this oracle: proves each recorded mutation turns it red.

Copies the parts of a source tree the oracle reads (`crates/` without build
output, and `migrations/`) into a fresh temporary directory, runs the oracle on
the unmutated copy (must exit 0), then applies each row of `mutations.json` to
the copy one at a time, runs the oracle with `--root <copy>` (must exit 1), and
restores the touched files from the source before the next row. The source
tree is only ever read; nothing is written outside the temporary copy, which
is deleted at the end.

Usage:
    python mutations.py [--root <source tree>] [--spec <mutations.json>] [--only ID ...]

`--root` defaults to this repository. The oracle run is the `oracle.py` beside
this file, so a candidate oracle is judged against the recorded mutations.

Spec rows: {"id", "why", "edits": [{"file", "old", "new"} | {"file", "content"}]}.
An `old` must occur exactly once in its file, or the row is an error. A
`content` edit creates (or overwrites) the file in the copy.

Exit 0: the clean copy exits 0 (before and after) and every row exits 1.
Exit 1: any other outcome, each named.
"""
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ORACLE = HERE / "oracle.py"
COPIED = ("crates", "migrations")
SKIP = shutil.ignore_patterns("target", "node_modules", ".git")


def run_oracle(copy: Path) -> tuple[int, str]:
    p = subprocess.run([sys.executable, str(ORACLE), "--root", str(copy)],
                       capture_output=True, text=True, encoding="utf-8", errors="replace")
    return p.returncode, p.stdout + p.stderr


def detail(out: str) -> list[str]:
    lines = [l.strip() for l in out.splitlines() if l.strip()]
    return [l for l in lines if not l.startswith(("FAIL", "PASS", "Probe", "Remediation"))][:4]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", type=Path, default=HERE.parents[2], help="source tree (read only)")
    ap.add_argument("--spec", type=Path, default=HERE / "mutations.json")
    ap.add_argument("--only", nargs="*", help="run only these row ids")
    args = ap.parse_args()
    src = args.root.resolve()
    rows = json.loads(args.spec.read_text(encoding="utf-8"))
    if args.only:
        rows = [r for r in rows if r["id"] in set(args.only)]
    bad: list[str] = []
    with tempfile.TemporaryDirectory(prefix="oracle-mut-") as tmp:
        copy = Path(tmp)
        for d in COPIED:
            if (src / d).is_dir():
                shutil.copytree(src / d, copy / d, ignore=SKIP)
        code, out = run_oracle(copy)
        print(f"clean: exit {code}")
        if code != 0:
            bad.append("clean copy")
            for l in detail(out):
                print(f"    {l[:300]}")
        for row in rows:
            touched: list[Path] = []
            try:
                for e in row["edits"]:
                    rel = e["file"]
                    f = copy / rel
                    touched.append(Path(rel))
                    if "old" in e:
                        s = f.read_text(encoding="utf-8")
                        n = s.count(e["old"])
                        if n != 1:
                            raise ValueError(f"`old` occurs {n}x in {rel}")
                        f.write_text(s.replace(e["old"], e["new"]), encoding="utf-8", newline="")
                    else:
                        f.parent.mkdir(parents=True, exist_ok=True)
                        f.write_text(e["content"], encoding="utf-8", newline="")
                code, out = run_oracle(copy)
                verdict = "red" if code == 1 else "NOT RED"
                print(f"{row['id']}: exit {code} ({verdict}) -- {row.get('why', '')}")
                for l in detail(out):
                    print(f"    {l[:300]}")
                if code != 1:
                    bad.append(row["id"])
            except (OSError, ValueError) as exc:
                print(f"{row['id']}: ERROR {exc}")
                bad.append(row["id"])
            finally:
                for rel in touched:
                    orig, f = src / rel, copy / rel
                    if orig.exists():
                        shutil.copyfile(orig, f)
                    elif f.exists():
                        f.unlink()
        code, out = run_oracle(copy)
        print(f"restored: exit {code}")
        if code != 0:
            bad.append("restored copy")
    if bad:
        print(f"FAIL: {', '.join(bad)}")
        return 1
    print(f"PASS: clean copy green, {len(rows)} mutation(s) red")
    return 0


if __name__ == "__main__":
    sys.exit(main())
