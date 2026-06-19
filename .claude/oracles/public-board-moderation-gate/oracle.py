#!/usr/bin/env python3
"""public-board-moderation-gate Verification Oracle (canonical implementation).

THE trust boundary between public submission and public EXPOSURE (FR-FBR-25a
sibling, applied to the public feedback board). It proves — FROM CODE, not from
a self-reported `is_public` flag — that no public-board endpoint can return a
feedback row whose `moderation_status != approved`, and that the board wire
shape leaks no submitter PII.

This is the anti-reward-hacking leg of the Public Feedback Board plan
(Testability Gate Flag 1, the highest plan-wide fidelity risk, Q2=5). A standard
"approve -> row appears on board" test confirms the happy path; it does NOT
prove the ABSENCE of a code path that returns an unapproved row, nor that the
board response withholds submitter identity. An unapproved row on the public
board is the exact spam/abuse/off-brand exposure this feature exists to prevent;
a PII leak is a privacy regression (DEC-FBR-02 / Q24 class).

THREE probes (detection-from-code, co-evolving with Worker A):

  A) STATE MACHINE (static, source parse, LIVE in Stage 0):
     `feedbackmonk-core/src/moderation.rs` must prove the structural gate —
     `is_publicly_visible` classifies EXACTLY `Approved` as visible and EXCLUDES
     `Pending` and `Rejected`. If any non-approved state were visible, the
     moderation gate is bypassable by construction.

  B) BOARD READ PATH (static, ACTIVATES when Worker A lands the board repo/handler):
     the public-board read query MUST hard-filter `moderation_status = 'approved'`
     in SQL, AND the board wire shape MUST NOT name any submitter-PII field. While
     the board files do not yet exist (Stage 0), this probe reports PENDING and
     does not fail; the moment they land it becomes a hard check (the GATE 1 exit
     condition).

  C) BEHAVIOR (gated behind --full, ACTIVATES when the tests land):
     runs `tests/board_moderation_gate.rs` + `tests/board_privacy_isolation.rs`
     against the real DB — the drift-detection leg so the static probes and live
     behavior cannot silently diverge. PENDING until Worker A writes them.

A green oracle (Probe A clean + B/C clean-or-pending) is the SCAFFOLD state now;
a green oracle with B+C ACTIVE is GATE 1.

Output: machine-parseable PASS / FAIL. Exit 0 on PASS, 1 on FAIL, 2 on
environment failure.

Lineage:
- FR-FBR-25a (approval-as-security-boundary) sibling — applied to board visibility
- DEC-FBR-02 (no-trackers brand promise) / Q24 (public-surface privacy) — no PII
- Contract C28 invariant 1 + C29 invariants 1 & 3
- Plan: docs/planning/plans/20260619T001105-public-feedback-board-moderation-gate.md
- Probandurgy Verification Oracle pattern (DEC-FBR-IMPL-03 canonical-Python + shims)
- Mirrors approval-gate-enforcement (the work-order trust-boundary oracle)
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path
from typing import List, Optional, Tuple

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[2]
MODERATION_RS = REPO_ROOT / "crates" / "feedbackmonk-core" / "src" / "moderation.rs"
# Worker A's board read path. Either file landing activates Probe B; the SQL
# filter is expected wherever the board read query is authored.
BOARD_HANDLER_RS = REPO_ROOT / "crates" / "feedbackmonk-api" / "src" / "handlers" / "board.rs"
FEEDBACK_REPO_RS = REPO_ROOT / "crates" / "feedbackmonk-repository" / "src" / "feedback.rs"
GATE_TEST_RS = REPO_ROOT / "crates" / "feedbackmonk-api" / "tests" / "board_moderation_gate.rs"
PRIVACY_TEST_RS = REPO_ROOT / "crates" / "feedbackmonk-api" / "tests" / "board_privacy_isolation.rs"

VISIBLE_VARIANTS = {"Approved"}
NON_VISIBLE_VARIANTS = {"Pending", "Rejected"}
VISIBILITY_PREDICATE = "is_publicly_visible"
# The board read query must hard-filter this in SQL (any quote style).
APPROVED_SQL_MARKERS = ["moderation_status = 'approved'", 'moderation_status = "approved"']
# Submitter-PII columns that must NEVER appear in a board wire/response shape.
PII_FIELDS = [
    "end_user_email",
    "end_user_name",
    "end_user_sub",
    "anon_token_hash",
    "external_metadata",
    "crash_event_id",
]


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p)


def _extract_fn_body(text: str, fn_sig: str) -> Optional[str]:
    """Return the brace-balanced body of the fn whose signature substring is
    `fn_sig`, or None if not found."""
    idx = text.find(fn_sig)
    if idx == -1:
        return None
    brace = text.find("{", idx)
    if brace == -1:
        return None
    depth = 0
    for i in range(brace, len(text)):
        c = text[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return text[brace : i + 1]
    return None


def probe_a() -> List[str]:
    """State-machine source proves only `Approved` is publicly visible."""
    offenders: List[str] = []
    if not MODERATION_RS.exists():
        return [f"{rel(MODERATION_RS)} does not exist — the moderation state machine is missing"]
    text = MODERATION_RS.read_text(encoding="utf-8")

    body = _extract_fn_body(text, f"fn {VISIBILITY_PREDICATE}")
    if body is None:
        offenders.append(
            f"{rel(MODERATION_RS)}: `fn {VISIBILITY_PREDICATE}` missing — the in-code "
            "visibility gate (C28/C29) is gone"
        )
        return offenders

    # Strip the doc-comment lines so prose mentioning Pending/Rejected does not
    # trip the variant scan; keep only code lines.
    code = "\n".join(
        ln for ln in body.splitlines() if not ln.lstrip().startswith("//")
    )

    for v in VISIBLE_VARIANTS:
        if not re.search(rf"\b{v}\b", code):
            offenders.append(
                f"{rel(MODERATION_RS)}: {VISIBILITY_PREDICATE} no longer classifies `{v}` as "
                "visible — the board would show nothing or rely on an unguarded path"
            )
    for v in NON_VISIBLE_VARIANTS:
        if re.search(rf"\b{v}\b", code):
            offenders.append(
                f"{rel(MODERATION_RS)}: {VISIBILITY_PREDICATE} references `{v}` — a "
                "non-approved state must NEVER be classified publicly visible "
                "(pending/rejected on the public board is the exposure this gate prevents)"
            )
    return offenders


def probe_b() -> Tuple[List[str], bool]:
    """Board read hard-filters approved-only in SQL + no PII in the wire shape.

    PENDING until Worker A lands the board read path. Returns (offenders, pending)."""
    board_files = [p for p in (BOARD_HANDLER_RS, FEEDBACK_REPO_RS) if p.exists()]
    # Probe B only engages once the board handler exists; the feedback repo
    # exists already (it predates this feature), so gate on the handler.
    if not BOARD_HANDLER_RS.exists():
        return [], True  # PENDING — Worker A has not implemented the board path yet.

    offenders: List[str] = []
    combined = "\n".join(p.read_text(encoding="utf-8") for p in board_files)

    # (1) approved-only SQL filter present somewhere in the board read path.
    if not any(marker in combined for marker in APPROVED_SQL_MARKERS):
        offenders.append(
            f"{rel(BOARD_HANDLER_RS)} / {rel(FEEDBACK_REPO_RS)}: the public-board read path "
            "does not hard-filter `moderation_status = 'approved'` in SQL. The approved-only "
            "invariant (C29 inv. 1) MUST live in the query, not be an optional/handler-side "
            "filter that a code path can skip."
        )

    # (2) no submitter PII in the board handler's response shape.
    handler_text = BOARD_HANDLER_RS.read_text(encoding="utf-8")
    for field in PII_FIELDS:
        if re.search(rf"\b{re.escape(field)}\b", handler_text):
            offenders.append(
                f"{rel(BOARD_HANDLER_RS)}: references submitter-PII field `{field}` — the "
                "board wire shape MUST NOT carry submitter identity (C29 inv. 3, Q24 class)."
            )
    return offenders, False


def probe_c(full: bool) -> Tuple[Optional[bool], str]:
    """Behavioral drift-detection (--full). PENDING until the tests land.

    Returns (passed, message). passed=None => skipped/pending/inconclusive."""
    if not full:
        return None, (
            "skipped (pass --full to run tests/board_moderation_gate.rs + "
            "board_privacy_isolation.rs)"
        )
    present = [t for t in (GATE_TEST_RS, PRIVACY_TEST_RS) if t.exists()]
    if not present:
        return None, (
            f"PENDING — neither {rel(GATE_TEST_RS)} nor {rel(PRIVACY_TEST_RS)} written yet "
            "(Workers A/B finalize the drift-detection leg)"
        )
    cmd = ["cargo", "test", "-p", "feedbackmonk-api"]
    if GATE_TEST_RS.exists():
        cmd += ["--test", "board_moderation_gate"]
    if PRIVACY_TEST_RS.exists():
        cmd += ["--test", "board_privacy_isolation"]
    try:
        proc = subprocess.run(
            cmd, cwd=str(REPO_ROOT), capture_output=True, text=True, timeout=600
        )
    except FileNotFoundError:
        return None, "cargo not found — Probe C inconclusive"
    except subprocess.TimeoutExpired:
        return False, "board gate/privacy tests timed out"
    if proc.returncode == 0:
        return True, "board_moderation_gate + board_privacy_isolation: all passed"
    tail = (proc.stdout + proc.stderr).strip().splitlines()[-8:]
    return False, "board gate/privacy tests failed:\n      " + "\n      ".join(tail)


def main() -> int:
    parser = argparse.ArgumentParser(description="public-board-moderation-gate oracle")
    parser.add_argument(
        "--full",
        action="store_true",
        help="also run the board gate + privacy integration tests (Probe C)",
    )
    args = parser.parse_args()

    a_offenders = probe_a()
    b_offenders, b_pending = probe_b()
    c_passed, c_message = probe_c(args.full)

    fails = (
        (1 if a_offenders else 0)
        + (1 if b_offenders else 0)
        + (1 if c_passed is False else 0)
    )

    if fails == 0:
        print("PASS public-board-moderation-gate")
        print(
            f"  Probe A (state machine: only Approved is publicly visible): "
            f"clean ({rel(MODERATION_RS)})"
        )
        if b_pending:
            print(
                "  Probe B (board read approved-only + no PII): PENDING — "
                f"{rel(BOARD_HANDLER_RS)} not yet implemented (Worker A; activates on landing)"
            )
        else:
            print(
                "  Probe B (board read approved-only + no PII): clean "
                f"({rel(BOARD_HANDLER_RS)})"
            )
        print(f"  Probe C (behavioral drift-detection): {c_message}")
        return 0

    print(f"FAIL public-board-moderation-gate ({fails} probe(s) failed)")
    if a_offenders:
        print()
        print("Probe A failures (visibility state machine):")
        for o in a_offenders:
            print(f"  {o}")
        print(
            "  Remediation: in feedbackmonk-core/src/moderation.rs keep "
            "`is_publicly_visible` returning true for EXACTLY {Approved}. Any change here "
            "needs a plan revision (C28/C29 are FROZEN)."
        )
    if b_offenders:
        print()
        print("Probe B failures (board read path):")
        for o in b_offenders:
            print(f"  {o}")
        print(
            "  Remediation: the board read query must hard-filter "
            "`moderation_status = 'approved'` in SQL, and the board response shape must omit "
            "every submitter-PII field (model on tests/me_feedback_isolation.rs)."
        )
    if c_passed is False:
        print()
        print("Probe C failure (behavioral drift):")
        print(f"  {c_message}")
        print(
            "  Remediation: cargo test -p feedbackmonk-api --test board_moderation_gate "
            "--test board_privacy_isolation"
        )
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
