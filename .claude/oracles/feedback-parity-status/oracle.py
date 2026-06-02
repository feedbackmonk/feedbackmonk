#!/usr/bin/env python3
"""feedback-parity-status — Verification Oracle.

Answers: "Which of the GitCellar customer-#1 parity gaps (1-4) are closed in the
feedbackmonk codebase, and is the GitCellar cutover gate OPEN?"

This is the convergence gate for GitCellar's Path-C adoption (see
docs/integrations/gitcellar-adoption.md §8 + the GitCellar adoption intake's
PARITY CHECKLIST). GitCellar will NOT retire its internal feedback backend until
all four build-gaps report CLOSED here.

Anti-reward-hacking: parity is detected from ACTUAL CODE STATE (migrations,
handlers, routes, widget), never from a self-reported flag a worker could flip.
Each gap has a deterministic detector. Gap #5 (Forge bridge) is N/A — GitCellar
drops it; it is reported as such and excluded from the gate.

Exit codes:
  0  GATE OPEN — all four build-gaps detected CLOSED.
  3  GATE CLOSED — one or more gaps still open (normal pre-convergence state).
  2  oracle error (could not locate repo root / unreadable tree).

Usage:
  python3 oracle.py            # human-readable table + GATE line
  python3 oracle.py --json     # machine-readable JSON for GitCellar's gate script
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

# Repo root = three levels up from .claude/oracles/feedback-parity-status/
ORACLE_DIR = Path(__file__).resolve().parent
REPO_ROOT = ORACLE_DIR.parents[2]

MIGRATIONS = REPO_ROOT / "migrations"
HANDLERS = REPO_ROOT / "crates" / "feedbackmonk-api" / "src" / "handlers"
ROUTER = REPO_ROOT / "crates" / "feedbackmonk-api" / "src" / "router.rs"
WIDGET_SRC = REPO_ROOT / "widget" / "src"


def _read_all(paths) -> str:
    """Concatenate text of all given files (missing files contribute nothing)."""
    blob = []
    for p in paths:
        try:
            blob.append(p.read_text(encoding="utf-8", errors="ignore"))
        except (OSError, UnicodeError):
            continue
    return "\n".join(blob)


def _migrations_blob() -> str:
    if not MIGRATIONS.is_dir():
        return ""
    return _read_all(sorted(MIGRATIONS.glob("*.sql")))


def _handlers_blob() -> str:
    if not HANDLERS.is_dir():
        return ""
    files = list(HANDLERS.glob("*.rs"))
    if ROUTER.is_file():
        files.append(ROUTER)
    return _read_all(files)


def detect_gap1_attachments() -> tuple[bool, str]:
    """#1 Attachments: an `attachments` table migration must exist."""
    mig = _migrations_blob().lower()
    if "create table attachments" in mig or "create table if not exists attachments" in mig:
        # Widget-side capture/redaction is a secondary signal (not gate-blocking
        # on its own, but reported for visibility).
        widget = _read_all(sorted(WIDGET_SRC.rglob("*.ts"))) if WIDGET_SRC.is_dir() else ""
        wflag = "redact" in widget.lower() or "canvas" in widget.lower()
        return True, f"attachments table present in migrations (widget redaction: {'yes' if wflag else 'not detected'})"
    return False, "no `attachments` table found in migrations/"


def detect_gap2_crash() -> tuple[bool, str]:
    """#2 Crash correlation: `crash_event_id` must be added to the schema."""
    if "crash_event_id" in _migrations_blob():
        return True, "crash_event_id present in migrations"
    return False, "no `crash_event_id` column found in migrations/"


def detect_gap3_search() -> tuple[bool, str]:
    """#3 Admin full-text search: a search route or FTS migration must exist."""
    handlers = _handlers_blob()
    mig = _migrations_blob().lower()
    if "/admin/feedback/search" in handlers:
        return True, "admin feedback search route registered"
    if "tsvector" in mig or "to_tsvector" in mig:
        return True, "full-text-search (tsvector) migration present"
    return False, "no admin search route or tsvector migration found"


def detect_gap4_myfeedback() -> tuple[bool, str]:
    """#4 End-user my-feedback read API: JWT-sub-scoped read route must exist."""
    handlers = _handlers_blob()
    if (HANDLERS / "me_feedback.rs").is_file():
        return True, "handlers/me_feedback.rs present"
    if "/me/feedback" in handlers:
        return True, "/me/feedback route registered"
    return False, "no end-user my-feedback read route found (public surface still submit-only)"


GAPS = [
    ("1", "Attachments (screenshots + redaction + log capture)", detect_gap1_attachments),
    ("2", "Crash-event correlation (crash_event_id + worker)", detect_gap2_crash),
    ("3", "Admin full-text search", detect_gap3_search),
    ("4", "End-user my-feedback list + reply-thread read API", detect_gap4_myfeedback),
]


def evaluate() -> dict:
    if not REPO_ROOT.is_dir() or not (REPO_ROOT / "crates").is_dir():
        raise RuntimeError(f"repo root not found or not a feedbackmonk tree: {REPO_ROOT}")
    results = []
    for num, title, detector in GAPS:
        closed, detail = detector()
        results.append({"gap": num, "title": title, "closed": closed, "detail": detail})
    gate_open = all(r["closed"] for r in results)
    return {
        "gaps": results,
        "gap5_forge_bridge": "N/A — GitCellar drops it; excluded from gate (DEC-FBR-06)",
        "gate_open": gate_open,
        "closed_count": sum(1 for r in results if r["closed"]),
        "total": len(results),
    }


def main(argv: list[str]) -> int:
    try:
        report = evaluate()
    except Exception as exc:  # noqa: BLE001 — oracle must report, not crash
        print(f"feedback-parity-status: ERROR — {exc}", file=sys.stderr)
        return 2

    if "--json" in argv:
        print(json.dumps(report, indent=2))
    else:
        print("feedback-parity-status — GitCellar customer-#1 cutover gate")
        print(f"  {report['closed_count']}/{report['total']} build-gaps CLOSED\n")
        for r in report["gaps"]:
            mark = "CLOSED" if r["closed"] else "OPEN  "
            print(f"  [{mark}] Gap #{r['gap']}: {r['title']}")
            print(f"           ↳ {r['detail']}")
        print(f"\n  Gap #5 (Forge bridge): {report['gap5_forge_bridge']}")
        gate = "OPEN ✅ — GitCellar may retire its internal backend" if report["gate_open"] \
            else "CLOSED ⛔ — gaps remain; GitCellar must NOT cut over yet"
        print(f"\n  CUTOVER GATE: {gate}")

    return 0 if report["gate_open"] else 3


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
