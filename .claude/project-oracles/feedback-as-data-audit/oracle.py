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

THREE probes (detection-from-code; comments and #[cfg(test)] modules stripped):

  A) ENVELOPE: prompt.rs defines `wrap_untrusted`; no other runner file names the
     envelope delimiters; `assemble` reads nothing of the recommendation before it
     builds `untrusted_envelope`, and routes it through
     wrap_untrusted(render_untrusted_block(..)); nothing else calls
     render_untrusted_block.

  B) EGRESS, per function: every `.runner_transition(..)` result_ref /
     failure_reason and every `.post_recommendation(..)` payload is `None` or a
     variable bound in the same function from sanitize_outbound / sanitize_clean /
     failure_reason_for_egress (the latter two must call sanitize_outbound); no
     file but client.rs names an HTTP client.

  C) CORPUS (--full): cargo test -p feedbackmonk-api --test feedback_injection_corpus.

What it does not see: text that enters the ClaimedOrder's trusted fields
(title, instructions) upstream in the API -- see
docs/planning/deferred/runner-recommendation-text-in-trusted-layer-20260930.md.

Output: machine-parseable PASS / FAIL. Exit 0 PASS, 1 FAIL, 2 environment.

Lineage:
- FR-FBR-25b (prompt data-envelope) / FR-FBR-25c (source-never-leaves)
- Contract C27 (P5b plan, FROZEN) + Testability Gate Flags 1 & 2
- C24 corpus (feedback_injection_corpus.rs) cases (g)/(f)
- Probandurgy Verification Oracle pattern (canonical Python)
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


def _fn_bodies(text: str) -> List[Tuple[str, str]]:
    """Every `fn name ... { body }` in `text`, in order."""
    out: List[Tuple[str, str]] = []
    for m in re.finditer(r"\bfn\s+(\w+)", text):
        body = _extract_fn_body(text[m.start():], "fn " + m.group(1))
        if body is not None:
            out.append((m.group(1), body))
    return out


def _call_args(body: str, callee: str) -> List[List[str]]:
    """The top-level argument list of every `.callee(` call in `body`."""
    calls: List[List[str]] = []
    for m in re.finditer(r"\.\s*" + re.escape(callee) + r"\s*\(", body):
        depth, args, cur = 1, [], ""
        for c in body[m.end():]:
            if c in "([{":
                depth += 1
            elif c in ")]}":
                depth -= 1
                if depth == 0:
                    break
            if c == "," and depth == 1:
                args.append(cur.strip())
                cur = ""
            else:
                cur += c
        if cur.strip():
            args.append(cur.strip())
        calls.append(args)
    return calls


def _without_test_modules(text: str) -> str:
    """Drop `#[cfg(test)] mod ... { }` blocks: tests call the sinks with fixtures."""
    out, i = "", 0
    for m in re.finditer(r"#\[cfg\(test\)\]\s*mod\s+\w+\s*\{", text):
        if m.start() < i:
            continue
        out += text[i:m.start()]
        depth, j = 1, m.end()
        while j < len(text) and depth:
            depth += {"{": 1, "}": -1}.get(text[j], 0)
            j += 1
        i = j
    return out + text[i:]


def _runner_sources() -> List[Tuple[Path, str]]:
    return [(p, _without_test_modules(_strip_comments(p.read_text(encoding="utf-8"))))
            for p in sorted(RUNNER_SRC.rglob("*.rs"))]


