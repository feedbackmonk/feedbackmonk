#!/usr/bin/env python3
"""feedback-erasure-completeness Verification Oracle (canonical implementation).

The anti-reward-hacking leg for Phase A A1 — the P0 right-to-erasure endpoint
`DELETE /api/v1/projects/{project}/me/feedback/{id}` (D-A1). It proves — FROM
CODE, not a self-reported flag — FOUR load-bearing properties of the erasure
path, so a future refactor cannot silently leave residual PII (the failure a
happy-path "row is gone" test does NOT catch):

  A) ERASURE ORDER (static, me_feedback handler): the delete handler PURGES the
     attachment OBJECT BYTES (ObjectStore::delete over the feedback's storage
     keys) BEFORE it deletes the feedback ROW. DB FK cascade removes the
     attachment metadata ROWS but NOT the object-store bytes — so if the row
     delete ran first (or the byte purge were dropped), the screenshots/logs
     would survive in the bucket after "erasure". The order also matters for
     retry-safety: byte-purge-then-row-delete means a mid-failure leaves DB rows
     (retryable) rather than orphaned bytes.

  B) CASCADE COMPLETENESS (static, migrations): every table that REFERENCES
     feedback(id) must be `ON DELETE CASCADE` (child rows removed) or
     `ON DELETE SET NULL` (link nulled) — never RESTRICT / NO ACTION, which would
     either BLOCK the erasure (DoS) or ORPHAN child rows carrying the feedback's
     data. A new table with a plain `REFERENCES feedback(id)` (default NO ACTION)
     is exactly the regression this catches.

  C) SUB-SCOPED DELETE (static, repository): the erasure DELETE
     (`delete_for_end_user`) scopes its WHERE by `end_user_sub` AND `tenant_id`
     AND `project_id` — so a caller can only erase their OWN feedback, never
     another user's / another tenant's row (DEC-FBR-04 / DEC-FBR-03). A DELETE
     that dropped `end_user_sub` from the WHERE would be a cross-user erasure
     hole.

  D) DERIVED-TEXT COMPLETENESS (static, repository; scrutiny P0-1): the erasure
     ALSO UPDATE-scrubs the P5 analysis corpus (feedback_clusters /
     recommendations / work_orders / analysis_sweeps) for the erased feedback's
     cluster. Those tables reference the CLUSTER, not feedback, so FK cascade
     never reaches them; without the scrub the submitter's verbatim/paraphrased
     text survives in admin-readable prose.

  --full) BEHAVIOR: runs tests/me_feedback_delete.rs against the real DB
     (drift-detection leg — byte purge + cascade + cross-sub survival), so the
     static probes and live behaviour cannot silently diverge.

Output: machine-parseable PASS / FAIL. Exit 0 on PASS, 1 on FAIL, 2 on
environment failure.

Lineage:
- Phase A A1 (P0 right-to-erasure; D-A1 hard-delete + cascade + object-byte purge)
- DEC-FBR-04 (JWT sub is the sole end-user identity) / DEC-FBR-03 (tenant+project scope)
- DEC-FBR-02 / Q24 (no residual submitter PII)
- Plan: docs/planning/plans/20260701T161200-feedbackmonk-phase-a-gitcellar-contract.md (S0.2 / Stream 1)
- Testability Gate Flag (A1, composite 16/25): a "row gone" test cannot prove the
  ABSENCE of surviving object bytes or an un-cascaded child table.
- Mirrors translation-egress-q24-isolation / public-board-moderation-gate (detection-from-code).
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
CRATES_DIR = REPO_ROOT / "crates"
ME_FEEDBACK_RS = CRATES_DIR / "feedbackmonk-api" / "src" / "handlers" / "me_feedback.rs"
REPO_FEEDBACK_RS = CRATES_DIR / "feedbackmonk-repository" / "src" / "feedback.rs"
MIGRATIONS_DIR = REPO_ROOT / "migrations"
DELETE_TEST_RS = CRATES_DIR / "feedbackmonk-api" / "tests" / "me_feedback_delete.rs"

DELETE_HANDLER_FN = "delete_my_feedback"
ROW_DELETE_FN = "delete_for_end_user"
# P1-16 / M1: the derived-text scrub was factored into ONE shared helper so both
# the per-item and the user-level ("forget me") erasure paths use a single
# implementation. Probe D verifies the 4-table scrub lives in the helper AND that
# both erasure fns reach it (no path silently skips the scrub).
SCRUB_HELPER_FN = "scrub_cluster_derived_text"
BULK_ERASE_FN = "erase_all_for_end_user"


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p).replace("\\", "/")


def _strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
    text = re.sub(r"//[^\n\r]*", "", text)
    return text


def _iter_fn_bodies(text: str):
    """Yield (fn_name, body) for every fn that HAS a body. Skips signature-only
    trait declarations (`fn foo(...);`)."""
    for m in re.finditer(r"fn\s+(\w+)\s*[(<]", text):
        brace = text.find("{", m.end())
        semi = text.find(";", m.end())
        if brace == -1:
            continue
        if semi != -1 and semi < brace:
            continue
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


def _fn_body(text: str, fn_name: str) -> Optional[str]:
    for name, body in _iter_fn_bodies(text):
        if name == fn_name:
            return body
    return None


def probe_a() -> List[str]:
    """Erasure order: object-byte purge BEFORE the row delete."""
    if not ME_FEEDBACK_RS.exists():
        return [f"{rel(ME_FEEDBACK_RS)} does not exist — cannot verify the erasure handler"]
    text = ME_FEEDBACK_RS.read_text(encoding="utf-8")
    body = _fn_body(text, DELETE_HANDLER_FN)
    if body is None:
        return [
            f"{rel(ME_FEEDBACK_RS)}: `fn {DELETE_HANDLER_FN}` missing — the A1 erasure handler is gone"
        ]
    body = _strip_comments(body)
    offenders: List[str] = []

    # The row delete call (`.delete_for_end_user(`). NOTE `\.delete\(` below does
    # NOT match this (there is no `.delete(` substring in `.delete_for_end_user(`).
    row_del = re.search(rf"{ROW_DELETE_FN}\s*\(", body)
    # The object-store byte purge (`.delete(` on the storage handle, inside the
    # per-key purge loop). Distinct from the row delete by construction.
    byte_purge = re.search(r"\.delete\s*\(", body)

    if row_del is None:
        offenders.append(
            f"{rel(ME_FEEDBACK_RS)}::{DELETE_HANDLER_FN}: no `{ROW_DELETE_FN}(` call — the row is "
            "never hard-deleted."
        )
    if byte_purge is None:
        offenders.append(
            f"{rel(ME_FEEDBACK_RS)}::{DELETE_HANDLER_FN}: no `ObjectStore::delete` (`.delete(`) call — "
            "attachment object BYTES are never purged (DB cascade removes only the metadata rows). "
            "Residual screenshots/logs would survive erasure."
        )
    if row_del is not None and byte_purge is not None and byte_purge.start() > row_del.start():
        offenders.append(
            f"{rel(ME_FEEDBACK_RS)}::{DELETE_HANDLER_FN}: the object-byte purge (`.delete(`) runs AFTER "
            f"the row delete (`{ROW_DELETE_FN}(`). Purge the bytes BEFORE deleting the row — otherwise a "
            "mid-failure orphans object bytes with no row to find them from (D-A1 ordering)."
        )
    # The handler must fetch the storage keys it purges.
    if "list_storage_keys_for_feedback" not in body:
        offenders.append(
            f"{rel(ME_FEEDBACK_RS)}::{DELETE_HANDLER_FN}: does not call `list_storage_keys_for_feedback` "
            "— it cannot know which object-store keys to purge."
        )
    return offenders


def probe_b() -> List[str]:
    """Cascade completeness: every REFERENCES feedback(id) is CASCADE or SET NULL."""
    offenders: List[str] = []
    if not MIGRATIONS_DIR.is_dir():
        return [f"{rel(MIGRATIONS_DIR)} missing — cannot verify cascade completeness"]
    ref_re = re.compile(r"REFERENCES\s+feedback\s*\(\s*id\s*\)", re.IGNORECASE)
    for sql in sorted(MIGRATIONS_DIR.glob("*.sql")):
        text = _strip_comments(sql.read_text(encoding="utf-8"))
        for m in ref_re.finditer(text):
            # The ON DELETE clause follows REFERENCES feedback(id) on the same
            # column statement — inspect up to the next comma / newline.
            tail = text[m.end() : m.end() + 160]
            stop = min(
                [p for p in (tail.find(","), tail.find("\n")) if p != -1] or [len(tail)]
            )
            window = tail[:stop] if stop else tail
            if not re.search(r"ON\s+DELETE\s+(CASCADE|SET\s+NULL)", window, re.IGNORECASE):
                # Widen once in case the clause wrapped onto the next line.
                wide = tail[: stop + 60] if stop else tail
                if not re.search(r"ON\s+DELETE\s+(CASCADE|SET\s+NULL)", wide, re.IGNORECASE):
                    offenders.append(
                        f"migrations/{sql.name}: a `REFERENCES feedback(id)` has no "
                        f"`ON DELETE CASCADE`/`SET NULL` (near: {window.strip()[:70]!r}). A plain FK "
                        "defaults to NO ACTION, which would BLOCK erasure or ORPHAN child rows."
                    )
    return offenders


def probe_c() -> List[str]:
    """Sub-scoped DELETE: delete_for_end_user scopes by end_user_sub + tenant + project."""
    if not REPO_FEEDBACK_RS.exists():
        return [f"{rel(REPO_FEEDBACK_RS)} does not exist — cannot verify the erasure query"]
    text = REPO_FEEDBACK_RS.read_text(encoding="utf-8")
    body = _fn_body(text, ROW_DELETE_FN)
    if body is None:
        return [
            f"{rel(REPO_FEEDBACK_RS)}: `fn {ROW_DELETE_FN}` (impl) missing — the sub-scoped erasure "
            "query is gone."
        ]
    body_nc = _strip_comments(body)
    if not re.search(r"DELETE\s+FROM\s+feedback\b", body_nc, re.IGNORECASE):
        return [
            f"{rel(REPO_FEEDBACK_RS)}::{ROW_DELETE_FN}: no `DELETE FROM feedback` — the hard delete "
            "is missing."
        ]
    offenders: List[str] = []
    for col in ("end_user_sub", "tenant_id", "project_id"):
        if col not in body_nc:
            offenders.append(
                f"{rel(REPO_FEEDBACK_RS)}::{ROW_DELETE_FN}: the erasure DELETE does not scope by "
                f"`{col}` — a caller could erase a row outside their own (sub, tenant, project) "
                "(DEC-FBR-04 / DEC-FBR-03)."
            )
    return offenders


def probe_d() -> List[str]:
    """Derived-text completeness (scrutiny P0-1): `delete_for_end_user` must
    scrub the P5 analysis corpus, which has NO FK to feedback (it references the
    CLUSTER) and so is UNREACHED by the DELETE's cascade. Without this scrub, a
    'forget me' leaves the erased submitter's verbatim/paraphrased text alive in
    admin-readable prose (cluster.summary, recommendation.body, sweep digest)."""
    if not REPO_FEEDBACK_RS.exists():
        return [f"{rel(REPO_FEEDBACK_RS)} does not exist — cannot verify the erasure scrub"]
    text = REPO_FEEDBACK_RS.read_text(encoding="utf-8")
    offenders: List[str] = []

    # P1-16 / M1: the 4-table scrub lives in the shared `scrub_cluster_derived_text`
    # helper (used by BOTH erasure paths). Prefer it; fall back to the per-item fn
    # for older layouts where the SQL was still inlined there.
    scrub_body = _fn_body(text, SCRUB_HELPER_FN) or _fn_body(text, ROW_DELETE_FN)
    scrub_where = SCRUB_HELPER_FN if _fn_body(text, SCRUB_HELPER_FN) else ROW_DELETE_FN
    if scrub_body is None:
        return [
            f"{rel(REPO_FEEDBACK_RS)}: neither `fn {SCRUB_HELPER_FN}` (shared scrub helper) nor "
            f"`fn {ROW_DELETE_FN}` (impl) found — the derived-text scrub is gone."
        ]
    body_nc = _strip_comments(scrub_body)
    # Each derived table must be scrubbed (an UPDATE that nulls/clears its
    # free-text) inside the scrub fn. Detection-from-code: dropping any one
    # re-opens a residual-PII path.
    required = [
        ("feedback_clusters", r"UPDATE\s+feedback_clusters\b", r"summary\s*=\s*NULL"),
        ("recommendations", r"UPDATE\s+recommendations\b", r"body\s*=\s*''"),
        ("work_orders", r"UPDATE\s+work_orders\b", r"instructions\s*=\s*''"),
        ("analysis_sweeps", r"UPDATE\s+analysis_sweeps\b", r"digest_summary\s*=\s*NULL"),
    ]
    for table, upd_re, scrub_re in required:
        if not re.search(upd_re, body_nc, re.IGNORECASE):
            offenders.append(
                f"{rel(REPO_FEEDBACK_RS)}::{scrub_where}: no `UPDATE {table}` — the erased "
                f"feedback's derived text in `{table}` survives (no FK cascade reaches it)."
            )
        elif not re.search(scrub_re, body_nc, re.IGNORECASE):
            offenders.append(
                f"{rel(REPO_FEEDBACK_RS)}::{scrub_where}: `UPDATE {table}` present but does not "
                f"scrub its free-text (expected `{scrub_re}`)."
            )

    # If the scrub was factored into the helper, EVERY erasure path must actually
    # call it — otherwise a path could silently skip the scrub. Both the per-item
    # (`delete_for_end_user`) and the user-level (`erase_all_for_end_user`)
    # erasures must reach the helper.
    if scrub_where == SCRUB_HELPER_FN:
        for caller in (ROW_DELETE_FN, BULK_ERASE_FN):
            caller_body = _fn_body(text, caller)
            if caller_body is None:
                offenders.append(
                    f"{rel(REPO_FEEDBACK_RS)}: erasure fn `{caller}` missing — a required erasure "
                    "path is gone."
                )
            elif SCRUB_HELPER_FN not in _strip_comments(caller_body):
                offenders.append(
                    f"{rel(REPO_FEEDBACK_RS)}::{caller}: does not call `{SCRUB_HELPER_FN}` — this "
                    "erasure path skips the P5 derived-text scrub (residual-PII hole)."
                )
    return offenders


def probe_full(full: bool) -> Tuple[Optional[bool], str]:
    if not full:
        return None, "skipped (pass --full to run tests/me_feedback_delete.rs)"
    if not DELETE_TEST_RS.exists():
        return None, f"PENDING — {rel(DELETE_TEST_RS)} not written yet"
    cmd = ["cargo", "test", "-p", "feedbackmonk-api", "--test", "me_feedback_delete"]
    try:
        proc = subprocess.run(
            cmd, cwd=str(REPO_ROOT), capture_output=True, text=True, timeout=600
        )
    except FileNotFoundError:
        return None, "cargo not found — behavioral probe inconclusive"
    except subprocess.TimeoutExpired:
        return False, "me_feedback_delete tests timed out"
    if proc.returncode == 0:
        return True, "me_feedback_delete: all passed"
    tail = (proc.stdout + proc.stderr).strip().splitlines()[-8:]
    return False, "me_feedback_delete tests failed:\n      " + "\n      ".join(tail)


def main() -> int:
    parser = argparse.ArgumentParser(description="feedback-erasure-completeness oracle")
    parser.add_argument(
        "--full", action="store_true", help="also run tests/me_feedback_delete.rs"
    )
    args = parser.parse_args()

    a = probe_a()
    b = probe_b()
    c = probe_c()
    d = probe_d()
    full_passed, full_msg = probe_full(args.full)

    fails = sum(1 for x in (a, b, c, d) if x) + (1 if full_passed is False else 0)

    if fails == 0:
        print("PASS feedback-erasure-completeness")
        print(f"  Probe A (byte purge BEFORE row delete): clean ({rel(ME_FEEDBACK_RS)}::{DELETE_HANDLER_FN})")
        print("  Probe B (every REFERENCES feedback(id) is CASCADE/SET NULL): clean")
        print(f"  Probe C (erasure DELETE scoped by sub+tenant+project): clean ({ROW_DELETE_FN})")
        print("  Probe D (P5 derived-text scrubbed: clusters/recommendations/work_orders/sweeps): clean")
        print(f"  Probe --full (behavioral drift): {full_msg}")
        return 0

    print(f"FAIL feedback-erasure-completeness ({fails} probe(s) failed)")
    for label, offs, fixhint in (
        ("A (erasure order)", a,
         "purge attachment object bytes (ObjectStore::delete over list_storage_keys_for_feedback) BEFORE delete_for_end_user."),
        ("B (cascade completeness)", b,
         "every table referencing feedback(id) must be ON DELETE CASCADE or SET NULL."),
        ("C (sub-scoped delete)", c,
         "the erasure DELETE WHERE must include end_user_sub AND tenant_id AND project_id."),
        ("D (derived-text scrub)", d,
         "delete_for_end_user must UPDATE-scrub feedback_clusters/recommendations/work_orders/analysis_sweeps for the erased feedback's cluster (P0-1)."),
    ):
        if offs:
            print()
            print(f"Probe {label} failures:")
            for o in offs:
                print(f"  {o}")
            print(f"  Remediation: {fixhint}")
    if full_passed is False:
        print()
        print("Probe --full (behavioral drift):")
        print(f"  {full_msg}")
        print("  Remediation: cargo test -p feedbackmonk-api --test me_feedback_delete")
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
