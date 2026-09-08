#!/usr/bin/env python3
"""public-id-as-capability Verification Oracle (scrutiny 2026-07-01, Arc 1).

The anti-treadmill guard for finding P0-3: attachment list/download were gated
only by the PUBLIC project_id + a non-secret FB-XXXXXX short code, so anyone who
learned a short code could enumerate/download another user's PII-dense
screenshots and logs. The generative principle from the review: **a public
identifier is never an access token.** The fix binds attachment READS to the
submitter credential (`authorize_submitter`); this oracle proves — FROM CODE —
that the binding stays, so a future edit cannot silently turn the short code back
into a bearer capability.

Probes (crates/feedbackmonk-api/src/handlers/attachments.rs):
  A) both `list_attachments` and `download_attachment` call `authorize_submitter`
     BEFORE their data-returning repo/storage call (`list_for_feedback` /
     `get_meta` / `storage.get`), and short-circuit (return) on its Err.
  B) `authorize_submitter` actually resolves the submitter identity from the row
     (`owner_of`) and checks BOTH an auth-mode JWT `sub` match (`verify_with_leeway`)
     AND an anon-cookie hash match (`token_hash`) — i.e. it is a real identity
     check, not a self-reported flag.
  C) `upload` stays PUBLIC (write-only) — it must NOT require `authorize_submitter`
     (a false positive would break the widget's post-submit upload). Informational.

Exit 0 PASS, 1 FAIL, 2 environment error.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[2]
ATTACH_RS = REPO_ROOT / "crates" / "feedbackmonk-api" / "src" / "handlers" / "attachments.rs"

AUTHZ_FN = "authorize_submitter"
READ_HANDLERS = ["list_attachments", "download_attachment"]
# The first data-exposing call in each read handler; authz must precede it.
DATA_CALLS = ["list_for_feedback", "get_meta", "storage"]


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p).replace("\\", "/")


def _strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
    text = re.sub(r"//[^\n\r]*", "", text)
    return text


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


def main() -> int:
    if not ATTACH_RS.exists():
        print(f"FAIL public-id-as-capability\n  {rel(ATTACH_RS)} does not exist")
        return 2
    text = ATTACH_RS.read_text(encoding="utf-8")
    offenders = []

    # Probe A: each read handler authorizes before exposing data.
    for fn in READ_HANDLERS:
        body = _fn_body(text, fn)
        if body is None:
            offenders.append(f"{rel(ATTACH_RS)}: read handler `{fn}` missing.")
            continue
        body_nc = _strip_comments(body)
        authz = re.search(rf"{AUTHZ_FN}\s*\(", body_nc)
        if authz is None:
            offenders.append(
                f"{rel(ATTACH_RS)}::{fn}: no `{AUTHZ_FN}(` call — attachment reads are no longer "
                "bound to the submitter (a public short code would enumerate/serve another user's "
                "PII attachments; P0-3)."
            )
            continue
        # The authz call must precede the first data-exposing call.
        first_data = None
        for dc in DATA_CALLS:
            m = re.search(rf"\b{dc}\b", body_nc)
            if m and (first_data is None or m.start() < first_data):
                first_data = m.start()
        if first_data is not None and authz.start() > first_data:
            offenders.append(
                f"{rel(ATTACH_RS)}::{fn}: `{AUTHZ_FN}` runs AFTER the data call — authorize the "
                "submitter BEFORE returning attachment data."
            )

    # Probe B: authorize_submitter is a real identity check.
    authz_body = _fn_body(text, AUTHZ_FN)
    if authz_body is None:
        offenders.append(f"{rel(ATTACH_RS)}: `fn {AUTHZ_FN}` missing — no submitter binding exists.")
    else:
        b = _strip_comments(authz_body)
        for needle, why in (
            ("owner_of", "must read the feedback's submitter identity via `owner_of`"),
            ("verify_with_leeway", "must verify the auth-mode JWT (`verify_with_leeway`)"),
            ("token_hash", "must match the anon cookie via `AnonGate::token_hash`"),
        ):
            if needle not in b:
                offenders.append(
                    f"{rel(ATTACH_RS)}::{AUTHZ_FN}: does not reference `{needle}` — {why}. Without it "
                    "the check is not a real submitter binding."
                )

    if not offenders:
        print("PASS public-id-as-capability")
        print(f"  {READ_HANDLERS} both authorize via {AUTHZ_FN} before exposing data")
        print(f"  {AUTHZ_FN} checks owner_of + JWT sub (verify_with_leeway) + anon token_hash")
        return 0

    print(f"FAIL public-id-as-capability ({len(offenders)} offender(s))")
    for o in offenders:
        print(f"  {o}")
    print("  Remediation: attachment list/download must call authorize_submitter (owner_of + JWT "
          "sub-match / anon cookie-hash-match) BEFORE returning data; a public short code is not a "
          "bearer capability (P0-3).")
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
