#!/usr/bin/env python3
"""cors-allowlist-enforcement Verification Oracle (canonical implementation).

Defends the credentialed-CORS posture for the public widget endpoints
(feedback submission + attachment upload) as a CODE-STATE invariant — the
posture that DEC-FBR-IMPL-09 introduced to fix the GitCellar embed blocker
(preflight OPTIONS returned 405 because no CORS layer was wired).

Why this exists beyond `tests/cors_preflight.rs`: that test exercises
`public_cors_layer(...)` in isolation, so it stays green even if someone
deletes the `.layer(cors)` wiring from `main.rs::build_app` — i.e. it cannot
catch a *wiring-removal* regression, which is exactly how the bug would
silently return. This oracle reads the wiring + policy from source, closing
that gap.

Two probes (both static, no compile, no DB):

  A) Wiring: `crates/feedbackmonk-api/src/main.rs` builds the CORS layer
     (`public_cors_layer(...)`), reads the `FEEDBACKMONK_CORS_ORIGINS`
     allowlist, AND applies `.layer(...)` to BOTH the submission router and
     the attachments router. If either endpoint loses its CORS layer, the
     browser preflight 405-regresses for that endpoint.

  B) Policy: `crates/feedbackmonk-api/src/cors.rs::public_cors_layer` keeps
     the credentialed, echo-origin posture — `allow_credentials(true)` +
     `AllowOrigin::list(...)` (per-request echo) — and NEVER uses a wildcard
     origin (`AllowOrigin::any()` / `allow_origin(Any)`). A wildcard with
     credentials is invalid per the Fetch spec (tower-http panics), and the
     anonymous `credentials: include` path requires the specific origin to be
     echoed (DEC-FBR-IMPL-09 / DEC-FBR-04).

Optional Probe C (gated behind `--full`): run the behavioral integration
test `cargo test -p feedbackmonk-api --test cors_preflight`. NOT run in the
inner loop unless `--full` is passed.

Output: machine-parseable PASS / FAIL. Exit 0 on PASS, 1 on FAIL, 2 on
environment failure.

Lineage:
- DEC-FBR-IMPL-09 (CORS on public widget endpoints + cross-site anon cookie)
- DEC-FBR-04 (domain allowlist for widget embed — CORS at the submission endpoint)
- Probandurgy Verification Oracle pattern (DEC-FBR-IMPL-03 canonical-Python + shims)
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path
from typing import List

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[2]
MAIN_RS = REPO_ROOT / "crates" / "feedbackmonk-api" / "src" / "main.rs"
CORS_RS = REPO_ROOT / "crates" / "feedbackmonk-api" / "src" / "cors.rs"

ENV_VAR = "FEEDBACKMONK_CORS_ORIGINS"


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p)


def probe_a() -> List[str]:
    """Wiring: main.rs builds + applies the CORS layer to submit AND attachments."""
    offenders: List[str] = []
    if not MAIN_RS.exists():
        return [f"{rel(MAIN_RS)} does not exist"]
    text = MAIN_RS.read_text(encoding="utf-8")

    if "public_cors_layer(" not in text:
        offenders.append(
            f"{rel(MAIN_RS)}: never calls public_cors_layer(...) — the CORS layer is not built"
        )
    if ENV_VAR not in text:
        offenders.append(
            f"{rel(MAIN_RS)}: does not read {ENV_VAR} — the allowlist source is gone"
        )
    # `.layer(` applied to each public credentialed router (single-line merge calls).
    if not re.search(r"submission_router\(.*\)\s*\.layer\(", text):
        offenders.append(
            f"{rel(MAIN_RS)}: submission_router is not wrapped with .layer(<cors>) — "
            "preflight will 405-regress on POST .../feedback"
        )
    if not re.search(r"attachments_router\(.*\)\s*\.layer\(", text):
        offenders.append(
            f"{rel(MAIN_RS)}: attachments_router is not wrapped with .layer(<cors>) — "
            "preflight will 405-regress on POST .../attachments"
        )
    return offenders


def probe_b() -> List[str]:
    """Policy: public_cors_layer is credentialed + echo-origin, never wildcard."""
    offenders: List[str] = []
    if not CORS_RS.exists():
        return [f"{rel(CORS_RS)} does not exist"]
    text = CORS_RS.read_text(encoding="utf-8")

    if "fn public_cors_layer" not in text:
        offenders.append(f"{rel(CORS_RS)}: public_cors_layer fn missing")
        return offenders

    if not re.search(r"\.allow_credentials\s*\(\s*true\s*\)", text):
        offenders.append(
            f"{rel(CORS_RS)}: missing .allow_credentials(true) — the anonymous "
            "credentials:include path needs Access-Control-Allow-Credentials"
        )
    if "AllowOrigin::list" not in text:
        offenders.append(
            f"{rel(CORS_RS)}: missing AllowOrigin::list(...) — origin must be echoed "
            "from an explicit allowlist, not wildcarded"
        )
    # Wildcard guard: a wildcard origin with credentials is invalid (and panics).
    for forbidden in (r"AllowOrigin::any\s*\(", r"allow_origin\s*\(\s*Any", r"cors::Any"):
        if re.search(forbidden, text):
            offenders.append(
                f"{rel(CORS_RS)}: wildcard origin detected (`{forbidden}`) — credentialed "
                "CORS MUST echo the specific origin, never '*' (DEC-FBR-IMPL-09)"
            )
    return offenders


def probe_c(full: bool):
    """Behavioral integration test (gated behind --full)."""
    if not full:
        return None, "skipped (pass --full to run the cors_preflight integration test)"
    try:
        proc = subprocess.run(
            ["cargo", "test", "-p", "feedbackmonk-api", "--test", "cors_preflight"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            timeout=600,
        )
    except FileNotFoundError:
        return None, "cargo not found — Probe C inconclusive"
    except subprocess.TimeoutExpired:
        return False, "cargo test --test cors_preflight timed out"
    if proc.returncode == 0:
        return True, "cargo test --test cors_preflight: all passed"
    tail = (proc.stdout + proc.stderr).strip().splitlines()[-8:]
    return False, "cors_preflight failed:\n      " + "\n      ".join(tail)


def main() -> int:
    parser = argparse.ArgumentParser(description="cors-allowlist-enforcement oracle")
    parser.add_argument(
        "--full",
        action="store_true",
        help="also run the cors_preflight integration test (Probe C)",
    )
    args = parser.parse_args()

    a_offenders = probe_a()
    b_offenders = probe_b()
    c_passed, c_message = probe_c(args.full)

    fails = (1 if a_offenders else 0) + (1 if b_offenders else 0) + (1 if c_passed is False else 0)

    if fails == 0:
        print("PASS cors-allowlist-enforcement")
        print(f"  Probe A (CORS layer wired to submit + attachments): clean ({rel(MAIN_RS)})")
        print(f"  Probe B (credentialed echo-origin policy, never '*'): clean ({rel(CORS_RS)})")
        print(f"  Probe C (integration smoke): {c_message}")
        return 0

    print(f"FAIL cors-allowlist-enforcement ({fails} probe(s) failed)")
    if a_offenders:
        print()
        print("Probe A failures (CORS wiring):")
        for o in a_offenders:
            print(f"  {o}")
        print(
            "  Remediation: in main.rs::build_app, build the layer with "
            "`public_cors_layer(&cors_origins)` (origins from FEEDBACKMONK_CORS_ORIGINS) "
            "and apply `.layer(cors.clone())` to BOTH submission_router and attachments_router."
        )
    if b_offenders:
        print()
        print("Probe B failures (CORS policy drift):")
        for o in b_offenders:
            print(f"  {o}")
        print(
            "  Remediation: public_cors_layer must use AllowOrigin::list(...) + "
            ".allow_credentials(true) and never a wildcard origin. See DEC-FBR-IMPL-09."
        )
    if c_passed is False:
        print()
        print("Probe C failure (integration smoke):")
        print(f"  {c_message}")
        print("  Remediation: cargo test -p feedbackmonk-api --test cors_preflight")
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
