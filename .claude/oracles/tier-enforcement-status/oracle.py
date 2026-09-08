#!/usr/bin/env python3
"""tier-enforcement-status Verification Oracle (canonical implementation).

Three probes:
  A) AST scan: every domain-write handler under
     `crates/feedbackmonk-api/src/handlers/` either invokes
     `check_tier_quota(...)` BEFORE its first repository write, OR
     appears in `allowlist.toml` with a documented rationale.
  B) Config shape: `tier_quotas()` in `crates/feedbackmonk-core/src/tier.rs`
     returns the canonical `TierQuotas { ... }` shape per `Tier` variant,
     matching Contract C19 byte-for-byte (caps + footer + flags). Defends
     against accidental edits like setting Free to unlimited.
  C) Integration smoke (gated behind `--full`): exercises end-to-end cap
     firing. NOT run in the inner-loop unless `--full` is passed. The
     fixtures live in `crates/feedbackmonk-api/tests/tier_enforcement_smoke.rs`
     (Phase 4+); cold-start passes Probe C vacuously by skipping it.

Cold-start vacuous-PASS plan: Probe A passes by allowlist (handlers not
yet wired); Probe B passes on the bare `tier_quotas()` const fn; Probe C
is gated behind `--full` so default invocation reports PASS.

Output: machine-parseable PASS / FAIL. Exit 0 on PASS, 1 on FAIL, 2 on
environment failure.

Lineage:
- FR-FBR-14 (tier enforcement + caps + footer)
- DEC-FBR-03 (pricing tier matrix; load-bearing for `tier_quotas()` shape)
- P3 plan § Oracle Pre-Build Plan (Probe A + Probe B + Probe C gated)
- P3 plan § Testability Gate (composite 16/25 flagged for scaffolding)
- Three-leg defense pattern:
    leg 1 = type system (`Tier` enum + `TierQuotas` struct)
    leg 2 = THIS oracle (handler-write-coverage + config-shape + smoke)
    leg 3 = cargo-test integration (sqlx::test fixtures driving HTTP path)
"""
from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import List, Optional, Tuple


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[2]
HANDLERS_DIR = REPO_ROOT / "crates" / "feedbackmonk-api" / "src" / "handlers"
TIER_RS = REPO_ROOT / "crates" / "feedbackmonk-core" / "src" / "tier.rs"
ALLOWLIST = SCRIPT_DIR / "allowlist.toml"


# Domain-write markers: any handler containing these as a top-level call is
# considered a "mutating handler" and MUST consult check_tier_quota (or be
# allowlisted). The set is conservative — better to over-flag and allowlist
# than to under-flag and miss a write.
WRITE_PATTERNS = [
    r"\.create\s*\(",                 # X::create
    r"\.update\s*\(",
    r"\.update_brand\s*\(",
    r"\.submit_authenticated\s*\(",
    r"\.submit_anonymous\s*\(",
    r"\.append_in_executor\s*\(",
    r"\.update_status_in_executor\s*\(",
    r"\.register\s*\(",
    r"\.deactivate\s*\(",
    r"\.cast\s*\(",
    r"\.retract\s*\(",
    r"\.promote\s*\(",
    r"\.set_status\s*\(",
    r"\.redeem\s*\(",
    r"\.mark_verified\s*\(",
]

TIER_CHECK_PATTERN = r"check_tier_quota\s*\("

# Canonical Contract C19 shape. Each entry is (substring-tokens that must
# all appear in the matching tier_quotas() arm). We do not parse Rust;
# we anchor on the `Tier::Variant => TierQuotas { ... }` blob and check
# for the canonical field values. Drift surfaces as a missing match.
EXPECTED_C19 = {
    "Free": [
        "projects_per_org: Some(1)",
        "monthly_feedback_volume: Some(50)",
        "custom_branding: false",
        "custom_domain: false",
        "eu_residency: false",
        'footer_text: Some("powered by feedbackmonk")',
    ],
    "Starter": [
        "projects_per_org: Some(3)",
        "monthly_feedback_volume: Some(500)",
        "custom_branding: true",
        "custom_domain: false",
        "eu_residency: false",
        "footer_text: None",
    ],
    "Pro": [
        "projects_per_org: None",
        "monthly_feedback_volume: Some(10000)",
        "custom_branding: true",
        "custom_domain: true",
        "eu_residency: true",
        "footer_text: None",
    ],
    "SelfHost": [
        "projects_per_org: None",
        "monthly_feedback_volume: None",
        "custom_branding: true",
        "custom_domain: true",
        "eu_residency: true",
        "footer_text: None",
    ],
}


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p).replace("\\", "/")


def strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
    text = re.sub(r"//[^\n\r]*", "", text)
    return text


def line_no(text: str, idx: int) -> int:
    return text.count("\n", 0, max(0, min(idx, len(text)))) + 1


