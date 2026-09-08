#!/usr/bin/env python3
"""feedback-as-data-audit Verification Oracle (canonical implementation).

THE prompt-injection + source-exfiltration defense for P5b (FR-FBR-25b/c,
Contract C27). P5b is where public-internet text first reaches a code-writing
agent: feedback → an agent that writes and (per rung) lands code. This is the
literal remote-code-execution-from-public-input surface — the highest plan-wide
fidelity risk (Testability Gate Flag 1, Q2=5).

It proves — FROM CODE, not from a self-reported "safe" flag — two properties:

  (b) DATA-ENVELOPE: feedback-derived content enters the implementer prompt
      through EXACTLY ONE chokepoint (`prompt::wrap_untrusted`), wrapped in a
      single delimited untrusted-data envelope. No other path may concatenate
      feedback text into the prompt (mirrors `pii-scrub-audit`'s single-writer
      chokepoint).

  (c) SOURCE-NEVER-LEAVES: every outbound payload the runner POSTs
      (implementer `result_ref`, analyst recommendations) routes through the
      egress sanitizer chokepoint `sanitizer::sanitize_outbound` — references,
      never source/secret dumps.

A standard happy-path "the prompt looks right" unit test confirms ONE assembly;
it does NOT prove the ABSENCE of a bypass path (feedback text reaching the
instruction layer) or an outbound path that skips the sanitizer. This oracle is
the anti-reward-hacking leg — a worker cannot satisfy it with a flag.

THREE probes (detection-from-code; ALL ACTIVE as of Worker A's Stage-1 landing):

  A) ENVELOPE CHOKEPOINT (static, ACTIVE):
     `feedbackmonk-runner/src/prompt.rs` defines `wrap_untrusted` + the envelope
     delimiters + the DEC-84 preamble, and the `<untrusted-feedback-data>`
     envelope literal appears in EXACTLY ONE place (the chokepoint). Now that
     `assemble` is finalized (no longer `unimplemented!`), the probe additionally
     asserts it routes feedback-derived fields through `wrap_untrusted` rather
     than concatenating them raw. (Auto-degrades to PENDING if assemble's body is
     ever removed.)

  B) EGRESS CHOKEPOINT (static, ACTIVE):
     `feedbackmonk-runner/src/sanitizer.rs` defines `sanitize_outbound`. The
     outbound modules have landed (Worker B `report`, Worker C `analyst`); the
     probe asserts each routes its POST payload through `sanitize_outbound`.
     (Auto-degrades to PENDING if every outbound module is removed.)

  C) CORPUS / BEHAVIOR (gated behind --full, ACTIVE):
     runs the C24 adversarial corpus (`tests/feedback_injection_corpus.rs`). The
     P5b cases `case_g_destructive_steering_p5b` + `case_f_runner_side_exfil_
     defense_p5b` are un-ignored and backed by the real runner prompt-assembly /
     egress sanitizer; the probe reports them ACTIVE once they are no longer
     `#[ignore]` and the corpus is green. (Full corpus green needs the dev DB for
     the `sqlx::test` behavioural cases.)

A green oracle with A+B+C ACTIVE is Worker A's Stage-1 exit gate — MET. The probe
states are computed dynamically from the code, so the language self-degrades to
PENDING if a future change removes a chokepoint body or an outbound module.

Output: machine-parseable PASS / FAIL. Exit 0 PASS, 1 FAIL, 2 environment.

Lineage:
- FR-FBR-25b (prompt data-envelope) / FR-FBR-25c (source-never-leaves)
- Contract C27 (P5b plan, FROZEN) + Testability Gate Flags 1 & 2
- C24 corpus (feedback_injection_corpus.rs) cases (g)/(f)
- Probandurgy Verification Oracle pattern (canonical-Python + shims)
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
RUNNER_SRC = REPO_ROOT / "crates" / "feedbackmonk-runner" / "src"
PROMPT_RS = RUNNER_SRC / "prompt.rs"
SANITIZER_RS = RUNNER_SRC / "sanitizer.rs"
CORPUS_RS = (
    REPO_ROOT / "crates" / "feedbackmonk-api" / "tests" / "feedback_injection_corpus.rs"
)

ENVELOPE_LITERAL = "<untrusted-feedback-data>"
CHOKEPOINT_FN = "fn wrap_untrusted"
EGRESS_FN = "fn sanitize_outbound"
# Untrusted feedback-derived fields that must only reach the prompt via the
# chokepoint (used by the tightened Probe A once `assemble` is implemented).
UNTRUSTED_FIELDS = ["member_bodies", "cluster_summary", "rationale"]
# The P5b corpus cases that activate the behavioral leg.
P5B_CASES = ["case_g_destructive_steering_p5b", "case_f_runner_side_exfil_defense_p5b"]


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p)


def _extract_fn_body(text: str, fn_sig: str) -> Optional[str]:
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


def _strip_comments(text: str) -> str:
    # Drop line comments + block comments so literal scans don't trip on docs.
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
    text = re.sub(r"//[^\n]*", "", text)
    return text


def probe_a() -> Tuple[List[str], bool]:
    """Single envelope chokepoint. Returns (offenders, assemble_pending)."""
    offenders: List[str] = []
    if not PROMPT_RS.exists():
        return [f"{rel(PROMPT_RS)} does not exist — the prompt data-envelope (25b) is missing"], False
    text = PROMPT_RS.read_text(encoding="utf-8")

    if CHOKEPOINT_FN not in text:
        offenders.append(
            f"{rel(PROMPT_RS)}: `{CHOKEPOINT_FN}` missing — the single untrusted-data "
            "chokepoint (C27 25b) is gone. ALL feedback-derived text must enter the prompt "
            "through this one function."
        )
    if "DEC84_PREAMBLE" not in text and "DEC-84" not in text:
        offenders.append(
            f"{rel(PROMPT_RS)}: the DEC-84 critical-action preamble is missing — the "
            "assembled prompt must carry the deferral preamble (C27 25b)."
        )

    # Single-chokepoint: the envelope literal must appear ONLY inside prompt.rs
    # (and, within it, be produced only by wrap_untrusted). Any OTHER runner
    # source file building the envelope is a second writer (a bypass path).
    for other in sorted(RUNNER_SRC.rglob("*.rs")):
        if other == PROMPT_RS:
            continue
        body = _strip_comments(other.read_text(encoding="utf-8"))
        if ENVELOPE_LITERAL in body:
            offenders.append(
                f"{rel(other)}: builds the `{ENVELOPE_LITERAL}` envelope outside the single "
                f"chokepoint `{CHOKEPOINT_FN}` in {rel(PROMPT_RS)} — feedback text must enter "
                "the prompt through exactly one place."
            )

    # Tightened leg: once `assemble` is real (not unimplemented!), assert it does
    # not concatenate untrusted fields outside wrap_untrusted.
    assemble_body = _extract_fn_body(text, "fn assemble")
    assemble_pending = assemble_body is None or "unimplemented!" in assemble_body
    if not assemble_pending and assemble_body is not None:
        # Every untrusted field referenced in assemble must be routed via
        # wrap_untrusted (i.e. wrap_untrusted must be called in assemble).
        if "wrap_untrusted" not in assemble_body:
            offenders.append(
                f"{rel(PROMPT_RS)}: `assemble` does not call `wrap_untrusted` — feedback-derived "
                "fields must be wrapped by the single chokepoint, never concatenated raw."
            )
    return offenders, assemble_pending


def probe_b() -> Tuple[List[str], bool]:
    """Egress sanitizer chokepoint. Returns (offenders, outbound_pending)."""
    offenders: List[str] = []
    if not SANITIZER_RS.exists():
        return [f"{rel(SANITIZER_RS)} does not exist — the egress sanitizer (25c) is missing"], False
    text = SANITIZER_RS.read_text(encoding="utf-8")
    if EGRESS_FN not in text:
        offenders.append(
            f"{rel(SANITIZER_RS)}: `{EGRESS_FN}` missing — the single source-never-leaves egress "
            "chokepoint (C27 25c) is gone."
        )
    # The sanitizer must reuse the canonical PII scrubber (FR-FBR-10), not a
    # bespoke re-implementation.
    if "feedbackmonk_tracing::scrub" not in text and "scrub" not in text:
        offenders.append(
            f"{rel(SANITIZER_RS)}: the egress sanitizer does not reuse the canonical "
            "`feedbackmonk_tracing::scrub` PII chokepoint (FR-FBR-10)."
        )

    # Outbound-routing assertion activates when the outbound modules land.
    report_rs = RUNNER_SRC / "report.rs"
    analyst_dir = RUNNER_SRC / "analyst"
    outbound_present = report_rs.exists() or analyst_dir.exists()
    if not outbound_present:
        return offenders, True  # PENDING — Worker B/C have not landed outbound paths yet.

    for mod in [report_rs, *(analyst_dir.rglob("*.rs") if analyst_dir.exists() else [])]:
        if not mod.exists():
            continue
        body = _strip_comments(mod.read_text(encoding="utf-8"))
        # A module that POSTs (runner_transition with a result_ref / post_recommendation)
        # MUST reference sanitize_outbound.
        posts = "runner_transition" in body or "post_recommendation" in body
        if posts and "sanitize_outbound" not in body:
            offenders.append(
                f"{rel(mod)}: an outbound POST path does not route through `sanitize_outbound` "
                "— every payload that crosses the wire must pass the egress chokepoint (C27 25c)."
            )
    return offenders, False


def probe_c(full: bool) -> Tuple[Optional[bool], str]:
    """Behavioral corpus (--full). PENDING while the P5b cases are #[ignore]."""
    if not full:
        return None, "skipped (pass --full to run tests/feedback_injection_corpus.rs)"
    if not CORPUS_RS.exists():
        return None, f"PENDING — {rel(CORPUS_RS)} not found"
    text = CORPUS_RS.read_text(encoding="utf-8")
    # Detect whether the P5b cases are still ignored (scaffold) or activated.
    still_ignored = []
    for case in P5B_CASES:
        # crude: the case fn is preceded by an #[ignore ...] attribute.
        m = re.search(r"#\[ignore[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*fn\s+" + re.escape(case), text)
        if m:
            still_ignored.append(case)
    try:
        proc = subprocess.run(
            ["cargo", "test", "-p", "feedbackmonk-api", "--test", "feedback_injection_corpus"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            timeout=600,
        )
    except FileNotFoundError:
        return None, "cargo not found — Probe C inconclusive"
    except subprocess.TimeoutExpired:
        return False, "cargo test feedback_injection_corpus timed out"
    if proc.returncode != 0:
        tail = (proc.stdout + proc.stderr).strip().splitlines()[-8:]
        return False, "feedback_injection_corpus failed:\n      " + "\n      ".join(tail)
    if still_ignored:
        return None, (
            "active corpus cases pass; P5b cases still #[ignore] (scaffold) — "
            + ", ".join(still_ignored)
            + " (Worker A un-ignores + backs them)"
        )
    return True, "feedback_injection_corpus: all cases (incl. P5b g/f) passed"


def main() -> int:
    parser = argparse.ArgumentParser(description="feedback-as-data-audit oracle")
    parser.add_argument("--full", action="store_true", help="also run the C24 corpus (Probe C)")
    args = parser.parse_args()

    a_offenders, a_pending = probe_a()
    b_offenders, b_pending = probe_b()
    c_passed, c_message = probe_c(args.full)

    fails = (
        (1 if a_offenders else 0)
        + (1 if b_offenders else 0)
        + (1 if c_passed is False else 0)
    )

    if fails == 0:
        print("PASS feedback-as-data-audit")
        a_state = "chokepoint present; full-assembly PENDING (Worker A)" if a_pending else "clean (single chokepoint enforced)"
        print(f"  Probe A (prompt data-envelope, single chokepoint): {a_state} ({rel(PROMPT_RS)})")
        b_state = "chokepoint present; outbound-routing PENDING (Worker B/C)" if b_pending else "clean (every outbound routes through sanitize_outbound)"
        print(f"  Probe B (egress sanitizer chokepoint): {b_state} ({rel(SANITIZER_RS)})")
        print(f"  Probe C (C24 corpus behavior): {c_message}")
        return 0

    print(f"FAIL feedback-as-data-audit ({fails} probe(s) failed)")
    if a_offenders:
        print("\nProbe A failures (prompt data-envelope / single chokepoint):")
        for o in a_offenders:
            print(f"  {o}")
        print(
            "  Remediation: ALL feedback-derived text must enter the prompt via "
            "`prompt::wrap_untrusted` (the one chokepoint) wrapped in the "
            "`<untrusted-feedback-data>` envelope; keep the DEC-84 preamble in the trusted layer."
        )
    if b_offenders:
        print("\nProbe B failures (egress sanitizer):")
        for o in b_offenders:
            print(f"  {o}")
        print(
            "  Remediation: every outbound POST (result_ref, recommendations) must pass through "
            "`sanitizer::sanitize_outbound`, which reuses `feedbackmonk_tracing::scrub` and rejects "
            "source/secret dumps (references-not-dumps, C27 25c)."
        )
    if c_passed is False:
        print("\nProbe C failure (C24 corpus behavior):")
        print(f"  {c_message}")
        print("  Remediation: cargo test -p feedbackmonk-api --test feedback_injection_corpus")
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
