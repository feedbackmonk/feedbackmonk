#!/usr/bin/env python3
"""public-route-ceiling Verification Oracle (scrutiny 2026-07-01, Arc 1).

The anti-treadmill guard for the class-level per-IP rate ceiling (finding P0-2).
The fix moved the abuse FLOOR from a per-handler cookie-keyed limiter (which
every route added after P0 silently failed to inherit — voting, attachments) to
a router-level middleware, `apply_public_rate_limit`, applied to EVERY public
router in `build_app`. This oracle proves — FROM CODE — that each known public
router is still wrapped by that middleware, so a future edit cannot silently
drop the ceiling from a public surface (the exact "wiring-removal" regression a
handler unit test cannot catch — the same class the cors-allowlist-enforcement
oracle exists for).

Probe: in `crates/feedbackmonk-api/src/main.rs::build_app`, each public router in
PUBLIC_ROUTERS must appear as the FIRST argument of an `apply_public_rate_limit(`
call. A public router merged directly (without the wrapper) fails.

Adding a NEW public route is a reviewable surface: the author must add its router
to PUBLIC_ROUTERS here (and wrap it in build_app) — that is the point.

Exit 0 PASS, 1 FAIL, 2 environment error.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[2]
MAIN_RS = REPO_ROOT / "crates" / "feedbackmonk-api" / "src" / "main.rs"
WRAPPER = "apply_public_rate_limit"

# The public (end-user-reachable, unauthenticated-or-widget) routers that MUST
# carry the class-level per-IP ceiling. Keep in sync with build_app.
PUBLIC_ROUTERS = [
    "submission_router",
    "attachments_router",
    "board_router",
    "roadmap_router",
]


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p).replace("\\", "/")


def _build_app_body(text: str):
    m = re.search(r"fn\s+build_app\s*\(", text)
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
    if not MAIN_RS.exists():
        print(f"FAIL public-route-ceiling\n  {rel(MAIN_RS)} does not exist")
        return 2
    text = MAIN_RS.read_text(encoding="utf-8")
    body = _build_app_body(text)
    if body is None:
        print(f"FAIL public-route-ceiling\n  {rel(MAIN_RS)}: `fn build_app` not found")
        return 1

    offenders = []
    for router in PUBLIC_ROUTERS:
        if not re.search(rf"\b{router}\s*\(", body):
            offenders.append(
                f"{rel(MAIN_RS)}::build_app: public router `{router}` is no longer mounted "
                "(removed? renamed?). Update PUBLIC_ROUTERS or restore it."
            )
            continue
        # It must be the FIRST argument of apply_public_rate_limit(...): allow
        # whitespace/newlines between the wrapper's `(` and the router call.
        if not re.search(rf"{WRAPPER}\s*\(\s*{router}\s*\(", body):
            offenders.append(
                f"{rel(MAIN_RS)}::build_app: public router `{router}` is NOT wrapped by "
                f"`{WRAPPER}(...)` — the class-level per-IP rate ceiling (P0-2) is missing from a "
                "public surface. Wrap it: `apply_public_rate_limit(" + router + "(...), prl.clone())`."
            )

    if not offenders:
        print("PASS public-route-ceiling")
        print(f"  every public router {PUBLIC_ROUTERS} is wrapped by {WRAPPER} in build_app")
        return 0

    print(f"FAIL public-route-ceiling ({len(offenders)} offender(s))")
    for o in offenders:
        print(f"  {o}")
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