def probe_a() -> List[str]:
    """Feedback-derived text enters the prompt only inside the envelope."""
    offenders: List[str] = []
    if not PROMPT_RS.exists():
        return [f"{rel(PROMPT_RS)} missing"]
    prompt = _strip_comments(PROMPT_RS.read_text(encoding="utf-8"))
    if CHOKEPOINT_FN not in prompt:
        offenders.append(f"{rel(PROMPT_RS)}: `{CHOKEPOINT_FN}` (the envelope chokepoint) is gone")
    # The envelope delimiters exist in prompt.rs alone: another file building an
    # envelope is a second, unaudited path into the prompt.
    for path, code in _runner_sources():
        if path == PROMPT_RS:
            continue
        for token in ("untrusted-feedback-data", "ENVELOPE_OPEN", "ENVELOPE_CLOSE"):
            if token in code:
                offenders.append(f"{rel(path)}: names `{token}` -- only prompt.rs may build the envelope")
    assemble = _extract_fn_body(prompt, "pub fn assemble")
    if assemble is None:
        return offenders + [f"{rel(PROMPT_RS)}: `pub fn assemble` not found"]
    # The trusted layer is everything `assemble` builds before the envelope; it
    # must not read the recommendation (feedback-derived, model-summarised).
    split = assemble.find("untrusted_envelope")
    trusted = assemble[:split] if split >= 0 else assemble
    if split < 0:
        offenders.append(f"{rel(PROMPT_RS)}: `assemble` builds no `untrusted_envelope`")
    if re.search(r"\brecommendation\b|\brec\.", trusted):
        offenders.append(f"{rel(PROMPT_RS)}: `assemble` reads the recommendation in the trusted instruction layer")
    if not re.search(r"wrap_untrusted\s*\(\s*&?\s*render_untrusted_block\s*\(", assemble):
        offenders.append(f"{rel(PROMPT_RS)}: `assemble` does not route the recommendation through "
                         "wrap_untrusted(render_untrusted_block(..))")
    for name, body in _fn_bodies(prompt):
        if name not in ("assemble", "render_untrusted_block") and re.search(r"\brender_untrusted_block\s*\(", body):
            offenders.append(f"{rel(PROMPT_RS)}: `{name}` calls render_untrusted_block outside the envelope")
    return offenders


# The runner's only ways out, and which arguments of each carry content:
# runner_transition(work_order_id, event_type, result_ref, failure_reason).
OUTBOUND = {"runner_transition": (2, 3), "post_recommendation": (0,)}
# Functions whose result has passed the egress chokepoint.
CLEANERS = ("sanitize_outbound", "sanitize_clean", "failure_reason_for_egress")


def probe_b() -> List[str]:
    """Every outbound payload is None or was produced by the egress chokepoint in
    the same function, and nothing but client.rs opens an HTTP path."""
    if not SANITIZER_RS.exists() or EGRESS_FN not in _strip_comments(SANITIZER_RS.read_text(encoding="utf-8")):
        return [f"{rel(SANITIZER_RS)}: `{EGRESS_FN}` (the egress chokepoint) is gone"]
    offenders: List[str] = []
    sites = 0
    cleaners_seen = {}
    for path, code in _runner_sources():
        for name, body in _fn_bodies(code):
            if name in CLEANERS[1:]:
                cleaners_seen[name] = "sanitize_outbound(" in body
        if path.name == "client.rs":
            continue
        if re.search(r"\breqwest\b|\bhyper\b|\bureq\b", code):
            offenders.append(f"{rel(path)}: opens its own HTTP path -- only client.rs may, behind the chokepoint")
        for name, body in _fn_bodies(code):
            for callee, positions in OUTBOUND.items():
                for args in _call_args(body, callee):
                    sites += 1
                    for pos in positions:
                        arg = args[pos] if pos < len(args) else ""
                        if arg == "None":
                            continue
                        var = re.sub(r"^Some\(\s*&?\s*|\)$|^&\s*", "", arg).strip()
                        bound = re.search(r"\blet\s+(?:mut\s+)?" + re.escape(var) + r"\s*(?::[^=]+)?=\s*([^;]*)", body)
                        if not (re.fullmatch(r"\w+", var) and bound and any(c + "(" in bound.group(1) for c in CLEANERS)):
                            offenders.append(f"{rel(path)}::{name}: `.{callee}(..)` sends `{arg}` without the egress chokepoint")
    for name in CLEANERS[1:]:
        if name not in cleaners_seen:
            offenders.append(f"{rel(RUNNER_SRC)}: `{name}` not found")
        elif not cleaners_seen[name]:
            offenders.append(f"{rel(RUNNER_SRC)}: `{name}` no longer calls sanitize_outbound")
    if sites == 0:
        offenders.append(f"no outbound call sites found under {rel(RUNNER_SRC)} -- the scan is broken")
    return offenders


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

    a_offenders = probe_a()
    b_offenders = probe_b()
    c_passed, c_message = probe_c(args.full)

    fails = (
        (1 if a_offenders else 0)
        + (1 if b_offenders else 0)
        + (1 if c_passed is False else 0)
    )

    if fails == 0:
        print("PASS feedback-as-data-audit")
        print(f"  Probe A (prompt data-envelope): clean -- recommendation text only inside wrap_untrusted ({rel(PROMPT_RS)})")
        print("  Probe B (egress): clean -- every outbound payload is None or chokepoint-produced in its function")
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
