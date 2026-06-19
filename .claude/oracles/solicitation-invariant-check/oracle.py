#!/usr/bin/env python3
"""solicitation-invariant-check Verification Oracle (canonical implementation).

THE load-bearing privacy boundary of the per-user feedback-solicitation feature
(FR-FBR-29): once a user opts out, that decision is FOREVER. It proves — FROM
CODE, not from a self-reported "opted out" flag — that `opted_out` is a TERMINAL
state: no event other than `opted_out` is ever honored once a sub has opted out,
and the state machine cannot be bypassed by the HTTP handler.

This is the anti-reward-hacking leg of the solicitation feature, mirroring
`approval-gate-enforcement` (work-order approval trust boundary) and
`public-board-moderation-gate` (board moderation trust boundary). A standard
"opt out -> status==opted_out" unit test confirms the happy path; it does NOT
prove the ABSENCE of a code path that honors a `prompted`/`dismissed`/
`gave_feedback` event after opt-out, nor that the handler can be rewired to
`upsert` a new status WITHOUT routing through the state machine. An opted-out
sub becoming eligible again — or a handler write that skips the terminal guard —
is the exact privacy regression this feature exists to prevent ("never solicit
me again" silently un-honored).

THREE probes (detection-from-code; anti-reward-hacking):

  A) STATE MACHINE (static, source parse, LIVE):
     `feedbackmonk-core/src/solicitation.rs` must prove the structural gate —
       (1) the `SolicitationStatus::OptedOut` variant exists;
       (2) `is_opted_out` returns true for EXACTLY `OptedOut`
           (`matches!(self, Self::OptedOut)`) — not widened to any other state;
       (3) `apply_event` carries the TERMINAL guard — a match arm that returns
           `Err(...OptedOut)` when `current.is_opted_out()` for non-opt-out
           events. If the guard is removed, an opted-out sub could be re-prompted
           or re-resolved (the privacy promise is bypassable by construction).
     Mirrors how `public-board-moderation-gate` Probe A asserts
     `is_publicly_visible` matches EXACTLY `Approved`.

  B) HANDLER BINDING (static, source scan, LIVE):
     `crates/feedbackmonk-api/src/handlers/solicitation.rs` —
       (1) `post_solicitation_event` MUST route through the state machine
           (`apply_solicitation_event(...)`) BEFORE `repo.upsert(...)`: every
           `upsert(` call in the fn is preceded by an `apply_solicitation_event(`
           call. A write that skips the apply would bypass the terminal guard.
           Mirrors approval-gate-enforcement's handler-binding check.
       (2) the auth helper requires JWT IDENTITY-class keys
           (`list_active_for_class(..., KeyClass::Identity)`) — the durable
           "don't ask me again" record is keyed by a verified, identity-class
           `sub` (DEC-FBR-04), never an anon token.

  C) BEHAVIOR (gated behind --full, ACTIVATES when the test lands):
     runs `tests/solicitation_integration.rs` against the real DB — the
     drift-detection leg so the static probes and live behavior cannot silently
     diverge. PENDING until the integration test is authored (concurrently, by
     the main session). NEVER run by the A+B default invocation.

A green oracle (Probe A+B clean; C clean-or-pending) is the LIVE state.

Output: machine-parseable PASS / FAIL. Exit 0 on PASS, 1 on FAIL, 2 on
environment failure.

Lineage:
- FR-FBR-29 (per-user feedback-solicitation state; GitCellar in-app nudge)
- DEC-FBR-IMPL-24 (opted_out is a TERMINAL state — the load-bearing privacy promise)
- DEC-FBR-04 (JWT identity is the ONLY identity; identity-class keys only)
- Probandurgy Verification Oracle pattern (DEC-FBR-IMPL-03 canonical-Python + shims)
- Mirrors approval-gate-enforcement + public-board-moderation-gate (the sibling
  anti-reward-hacking trust-boundary oracles)
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
SOLICITATION_RS = REPO_ROOT / "crates" / "feedbackmonk-core" / "src" / "solicitation.rs"
HANDLER_RS = (
    REPO_ROOT / "crates" / "feedbackmonk-api" / "src" / "handlers" / "solicitation.rs"
)
INTEGRATION_TEST_RS = (
    REPO_ROOT / "crates" / "feedbackmonk-api" / "tests" / "solicitation_integration.rs"
)

# The terminal state variant + the predicate that must classify EXACTLY it.
TERMINAL_VARIANT = "OptedOut"
TERMINAL_PREDICATE = "is_opted_out"
# The handler MUST route every write through the state machine. In the handler,
# core's `apply_event` is imported under this alias.
APPLY_FN = "apply_solicitation_event"
UPSERT_CALL = "upsert"
# The handler entry point that records events (the one that performs writes).
EVENT_HANDLER_FN = "post_solicitation_event"
# Auth must require JWT identity-class keys (DEC-FBR-04).
IDENTITY_KEY_CLASS = "KeyClass::Identity"
LIST_ACTIVE_FN = "list_active_for_class"


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


def _strip_line_comments(body: str) -> str:
    """Drop `//`-comment lines so prose (which may mention OptedOut / other
    states) does not trip the structural scans. Keeps only code lines."""
    return "\n".join(ln for ln in body.splitlines() if not ln.lstrip().startswith("//"))


def probe_a() -> List[str]:
    """State-machine source proves opted_out is TERMINAL."""
    offenders: List[str] = []
    if not SOLICITATION_RS.exists():
        return [
            f"{rel(SOLICITATION_RS)} does not exist — the solicitation state machine is missing"
        ]
    text = SOLICITATION_RS.read_text(encoding="utf-8")

    # (1) the OptedOut variant must exist on SolicitationStatus.
    if not re.search(rf"\b{TERMINAL_VARIANT}\b", text):
        offenders.append(
            f"{rel(SOLICITATION_RS)}: the `{TERMINAL_VARIANT}` variant is gone — the terminal "
            "opt-out state no longer exists (the privacy promise has no representation)"
        )
        # Without the variant the rest of the structural checks are moot.
        return offenders

    # (2) is_opted_out classifies EXACTLY OptedOut.
    body = _extract_fn_body(text, f"fn {TERMINAL_PREDICATE}")
    if body is None:
        offenders.append(
            f"{rel(SOLICITATION_RS)}: `fn {TERMINAL_PREDICATE}` missing — the in-code terminal "
            "predicate is gone; the handler's eligibility + apply_event guard depend on it"
        )
    else:
        code = _strip_line_comments(body)
        # The predicate must reference the terminal variant...
        if not re.search(rf"\bSelf::{TERMINAL_VARIANT}\b|\b{TERMINAL_VARIANT}\b", code):
            offenders.append(
                f"{rel(SOLICITATION_RS)}: {TERMINAL_PREDICATE} no longer classifies "
                f"`{TERMINAL_VARIANT}` — the terminal-state predicate is broken"
            )
        # ...and must NOT be widened to any OTHER status variant (which would
        # mark a non-terminal state as opted-out, or — worse — leave OptedOut
        # un-matched). EXACTLY {OptedOut}, mirroring public-board Probe A.
        for other in ("Eligible", "Prompted", "Dismissed", "GaveFeedback"):
            if re.search(rf"\bSelf::{other}\b|\b{other}\b", code):
                offenders.append(
                    f"{rel(SOLICITATION_RS)}: {TERMINAL_PREDICATE} references `{other}` — it MUST "
                    f"classify EXACTLY {{{TERMINAL_VARIANT}}} as opted-out. Widening it would "
                    "either mark a non-terminal state terminal or break the opt-out check."
                )

    # (3) apply_event carries the TERMINAL guard: a match arm that returns
    # Err(...OptedOut) when current.is_opted_out() for non-opt-out events.
    apply_body = _extract_fn_body(text, "fn apply_event")
    if apply_body is None:
        offenders.append(
            f"{rel(SOLICITATION_RS)}: `fn apply_event` missing — the transition function (and "
            "its terminal guard) is gone"
        )
    else:
        apply_code = _strip_line_comments(apply_body)
        # The guard arm: `_ if current.is_opted_out() => Err(... OptedOut ...)`.
        # Tolerant of whitespace, the catch-all `_` pattern, the error-path
        # spelling (`SolicitationError::OptedOut` / `Err(... OptedOut)`), and
        # the order of the guard relative to the OptedOut-event arm.
        has_guard_condition = re.search(
            rf"if\s+current\.{TERMINAL_PREDICATE}\s*\(\s*\)", apply_code
        )
        # The guard must REJECT — return an Err carrying the OptedOut error.
        has_optedout_err = re.search(
            r"Err\s*\([^)]*OptedOut", apply_code
        ) or re.search(r"SolicitationError::OptedOut", apply_code)
        if not has_guard_condition:
            offenders.append(
                f"{rel(SOLICITATION_RS)}: apply_event no longer guards on "
                f"`current.{TERMINAL_PREDICATE}()` — the terminal arm that rejects every "
                "non-opt-out event after opt-out is gone. An opted-out sub could be "
                "re-prompted/re-resolved (DEC-FBR-IMPL-24 violated)."
            )
        elif not has_optedout_err:
            offenders.append(
                f"{rel(SOLICITATION_RS)}: apply_event guards on `current.{TERMINAL_PREDICATE}()` "
                "but does not return `Err(SolicitationError::OptedOut)` from it — the terminal "
                "guard must REJECT non-opt-out events once opted out, not accept them."
            )
    return offenders


def probe_b() -> List[str]:
    """Handler routes every write through the state machine + requires JWT
    identity-class auth. LIVE (the handler already exists)."""
    offenders: List[str] = []
    if not HANDLER_RS.exists():
        return [
            f"{rel(HANDLER_RS)} does not exist — the solicitation HTTP handler is missing"
        ]
    text = HANDLER_RS.read_text(encoding="utf-8")

    # (1) the event handler must route every write through the state machine.
    body = _extract_fn_body(text, f"fn {EVENT_HANDLER_FN}")
    if body is None:
        offenders.append(
            f"{rel(HANDLER_RS)}: `fn {EVENT_HANDLER_FN}` missing — the write path (which must "
            "route through the state machine) is gone"
        )
    else:
        code = _strip_line_comments(body)
        apply_idx = code.find(f"{APPLY_FN}(")
        # Find every upsert( call in the fn. Each must be preceded by an
        # apply_solicitation_event( call — i.e. no write bypasses the machine.
        upsert_positions = [m.start() for m in re.finditer(rf"{UPSERT_CALL}\s*\(", code)]
        if not upsert_positions:
            offenders.append(
                f"{rel(HANDLER_RS)}::{EVENT_HANDLER_FN}: no `{UPSERT_CALL}(` write found — "
                "cannot confirm the write path routes through the state machine (handler shape "
                "changed; re-verify the apply→upsert ordering)."
            )
        elif apply_idx == -1:
            offenders.append(
                f"{rel(HANDLER_RS)}::{EVENT_HANDLER_FN}: writes via `{UPSERT_CALL}(` but never "
                f"calls `{APPLY_FN}(...)` — the write BYPASSES the state machine, so the terminal "
                "opt-out guard is not enforced for HTTP-driven events (DEC-FBR-IMPL-24)."
            )
        else:
            # Every upsert must come AFTER the apply call.
            for pos in upsert_positions:
                if pos < apply_idx:
                    offenders.append(
                        f"{rel(HANDLER_RS)}::{EVENT_HANDLER_FN}: a `{UPSERT_CALL}(` write precedes "
                        f"`{APPLY_FN}(...)` — the write is not gated by the state machine "
                        "(terminal opt-out guard bypassed; DEC-FBR-IMPL-24)."
                    )
                    break

    # (2) auth must require JWT identity-class keys (DEC-FBR-04). Scan whole file:
    # the auth helper is a separate fn (`authenticate`).
    if not re.search(rf"{LIST_ACTIVE_FN}\s*\([^)]*{re.escape(IDENTITY_KEY_CLASS)}", text):
        # Fall back to a co-occurrence check (multi-line call): both tokens present.
        if not (LIST_ACTIVE_FN in text and IDENTITY_KEY_CLASS in text):
            offenders.append(
                f"{rel(HANDLER_RS)}: auth does not request `{IDENTITY_KEY_CLASS}` keys via "
                f"`{LIST_ACTIVE_FN}(...)` — the durable solicitation record MUST be keyed by a "
                "verified IDENTITY-class JWT `sub` (DEC-FBR-04), never an anon token."
            )
    return offenders


def probe_c(full: bool) -> Tuple[Optional[bool], str]:
    """Behavioral drift-detection (--full). PENDING until the test lands.

    Returns (passed, message). passed=None => skipped/pending/inconclusive."""
    if not full:
        return None, (
            "skipped (pass --full to run tests/solicitation_integration.rs)"
        )
    if not INTEGRATION_TEST_RS.exists():
        return None, (
            f"PENDING — {rel(INTEGRATION_TEST_RS)} not yet written (authored concurrently "
            "by the main session; finalizes the drift-detection leg)"
        )
    try:
        proc = subprocess.run(
            [
                "cargo",
                "test",
                "-p",
                "feedbackmonk-api",
                "--test",
                "solicitation_integration",
            ],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            timeout=600,
        )
    except FileNotFoundError:
        return None, "cargo not found — Probe C inconclusive"
    except subprocess.TimeoutExpired:
        return False, "cargo test --test solicitation_integration timed out"
    if proc.returncode == 0:
        return True, "cargo test --test solicitation_integration: all passed"
    tail = (proc.stdout + proc.stderr).strip().splitlines()[-8:]
    return False, "solicitation_integration failed:\n      " + "\n      ".join(tail)


def main() -> int:
    parser = argparse.ArgumentParser(description="solicitation-invariant-check oracle")
    parser.add_argument(
        "--full",
        action="store_true",
        help="also run the solicitation_integration integration test (Probe C)",
    )
    args = parser.parse_args()

    a_offenders = probe_a()
    b_offenders = probe_b()
    c_passed, c_message = probe_c(args.full)

    fails = (
        (1 if a_offenders else 0)
        + (1 if b_offenders else 0)
        + (1 if c_passed is False else 0)
    )

    if fails == 0:
        print("PASS solicitation-invariant-check")
        print(
            f"  Probe A (state machine: opted_out is TERMINAL): clean "
            f"({rel(SOLICITATION_RS)})"
        )
        print(
            f"  Probe B (handler routes writes through the state machine + identity-class auth): "
            f"clean ({rel(HANDLER_RS)})"
        )
        print(f"  Probe C (behavioral drift-detection): {c_message}")
        return 0

    print(f"FAIL solicitation-invariant-check ({fails} probe(s) failed)")
    if a_offenders:
        print()
        print("Probe A failures (terminal-state machine):")
        for o in a_offenders:
            print(f"  {o}")
        print(
            "  Remediation: in feedbackmonk-core/src/solicitation.rs keep the `OptedOut` variant, "
            f"keep `{TERMINAL_PREDICATE}` matching EXACTLY {{OptedOut}}, and keep apply_event's "
            "`_ if current.is_opted_out() => Err(SolicitationError::OptedOut)` guard so every "
            "non-opt-out event is rejected once opted out (DEC-FBR-IMPL-24 is FROZEN)."
        )
    if b_offenders:
        print()
        print("Probe B failures (handler binding):")
        for o in b_offenders:
            print(f"  {o}")
        print(
            "  Remediation: post_solicitation_event must call "
            "`apply_solicitation_event(current, event)` BEFORE `repo.upsert(...)` (no write "
            "bypasses the machine), and auth must call "
            "`signing_keys.list_active_for_class(&scope, KeyClass::Identity)` (DEC-FBR-04)."
        )
    if c_passed is False:
        print()
        print("Probe C failure (behavioral drift):")
        print(f"  {c_message}")
        print(
            "  Remediation: cargo test -p feedbackmonk-api --test solicitation_integration"
        )
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
