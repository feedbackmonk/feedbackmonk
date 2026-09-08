#!/usr/bin/env python3
"""pii-scrub-audit Verification Oracle (canonical implementation).

Two probes:
  A) No direct `tracing_subscriber::fmt(`, `tracing_subscriber::registry(`,
     or custom `impl ...Layer<...> for ...` OUTSIDE crates/feedbackmonk-tracing/.
     The PII scrubber installs the SOLE global subscriber via
     `feedbackmonk_tracing::install_global_subscriber`. Any other call site is a
     potential bypass of the scrubbing layer (FR-FBR-10).
  B) SHA-256 of CANONICAL_PATTERNS in
     `crates/feedbackmonk-tracing/src/scrubber.rs` matches
     `expected_hash.txt`. Pattern-set drift (a new pattern, a missing pattern,
     a tweaked regex, a re-ordered slice) surfaces as oracle FAIL.

Output: machine-parseable PASS / FAIL with file:line offenders + hash diff on
FAIL. Exit 0 on PASS, 1 on FAIL.

Invoked by oracle.sh (Unix + Git Bash on Windows). Python 3.8+ is the only
runtime dependency.

Spec: see manifest.json. Lineage: P1 plan §Oracle Pre-Build Plan, FR-FBR-10,
DEC-FBR-01 Persona D, D-FBR-02 three-leg defense pattern. Pattern set is the
byte-for-byte port from GitCellar's
`gitcellar-service/src/feedback_logs/scrubber.rs`.
"""
from __future__ import annotations

import hashlib
import re
import sys
from pathlib import Path
from typing import List, Optional, Tuple


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[2]
CRATES_DIR = REPO_ROOT / "crates"
TRACING_CRATE = CRATES_DIR / "feedbackmonk-tracing"
SCRUBBER_RS = TRACING_CRATE / "src" / "scrubber.rs"
EXPECTED_HASH = SCRIPT_DIR / "expected_hash.txt"


# Probe A: forbidden patterns outside the scrubber crate.
#
# We INTENTIONALLY use specific anchors here. `impl.*Layer.*for` (the
# brief's loose regex) would false-positive on `TraceLayer::new_for_http()`
# in tower-http usage. The tightened regex requires an actual
# `impl ... Layer<...> for ...` block opener.
PROBE_A_PATTERNS = [
    (re.compile(r"\btracing_subscriber::fmt\s*\("),                "tracing_subscriber::fmt()"),
    (re.compile(r"\btracing_subscriber::registry\s*\("),           "tracing_subscriber::registry()"),
    (re.compile(r"\bimpl\b[^;{]*\bLayer\s*<[^>]*>\s+for\b"),       "impl Layer<...> for ..."),
]


def strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
    text = re.sub(r"//[^\n\r]*", "", text)
    return text


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p).replace("\\", "/")


def probe_a() -> List[str]:
    """Forbidden tracing-subscriber setup outside crates/feedbackmonk-tracing/."""
    offenders: List[str] = []
    if not CRATES_DIR.exists():
        return offenders
    for path in CRATES_DIR.rglob("*.rs"):
        # Skip files inside the tracing crate or any target/ build output.
        try:
            path.relative_to(TRACING_CRATE)
            continue
        except ValueError:
            pass
        if any(part == "target" for part in path.parts):
            continue
        try:
            raw = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        # Comment-strip so a doc-string mentioning the patterns doesn't false-fire.
        content = strip_comments(raw)
        lines = content.splitlines()
        for i, line in enumerate(lines, start=1):
            for pat, label in PROBE_A_PATTERNS:
                if pat.search(line):
                    offenders.append(
                        f"{rel(path)}:{i}  forbidden tracing-subscriber setup '{label}' outside crates/feedbackmonk-tracing/"
                    )
    return offenders