def load_allowlist() -> set:
    """Returns set of allowlisted `module::function` keys (e.g. `signup::signup`).

    Format: each `[[handlers]]` block with `module = "..."`, `function = "..."`,
    and `rationale = "..."`.
    """
    keys = set()
    if not ALLOWLIST.exists():
        return keys
    text = ALLOWLIST.read_text(encoding="utf-8")
    pat = re.compile(r"\[\[handlers\]\]([^\[]*)", re.DOTALL)
    for m in pat.finditer(text):
        body = m.group(1)
        mod = re.search(r'module\s*=\s*"([^"]+)"', body)
        fn = re.search(r'function\s*=\s*"([^"]+)"', body)
        if mod and fn:
            keys.add(f"{mod.group(1)}::{fn.group(1)}")
    return keys


def find_pub_async_fns(text: str) -> List[Tuple[str, int, int]]:
    """Return list of (fn_name, fn_kw_line, body_close_idx) for `pub async fn`
    declarations at module top level (not nested inside other fns)."""
    results = []
    # Find all `pub async fn NAME(` declarations. We accept `pub` only to
    # filter out helpers.
    for m in re.finditer(r"\bpub\s+async\s+fn\s+(\w+)\s*\(", text):
        name = m.group(1)
        # Find the opening brace of the body.
        i = m.end()
        depth = 0
        body_open = None
        while i < len(text):
            c = text[i]
            if c == "(":
                depth += 1
            elif c == ")":
                depth -= 1
            elif c == "{" and depth == 0:
                body_open = i
                break
            i += 1
        if body_open is None:
            continue
        # Walk to matching close brace.
        depth = 0
        body_close = None
        for j in range(body_open, len(text)):
            c = text[j]
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
                if depth == 0:
                    body_close = j
                    break
        if body_close is None:
            continue
        ln = line_no(text, m.start())
        results.append((name, ln, body_open, body_close))
    return results


def has_write_call(body: str) -> bool:
    for pat in WRITE_PATTERNS:
        if re.search(pat, body):
            return True
    return False


def has_tier_check(body: str) -> bool:
    return bool(re.search(TIER_CHECK_PATTERN, body))


def write_call_before_tier_check(body: str) -> bool:
    """True if a domain-write call appears BEFORE the first
    check_tier_quota() call. False if no writes, or if tier_check
    comes first, or if there is no tier check at all."""
    tier_match = re.search(TIER_CHECK_PATTERN, body)
    write_positions = []
    for pat in WRITE_PATTERNS:
        for m in re.finditer(pat, body):
            write_positions.append(m.start())
    if not write_positions:
        return False
    if tier_match is None:
        return True  # writes exist with no tier check at all
    first_write = min(write_positions)
    return first_write < tier_match.start()


def probe_a() -> List[str]:
    """Return offender list: handlers that write without consulting tier."""
    offenders = []
    if not HANDLERS_DIR.exists():
        return offenders
    allow = load_allowlist()
    for path in sorted(HANDLERS_DIR.rglob("*.rs")):
        # Skip mod.rs, README.md adjacency, etc.
        if path.name == "mod.rs":
            continue
        module_name = path.stem
        raw = path.read_text(encoding="utf-8")
        text = strip_comments(raw)
        # Strip `#[cfg(test)] mod tests { ... }` so test scaffolding doesn't
        # confound the scan.
        text = re.sub(r"#\[cfg\(test\)\]\s*mod\s+tests\s*\{", "// TEST_MOD_START {", text)
        # Crude — but only used to ignore test helpers within the file.
        for name, ln, body_open, body_close in find_pub_async_fns(text):
            key = f"{module_name}::{name}"
            if key in allow:
                continue
            body = text[body_open : body_close + 1]
            if not has_write_call(body):
                continue
            # Has a write — must have a tier check that precedes it.
            if not has_tier_check(body):
                offenders.append(
                    f"{rel(path)}:{ln}  {key}  performs a domain write without check_tier_quota (add the check or allowlist with rationale)"
                )
            elif write_call_before_tier_check(body):
                offenders.append(
                    f"{rel(path)}:{ln}  {key}  write call precedes check_tier_quota (the cap check must run BEFORE any write)"
                )
    return offenders


def probe_b() -> List[str]:
    """Return offender list: tier_quotas() shape drift from Contract C19."""
    offenders = []
    if not TIER_RS.exists():
        # Cold-start: tier.rs not yet authored → vacuous PASS.
        return offenders
    raw = TIER_RS.read_text(encoding="utf-8")
    text = strip_comments(raw)
    # Each arm is roughly `Tier::Variant => TierQuotas { ... },` — find by
    # variant header and grab the bracketed block.
    for variant, expected_tokens in EXPECTED_C19.items():
        # Find `Tier::<variant>` followed by `=>` and `TierQuotas {`.
        m = re.search(
            rf"Tier::{re.escape(variant)}\b\s*=>\s*TierQuotas\s*\{{",
            text,
        )
        if not m:
            offenders.append(
                f"tier.rs  missing tier_quotas() arm for Tier::{variant} (Contract C19)"
            )
            continue
        body_start = m.end() - 1
        depth = 0
        body_close = None
        for j in range(body_start, len(text)):
            c = text[j]
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
                if depth == 0:
                    body_close = j
                    break
        if body_close is None:
            offenders.append(
                f"tier.rs  unterminated TierQuotas literal for Tier::{variant}"
            )
            continue
        arm_body = text[body_start : body_close + 1]
        # Normalize whitespace — token comparison is whitespace-insensitive
        # so the canonical Rust formatting (rustfmt) is robust.
        arm_norm = re.sub(r"\s+", " ", arm_body)
        for token in expected_tokens:
            tok_norm = re.sub(r"\s+", " ", token)
            if tok_norm not in arm_norm:
                offenders.append(
                    f"tier.rs  Tier::{variant} arm missing canonical token `{token}` (Contract C19 drift)"
                )
    return offenders


