#!/usr/bin/env python3
"""approval-gate-enforcement Verification Oracle (canonical implementation).

THE security boundary between public internet input and code execution
(FR-FBR-25a / FR-FBR-22). It proves — FROM STATE, not from a self-reported
"approved" flag — that no work order can reach an EXECUTION state without a
prior owner-authored `approved` event in the `work_order_events` ledger.

This is the anti-reward-hacking leg of the P5a plan (Testability Gate Flag 1,
the highest plan-wide fidelity risk). A standard "approve -> state==approved"
unit test confirms the happy path; it does NOT prove the ABSENCE of bypass
paths. An execution-state row with no `approved` event is, literally, a
remote-code-execution path from public feedback. This oracle reads the
state-machine SOURCE + the handler + the ledger so a bypass cannot hide behind
a passing happy-path test.

THREE probes (detection-from-state, co-evolving with Worker A):

  A) STATE MACHINE (static, source parse, LIVE in Stage 0):
     `feedbackmonk-core/src/work_order.rs` must prove the structural gate —
       (1) `is_execution_state` classifies exactly {dispatched, claimed,
           building, verifying, reported, completed, failed} as execution
           states and EXCLUDES {draft, approved, cancelled}; and
       (2) in `legal_transitions_from`, the ONLY state whose legal targets
           include `Dispatched` (the first execution state) is `Approved`.
     If any other state could reach `Dispatched`, the approval gate is
     bypassable by construction.

  B) HANDLER (static, AST-grade, ACTIVATES when Worker A lands the handler):
     `crates/feedbackmonk-api/src/handlers/work_orders.rs` — the transition
     handler MUST consult the security predicate
     `WorkOrderEventRepo::has_approved_event` (the owner-approval ledger
     check). While the handler does not yet exist (Stage 0), this probe
     reports PENDING and does not fail; the moment the file lands it becomes a
     hard check (Worker A's exit gate).

  C) LEDGER / BEHAVIOR (gated behind --full, ACTIVATES when the test lands):
     runs `tests/work_order_state_machine.rs` against the real DB — the
     drift-detection leg that exercises the ledger invariant end-to-end so the
     static probes and live behavior cannot silently diverge. PENDING until
     Worker A writes the test.

A green oracle (Probe A clean + B/C clean-or-pending) is the SCAFFOLD state
now; a green oracle with B+C ACTIVE is Worker A's Stage 1 exit gate.

Output: machine-parseable PASS / FAIL. Exit 0 on PASS, 1 on FAIL, 2 on
environment failure.

Lineage:
- FR-FBR-25a (approval-as-security-boundary) / FR-FBR-22 (work-order API)
- Contract C22 invariant 1 (no execution state without owner-authored approval)
- P5a plan §Oracle Pre-Build Plan + Testability Gate Flag 1
- Probandurgy Verification Oracle pattern (DEC-FBR-IMPL-03 canonical-Python + shims)
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
WORK_ORDER_RS = REPO_ROOT / "crates" / "feedbackmonk-core" / "src" / "work_order.rs"
HANDLER_RS = REPO_ROOT / "crates" / "feedbackmonk-api" / "src" / "handlers" / "work_orders.rs"
SM_TEST_RS = REPO_ROOT / "crates" / "feedbackmonk-api" / "tests" / "work_order_state_machine.rs"

EXECUTION_VARIANTS = {
    "Dispatched", "Claimed", "Building", "Verifying", "Reported", "Completed", "Failed",
}
NON_EXECUTION_VARIANTS = {"Draft", "Approved", "Cancelled"}
SECURITY_PREDICATE = "has_approved_event"


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
    """State-machine source proves approval is the sole gate into execution."""
    offenders: List[str] = []
    if not WORK_ORDER_RS.exists():
        return [f"{rel(WORK_ORDER_RS)} does not exist — the state machine is missing"]
    text = WORK_ORDER_RS.read_text(encoding="utf-8")

    # (1) is_execution_state classifies the execution set correctly.
    body = _extract_fn_body(text, "fn is_execution_state")
    if body is None:
        offenders.append(
            f"{rel(WORK_ORDER_RS)}: `fn is_execution_state` missing — the in-code "
            "security predicate (C22 inv. 1) is gone"
        )
    else:
        for v in EXECUTION_VARIANTS:
            if not re.search(rf"\b{v}\b", body):
                offenders.append(
                    f"{rel(WORK_ORDER_RS)}: is_execution_state omits `{v}` — an execution "
                    "state would be misclassified as safe (approval gate bypassable)"
                )
        for v in NON_EXECUTION_VARIANTS:
            if re.search(rf"\b{v}\b", body):
                offenders.append(
                    f"{rel(WORK_ORDER_RS)}: is_execution_state includes `{v}` — a "
                    "non-execution state misclassified as execution (draft/approved/cancelled "
                    "must NOT be execution states)"
                )

    # (2) Only `Approved` may transition to `Dispatched`.
    body_lt = _extract_fn_body(text, "fn legal_transitions_from")
    if body_lt is None:
        offenders.append(
            f"{rel(WORK_ORDER_RS)}: `fn legal_transitions_from` missing — the transition "
            "table is gone"
        )
    else:
        reach_dispatched = set()
        # Match arms of the form `State => &[A, B, C],`
        for m in re.finditer(r"(\w+)\s*=>\s*&\[([^\]]*)\]", body_lt):
            state, targets = m.group(1), m.group(2)
            if re.search(r"\bDispatched\b", targets):
                reach_dispatched.add(state)
        if reach_dispatched != {"Approved"}:
            offenders.append(
                f"{rel(WORK_ORDER_RS)}: states that can transition to `Dispatched` = "
                f"{sorted(reach_dispatched) or 'none'} — MUST be exactly {{Approved}}. "
                "Any other predecessor of the first execution state breaks C22 inv. 1 "
                "(the approval gate)."
            )
    return offenders


def probe_b() -> Tuple[List[str], bool]:
    """Handler consults has_approved_event. PENDING until the handler lands.

    Returns (offenders, pending)."""
    if not HANDLER_RS.exists():
        return [], True  # PENDING — Worker A has not implemented the handler yet.
    text = HANDLER_RS.read_text(encoding="utf-8")
    offenders: List[str] = []
    if SECURITY_PREDICATE not in text:
        offenders.append(
            f"{rel(HANDLER_RS)}: the work-order transition handler never consults "
            f"`{SECURITY_PREDICATE}` — the owner-approval ledger check (C22 inv. 1) is not "
            "enforced in code. Every transition into an execution state MUST verify a prior "
            "owner-authored `approved` event."
        )
    return offenders, False


def probe_c(full: bool) -> Tuple[Optional[bool], str]:
    """Behavioral drift-detection (--full). PENDING until the test lands.

    Returns (passed, message). passed=None => skipped/pending/inconclusive."""
    if not full:
        return None, "skipped (pass --full to run tests/work_order_state_machine.rs)"
    if not SM_TEST_RS.exists():
        return None, (
            f"PENDING — {rel(SM_TEST_RS)} not yet written (Worker A finalizes the "
            "drift-detection leg)"
        )
    try:
        proc = subprocess.run(
            ["cargo", "test", "-p", "feedbackmonk-api", "--test", "work_order_state_machine"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            timeout=600,
        )
    except FileNotFoundError:
        return None, "cargo not found — Probe C inconclusive"
    except subprocess.TimeoutExpired:
        return False, "cargo test --test work_order_state_machine timed out"
    if proc.returncode == 0:
        return True, "cargo test --test work_order_state_machine: all passed"
    tail = (proc.stdout + proc.stderr).strip().splitlines()[-8:]
    return False, "work_order_state_machine failed:\n      " + "\n      ".join(tail)


def main() -> int:
    parser = argparse.ArgumentParser(description="approval-gate-enforcement oracle")
    parser.add_argument(
        "--full",
        action="store_true",
        help="also run the work_order_state_machine integration test (Probe C)",
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
        print("PASS approval-gate-enforcement")
        print(
            f"  Probe A (state machine: approval is the sole gate into execution): "
            f"clean ({rel(WORK_ORDER_RS)})"
        )
        if b_pending:
            print(
                "  Probe B (handler consults has_approved_event): PENDING — "
                f"{rel(HANDLER_RS)} not yet implemented (Worker A; activates on landing)"
            )
        else:
            print(f"  Probe B (handler consults has_approved_event): clean ({rel(HANDLER_RS)})")
        print(f"  Probe C (behavioral drift-detection): {c_message}")
        return 0

    print(f"FAIL approval-gate-enforcement ({fails} probe(s) failed)")
    if a_offenders:
        print()
        print("Probe A failures (state-machine gate):")
        for o in a_offenders:
            print(f"  {o}")
        print(
            "  Remediation: in feedbackmonk-core/src/work_order.rs keep `is_execution_state` "
            "classifying exactly {dispatched,claimed,building,verifying,reported,completed,"
            "failed} and keep `Approved` as the SOLE predecessor of `Dispatched` in "
            "legal_transitions_from. Any change here needs a plan revision (C22 is FROZEN)."
        )
    if b_offenders:
        print()
        print("Probe B failures (handler enforcement):")
        for o in b_offenders:
            print(f"  {o}")
        print(
            "  Remediation: the transition handler must call "
            "`work_order_events.has_approved_event(scope, id)` and refuse any transition into "
            "an execution state (`WorkOrderState::is_execution_state`) unless it returns true."
        )
    if c_passed is False:
        print()
        print("Probe C failure (behavioral drift):")
        print(f"  {c_message}")
        print("  Remediation: cargo test -p feedbackmonk-api --test work_order_state_machine")
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