# Probe B: canonical-pattern hash.
#
# Parses CANONICAL_PATTERNS from feedbackmonk-tracing/src/scrubber.rs and computes
# SHA-256 over the line-serialised `name\tregex\treplacement\n` for each tuple.
# Returns the hex digest + (count, list of (name, regex, replacement)) for
# diagnostics on mismatch.
TUPLE_RE = re.compile(
    r'\(\s*"(?P<name>[A-Za-z_][A-Za-z0-9_]*)"\s*,\s*r"(?P<regex>[^"]*)"\s*,\s*"(?P<replacement>[^"]*)"\s*\)'
)
SLICE_HEAD_RE = re.compile(
    r"CANONICAL_PATTERNS\s*:\s*&\s*\[\s*\(\s*&\s*str\s*,\s*&\s*str\s*,\s*&\s*str\s*\)\s*\]\s*=\s*&\s*\["
)


def extract_patterns(scrubber_text: str) -> List[Tuple[str, str, str]]:
    # Do NOT strip comments here: the DSN regex literal contains `//` and a
    # naive line-comment stripper would mangle it. Comments inside the
    # CANONICAL_PATTERNS slice body are not idiomatic anyway.
    text = scrubber_text
    m = SLICE_HEAD_RE.search(text)
    if not m:
        return []
    # Locate the matching closing `]` for the slice body.
    start = m.end()
    depth = 1
    end = -1
    i = start
    while i < len(text):
        c = text[i]
        if c == "[":
            depth += 1
        elif c == "]":
            depth -= 1
            if depth == 0:
                end = i
                break
        i += 1
    if end < 0:
        return []
    body = text[start:end]
    return [(m.group("name"), m.group("regex"), m.group("replacement")) for m in TUPLE_RE.finditer(body)]


def canonical_serialise(patterns: List[Tuple[str, str, str]]) -> bytes:
    """Stable serialisation: <name>\\t<regex>\\t<replacement>\\n per row, UTF-8.

    NEVER change this format without bumping `expected_hash.txt`. Drift here
    silently invalidates the hash. The format is intentionally trivial so
    Rust-side tests can reproduce it identically.
    """
    parts = []
    for name, regex, replacement in patterns:
        parts.append(f"{name}\t{regex}\t{replacement}\n")
    return "".join(parts).encode("utf-8")


def probe_b() -> Optional[str]:
    """Returns None on PASS, error message on FAIL."""
    if not SCRUBBER_RS.exists():
        return f"missing scrubber source: {rel(SCRUBBER_RS)}"
    if not EXPECTED_HASH.exists():
        return f"missing expected hash file: {rel(EXPECTED_HASH)}"

    patterns = extract_patterns(SCRUBBER_RS.read_text(encoding="utf-8"))
    if not patterns:
        return f"no CANONICAL_PATTERNS tuples parsed from {rel(SCRUBBER_RS)} (expected static &[(&str, &str, &str)])"

    actual_hash = hashlib.sha256(canonical_serialise(patterns)).hexdigest()
    expected = EXPECTED_HASH.read_text(encoding="utf-8").strip().lower()

    if expected == "placeholder" or not expected:
        return f"expected_hash.txt is unfilled (got: {expected!r}); current scrubber hash is {actual_hash} (record this once pattern set is finalised)"

    if actual_hash != expected:
        return (
            f"pattern-set hash drift: actual={actual_hash} expected={expected} "
            f"(parsed {len(patterns)} patterns; review every tuple in {rel(SCRUBBER_RS)})"
        )

    return None


def main() -> int:
    if not TRACING_CRATE.exists():
        # Vacuous pass during early P1 Stage 1 commit ordering: the oracle is
        # built BEFORE the crate. Once the crate lands, the freshness trigger
        # re-invalidates and the probes run for real.
        print("PASS pii-scrub-audit (crates/feedbackmonk-tracing/ not yet present - vacuous pass)")
        return 0

    a = probe_a()
    b_err = probe_b()

    fails = len(a) + (1 if b_err else 0)
    if fails == 0:
        print("PASS pii-scrub-audit")
        print("  Probe A (no tracing setup outside crates/feedbackmonk-tracing/): clean")
        print("  Probe B (CANONICAL_PATTERNS hash matches expected_hash.txt): clean")
        return 0

    print(f"FAIL pii-scrub-audit ({fails} offender(s))")
    if a:
        print()
        print("Probe A offenders (forbidden tracing-subscriber setup outside crates/feedbackmonk-tracing/):")
        for o in a:
            print(f"  {o}")
    if b_err:
        print()
        print("Probe B failure (canonical pattern-set hash):")
        print(f"  {b_err}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
