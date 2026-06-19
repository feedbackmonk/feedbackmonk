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

  B) BOARD READ PATH (static, ACTIVE — engaged when Worker A landed board.rs):
     the BOARD READ SCOPE = the `board.rs` handler + every repository fn named
     `*board*` that queries `FROM feedback` (so an unrelated query cannot
     false-satisfy the marker, a PII column in the repo query is caught, and
     board-SETTINGS fns querying `projects` are excluded). Within it:
       - the handler MUST invoke the approved-only board reads (not an unfiltered
         feedback read) — mirrors approval-gate-enforcement's handler binding;
       - EACH board read fn MUST hard-filter `moderation_status = 'approved'` as a
         SQL literal (per-fn, so a regression on one read isn't masked by another;
         a bound param is rejected — it can't be statically proven always-approved);
       - the scope MUST NOT name any submitter-PII field, MUST NOT reference a
         non-approved moderation literal (`pending`/`rejected`), and MUST NOT
         surface `feedback_replies` (internal reply content).
     Before board.rs exists this probe reports PENDING (does not fail).
     NOTE — C29 inv. 2 (board-disabled→404) is a distinct leak vector (approved
     rows from a non-opted-in project), OUTSIDE this oracle's framed question
     (non-approved OR PII); it is covered by the behavioral Probe C.

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
# Worker A's board read path. board.rs landing activates Probe B (per the C29
# announcement protocol). The approved-only SQL filter + the board wire shape
# are verified within the BOARD READ SCOPE (see _board_scope_texts): the board
# handler plus any *repository* fn whose name mentions the board — so detection
# works whether `list_public_board`/`get_public_board_item` live in `feedback.rs`
# or in a dedicated `feedbackmonk-repository/src/board.rs`. DEC-FBR-03 forbids
# raw SQL outside the repository layer, so the query lives in a repo fn.
BOARD_HANDLER_RS = REPO_ROOT / "crates" / "feedbackmonk-api" / "src" / "handlers" / "board.rs"
REPOSITORY_SRC = REPO_ROOT / "crates" / "feedbackmonk-repository" / "src"
GATE_TEST_RS = REPO_ROOT / "crates" / "feedbackmonk-api" / "tests" / "board_moderation_gate.rs"
PRIVACY_TEST_RS = REPO_ROOT / "crates" / "feedbackmonk-api" / "tests" / "board_privacy_isolation.rs"

VISIBLE_VARIANTS = {"Approved"}
NON_VISIBLE_VARIANTS = {"Pending", "Rejected"}
VISIBILITY_PREDICATE = "is_publicly_visible"
# The board read query MUST hard-filter on the literal `'approved'` (C29 inv. 1).
# Tolerant of: table-alias/qualifier prefix (substring match), whitespace around
# `=`, single/double quotes, an optional `::text`/`::cast`, and `IN ('approved')`
# as an equivalent. A BOUND PARAM (`moderation_status = $N`) deliberately does
# NOT match — a bound value cannot be statically proven to always be 'approved',
# and the anti-reward-hacking invariant wants the hard literal in the query.
APPROVED_SQL_RE = re.compile(
    r"moderation_status\s*(?:::\s*\w+)?\s*(?:=|\bIN\b\s*\()\s*['\"]approved['\"]",
    re.IGNORECASE,
)
# A board read must NEVER reference a non-approved moderation-status literal —
# catches `!= 'rejected'` / `IN ('approved','pending')`-style mistakes that would
# leak pending/rejected rows. These are moderation values (not triage statuses,
# which are submitted/triaged/in-progress/shipped/wontfix/duplicate), so their
# appearance in board scope is a filter bug.
NON_APPROVED_LITERAL_RE = re.compile(r"['\"](?:pending|rejected)['\"]", re.IGNORECASE)
# Submitter-PII columns that must NEVER appear anywhere in the board read scope
# (handler wire shape OR the repo query SELECT list). DEC-FBR-02 / Q24 class.
PII_FIELDS = [
    "end_user_email",
    "end_user_name",
    "end_user_sub",
    "anon_token_hash",
    "external_metadata",
    "crash_event_id",
]
# The board must not surface internal/admin reply content (C29: "internal/admin
# reply content"). `feedback_replies` carries `visibility IN ('public','internal')`
# rows; the board wire shape (C29) has no reply field, so any reference to the
# replies table inside the board read scope is a leak vector.
REPLY_TABLE_TOKEN = "feedback_replies"


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


def _iter_fn_bodies(text: str, name_re: str):
    """Yield (fn_name, body) for every fn whose name matches `name_re` AND has a
    body. Skips signature-only trait declarations (`fn foo(...) -> T;`) — the
    `;` terminator appears before any `{`. Handles a name appearing BOTH as a
    trait decl and an impl (the real `feedback.rs` shape): the decl is skipped,
    the impl body is returned. Handles multiple impls of the same name."""
    for m in re.finditer(rf"fn\s+({name_re})\s*[(<]", text, re.IGNORECASE):
        brace = text.find("{", m.end())
        semi = text.find(";", m.end())
        if brace == -1:
            continue
        if semi != -1 and semi < brace:
            continue  # signature-only declaration — no body to inspect
        # brace-balanced body extraction from the opening brace.
        depth = 0
        body = None
        for i in range(brace, len(text)):
            c = text[i]
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
                if depth == 0:
                    body = text[brace : i + 1]
                    break
        if body:
            yield m.group(1), body


# A board READ fn touches the `feedback` table (the rows the board exposes).
# `\b` after `feedback` excludes `feedback_moderation_events` / `feedback_replies`.
BOARD_READS_FEEDBACK_RE = re.compile(r"FROM\s+feedback\b", re.IGNORECASE)


def _board_read_fns() -> List[Tuple[str, str, str]]:
    """Repository fns that are the public-board READ: name mentions `board` AND
    the body queries the `feedback` table. Returns (relpath, fn_name, body).

    Name-keying on `board` excludes the admin moderation queue
    (`list_pending_for_admin`, PII-allowed behind `AdminSession`); the
    `FROM feedback` requirement further excludes board-SETTINGS fns
    (`get_board_settings`/`update_board_settings`, which query `projects`, not
    `feedback`) so the read invariants are checked only against actual reads."""
    out: List[Tuple[str, str, str]] = []
    if not REPOSITORY_SRC.is_dir():
        return out
    for src in sorted(REPOSITORY_SRC.glob("*.rs")):
        text = src.read_text(encoding="utf-8")
        for name, body in _iter_fn_bodies(text, r"\w*board\w*"):
            if BOARD_READS_FEEDBACK_RE.search(body):
                out.append((rel(src), name, body))
    return out


def probe_b() -> Tuple[List[str], bool]:
    """Board read hard-filters approved-only in SQL (PER read fn) + leaks no PII
    / reply content.

    Detection is BOARD-READ-SCOPED — the `board.rs` handler plus every repository
    fn that is named `*board*` AND queries `FROM feedback` — so (a) an unrelated
    query elsewhere in `feedback.rs` cannot false-satisfy the approved marker,
    (b) a PII column SELECTed in the repo query (not just the handler wire shape)
    is caught, and (c) the approved-only literal is required in EACH read fn (a
    regression that drops the filter from just one of the two reads is caught,
    not masked by the other).

    PENDING until Worker A lands `board.rs`. Returns (offenders, pending)."""
    # Probe B engages once the board handler exists (the C29 announcement
    # trigger). The board read fns only exist post-A.
    if not BOARD_HANDLER_RS.exists():
        return [], True  # PENDING — Worker A has not implemented the board path yet.

    offenders: List[str] = []
    handler_text = BOARD_HANDLER_RS.read_text(encoding="utf-8")
    read_fns = _board_read_fns()

    # Scope units for the leak scans: the handler (wire shape) + each read fn.
    scope: List[Tuple[str, str]] = [(rel(BOARD_HANDLER_RS), handler_text)]
    scope += [(f"{relpath}::{name}", body) for relpath, name, body in read_fns]

    # (0) sanity — at least one board read fn must be discoverable, else the
    # approved-only invariant cannot be scope-verified at all.
    if not read_fns:
        offenders.append(
            f"{rel(BOARD_HANDLER_RS)} exists but no board READ fn was found (a repository fn "
            "named `*board*` that queries `FROM feedback`) — the approved-only filter (C29 inv. 1) "
            "cannot be scope-verified. Name the board reads `list_public_board`/"
            "`get_public_board_item` per C29 and keep the SQL in the repository layer (DEC-FBR-03)."
        )

    # (0b) the handler must READ feedback only through the approved-only board
    # reads — a handler rewired to call an unfiltered feedback read (e.g.
    # `list_for_end_user`) would bypass the SQL gate while every read fn above
    # still filters correctly. Mirrors approval-gate-enforcement's handler-binding
    # check (handler must consult has_approved_event). Defense-in-depth behind the
    # behavioral Probe C, but catchable WITHOUT --full.
    if read_fns and not any(name in handler_text for _, name, _ in read_fns):
        offenders.append(
            f"{rel(BOARD_HANDLER_RS)}: the board handler does not invoke any approved-only "
            f"board read fn ({', '.join(sorted({n for _, n, _ in read_fns}))}) — it may read "
            "feedback through an unfiltered path that bypasses the SQL moderation gate (C29 inv. 1)."
        )

    # (1) approved-only SQL literal required in EACH board read fn.
    for relpath, name, body in read_fns:
        if not APPROVED_SQL_RE.search(body):
            offenders.append(
                f"{relpath}::{name}: board read fn does not hard-filter "
                "`moderation_status = 'approved'` as a SQL literal. The approved-only invariant "
                "(C29 inv. 1) MUST live in this query as a literal — not a bound param a code "
                "path could vary, not a handler-side filter. A non-approved row would be reachable "
                "through this read."
            )

    # (1b) no non-approved moderation literal anywhere in board scope.
    for label, body in scope:
        mm = NON_APPROVED_LITERAL_RE.search(body)
        if mm:
            offenders.append(
                f"{label}: references non-approved moderation literal `{mm.group(0)}` — a board "
                "read must filter EXACTLY `= 'approved'`; `!= 'rejected'` / "
                "`IN ('approved','pending')`-style filters leak pending rows (C29 inv. 1)."
            )

    # (2) no submitter PII anywhere in board scope (handler wire shape OR repo
    # query SELECT list).
    for label, body in scope:
        for field in PII_FIELDS:
            if re.search(rf"\b{re.escape(field)}\b", body):
                offenders.append(
                    f"{label}: references submitter-PII field `{field}` — the board read scope "
                    "(wire shape AND query) MUST NOT carry submitter identity (C29 inv. 3, Q24 "
                    "class). Model on feedback.rs::list_for_end_user (selects exactly "
                    "short_code, kind, status, body, accepted_at)."
                )

    # (3) no internal/admin reply content surfaced by the board.
    for label, body in scope:
        if REPLY_TABLE_TOKEN in body:
            offenders.append(
                f"{label}: references `{REPLY_TABLE_TOKEN}` — the board MUST NOT surface "
                "internal/admin reply content (C29 wire shape has no reply field; "
                "`feedback_replies` carries internal-visibility rows)."
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