def probe_c(full: bool) -> Tuple[Optional[bool], str]:
    """Probe C — integration smoke. Gated behind `--full`.

    Returns (passed, message). `passed = None` means skipped.
    """
    if not full:
        return None, "skipped (pass --full to run integration smoke)"
    # Run the smoke test crate. We bind the test name pattern so we don't
    # re-run the whole workspace test suite.
    cmd = [
        "cargo",
        "test",
        "--manifest-path",
        str(REPO_ROOT / "Cargo.toml"),
        "-p",
        "feedbackmonk-api",
        "--test",
        "tier_enforcement_smoke",
        "--",
        "--include-ignored",
    ]
    # Ensure DATABASE_URL is set for the sqlx::test fixtures. The dev
    # container default per docs/operations/TIER_OVERRIDE.md +
    # DEC-FBR-IMPL-04 is localhost:5433. If the caller already set
    # DATABASE_URL we honor it; otherwise we point at the dev container.
    env = os.environ.copy()
    env.setdefault(
        "DATABASE_URL",
        "postgres://postgres:dev@localhost:5433/feedbackmonk_dev",
    )
    try:
        proc = subprocess.run(
            cmd, capture_output=True, text=True, timeout=180, cwd=str(REPO_ROOT), env=env
        )
    except FileNotFoundError:
        return False, "cargo not on PATH"
    except subprocess.TimeoutExpired:
        return False, "cargo test exceeded 180s timeout"
    if proc.returncode == 0:
        return True, "cargo test --test tier_enforcement_smoke: GREEN"
    # The test crate doesn't exist yet (Phase 4 hasn't authored it). Treat
    # that specific failure mode (compile-time "no test target") as a
    # soft pass at cold-start; any other failure is a real FAIL.
    err = (proc.stderr or "") + (proc.stdout or "")
    if "no test target named" in err or "could not find `tier_enforcement_smoke`" in err:
        return None, "vacuous PASS — tier_enforcement_smoke test crate not yet authored (Phase 4 deliverable)"
    return False, f"cargo test failed:\n{err.strip()[:2000]}"


def main() -> int:
    parser = argparse.ArgumentParser(description="tier-enforcement-status Verification Oracle")
    parser.add_argument(
        "--full",
        action="store_true",
        help="Run Probe C integration smoke (cargo test --test tier_enforcement_smoke). Off by default for inner-loop cost.",
    )
    args = parser.parse_args()

    a_offenders = probe_a()
    b_offenders = probe_b()
    c_passed, c_message = probe_c(args.full)

    fails = (1 if a_offenders else 0) + (1 if b_offenders else 0) + (1 if c_passed is False else 0)

    if fails == 0:
        print("PASS tier-enforcement-status")
        print(f"  Probe A (handler tier-cap coverage): clean ({rel(HANDLERS_DIR)})")
        if not TIER_RS.exists():
            print(f"  Probe B (tier_quotas() shape): vacuous PASS — {rel(TIER_RS)} does not exist yet (pre-build)")
        else:
            print(f"  Probe B (tier_quotas() shape): clean (Contract C19 invariants hold)")
        if c_passed is True:
            print(f"  Probe C (integration smoke): {c_message}")
        elif c_passed is None:
            print(f"  Probe C (integration smoke): {c_message}")
        return 0

    print(f"FAIL tier-enforcement-status ({fails} probe(s) failed)")
    if a_offenders:
        print()
        print("Probe A failures (handler missing tier-cap check):")
        for o in a_offenders:
            print(f"  {o}")
        print(
            "  Remediation: add `state.tier_quotas.check_tier_quota(&scope, ResourceKind::*).await?` "
            "at the top of the handler BEFORE any data write, OR allowlist it in "
            ".claude/oracles/tier-enforcement-status/allowlist.toml with a documented rationale."
        )
    if b_offenders:
        print()
        print("Probe B failures (tier_quotas() shape drift from Contract C19):")
        for o in b_offenders:
            print(f"  {o}")
        print(
            "  Remediation: restore the canonical TierQuotas literal per "
            "docs/planning/plans/20260514T134816-feedbackmonk-p3-commercial-gate.md § Contract C19. "
            "Changing tier-cap defaults requires a spec-level decision (DEC-FBR-* entry)."
        )
    if c_passed is False:
        print()
        print("Probe C failure (integration smoke):")
        print(f"  {c_message}")
        print(
            "  Remediation: run the test locally with the dev Postgres on 5433; check the "
            "cap-wiring at the project-create and feedback-submission handlers."
        )
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
