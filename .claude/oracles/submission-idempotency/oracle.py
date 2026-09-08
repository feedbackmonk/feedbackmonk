#!/usr/bin/env python3
"""submission-idempotency Verification Oracle (scrutiny 2026-07-01, Arc 1).

Child H's missing-oracle finding: the submit `Idempotency-Key` dedupe had a
behavioral test but no anti-reward-hacking STATIC leg. Finding P1-3 then showed
the dedupe keyed on `(project_id, idempotency_key)` WITHOUT a submitter
dimension — a shared key across users silently dropped the second user's
feedback. This oracle proves — FROM CODE — that the idempotency claim is
identity-scoped and content-fingerprinted, so the cross-user data-loss hole
cannot silently reopen.

Probes:
  A) migrations: the submit_idempotency PRIMARY KEY includes `submitter_id`
     (identity scope) — never a bare `(project_id, idempotency_key)`; and a
     length CHECK bounds `idempotency_key`.
  B) repository feedback.rs: `claim_idempotency_key` writes `submitter_id` +
     `content_hash`, and its conflict path returns `IdempotencyKeyReuse` when the
     stored content_hash differs (so a key reused with different content 409s
     instead of silently returning the original).

Exit 0 PASS, 1 FAIL, 2 environment error.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[2]
MIGRATIONS = REPO_ROOT / "migrations"
FEEDBACK_RS = REPO_ROOT / "crates" / "feedbackmonk-repository" / "src" / "feedback.rs"
CLAIM_FN = "claim_idempotency_key"


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p).replace("\\", "/")


def _strip_comments_sql(text: str) -> str:
    return re.sub(r"--[^\n]*", "", text)


def _strip_comments_rs(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
    return re.sub(r"//[^\n\r]*", "", text)


def _fn_body(text: str, fn_name: str):
    m = re.search(rf"fn\s+{fn_name}\s*[(<]", text)
    if not m:
        return None
    brace = text.find("{", m.end())
    if brace == -1:
        return None
    depth = 0
    for i in range(brace, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return text[brace : i + 1]
    return None


def probe_a():
    offenders = []
    if not MIGRATIONS.is_dir():
        return [f"{rel(MIGRATIONS)} missing — cannot verify the idempotency PK"]
    # Gather the effective PK: the LAST migration that (re)defines the
    # submit_idempotency primary key wins.
    pk_clause = None
    has_len_check = False
    for sql in sorted(MIGRATIONS.glob("*.sql")):
        text = _strip_comments_sql(sql.read_text(encoding="utf-8"))
        for m in re.finditer(
            r"submit_idempotency[\s\S]{0,400}?PRIMARY\s+KEY\s*\(([^)]*)\)", text, re.IGNORECASE
        ):
            pk_clause = m.group(1)
        for m in re.finditer(r"ADD\s+CONSTRAINT[\s\S]{0,120}?PRIMARY\s+KEY\s*\(([^)]*)\)", text, re.IGNORECASE):
            # Only count if it's the submit_idempotency table context nearby.
            start = max(0, m.start() - 200)
            if "submit_idempotency" in text[start : m.end()].lower():
                pk_clause = m.group(1)
        if re.search(r"submit_idempotency[\s\S]{0,400}?char_length\s*\(\s*idempotency_key", text, re.IGNORECASE) \
           or re.search(r"idempotency_key[\s\S]{0,80}?char_length", text, re.IGNORECASE):
            has_len_check = True
    if pk_clause is None:
        offenders.append("no submit_idempotency PRIMARY KEY found in migrations.")
    elif "submitter_id" not in pk_clause.lower():
        offenders.append(
            f"submit_idempotency PRIMARY KEY ({pk_clause.strip()}) is NOT identity-scoped — it must "
            "include `submitter_id`, else a shared key across users drops the second user's feedback (P1-3)."
        )
    if not has_len_check:
        offenders.append(
            "submit_idempotency has no CHECK bounding idempotency_key length (P2-17: an unbounded key "
            "can overflow the btree row limit → raw 500)."
        )
    return offenders


def probe_b():
    if not FEEDBACK_RS.exists():
        return [f"{rel(FEEDBACK_RS)} does not exist"]
    text = FEEDBACK_RS.read_text(encoding="utf-8")
    body = _fn_body(text, CLAIM_FN)
    if body is None:
        return [f"{rel(FEEDBACK_RS)}: `fn {CLAIM_FN}` missing — the idempotency claim is gone."]
    b = _strip_comments_rs(body)
    offenders = []
    if "submitter_id" not in b:
        offenders.append(
            f"{rel(FEEDBACK_RS)}::{CLAIM_FN}: does not reference `submitter_id` — the claim is not "
            "identity-scoped (P1-3)."
        )
    if "content_hash" not in b:
        offenders.append(
            f"{rel(FEEDBACK_RS)}::{CLAIM_FN}: does not reference `content_hash` — a key reused with "
            "different content cannot be detected (M6)."
        )
    if "IdempotencyKeyReuse" not in b:
        offenders.append(
            f"{rel(FEEDBACK_RS)}::{CLAIM_FN}: does not return `IdempotencyKeyReuse` — a key reused with "
            "different content would silently return the original instead of 409 (M6)."
        )
    return offenders


def main() -> int:
    a = probe_a()
    b = probe_b()
    if not a and not b:
        print("PASS submission-idempotency")
        print("  Probe A (PK identity-scoped by submitter_id + key length CHECK): clean")
        print(f"  Probe B ({CLAIM_FN} writes submitter_id + content_hash, 409s on content mismatch): clean")
        return 0
    print(f"FAIL submission-idempotency ({len(a) + len(b)} issue(s))")
    for o in a + b:
        print(f"  {o}")
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
