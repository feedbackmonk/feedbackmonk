#!/usr/bin/env python3
"""pii-scrub-audit Verification Oracle.

Question: is every log line emitted through the PII scrubber? The scrubber is
installed as the SOLE global subscriber by
`feedbackmonk_tracing::install_global_subscriber` (FR-FBR-10); a subscriber
built or installed anywhere else writes unscrubbed lines. So no crate but
`feedbackmonk-tracing` may build or install one:

  tracing_subscriber::fmt(   tracing_subscriber::fmt::init / ::fmt::Subscriber
  tracing_subscriber::registry(   FmtSubscriber   SubscriberBuilder
  set_global_default(   set_default(   with_default(   `.try_init()` / `.init()` on a builder
  impl ... Layer<...> for ...

Test code is scanned too: a test that installs its own subscriber is how an
unscrubbed pattern gets copied into production. Comments are stripped first.

The pattern-set drift check this oracle used to carry (Probe B) is a test now:
`crates/feedbackmonk-tracing/tests/scrubber_patterns.rs` against
`tests/canonical_pattern_hash.txt`.

Exit 0 PASS, 1 FAIL (file:line offenders), 2 environment error.
Usage: python oracle.py [--root <repo>]
"""
from __future__ import annotations

import re
import sys
from pathlib import Path
from typing import List


FORBIDDEN = [
    (re.compile(r"\btracing_subscriber::fmt\b"), "tracing_subscriber::fmt"),
    (re.compile(r"\btracing_subscriber::registry\s*\("), "tracing_subscriber::registry()"),
    (re.compile(r"\bFmtSubscriber\b"), "FmtSubscriber"),
    (re.compile(r"\bSubscriberBuilder\b"), "SubscriberBuilder"),
    (re.compile(r"\bset_global_default\s*\("), "set_global_default()"),
    (re.compile(r"\btracing::subscriber::(set_default|with_default)\s*\("), "tracing::subscriber::set_default/with_default()"),
    (re.compile(r"\bimpl\b[^;{]*\bLayer\s*<[^>]*>\s+for\b"), "impl Layer<...> for ..."),
]
# `.init()` / `.try_init()` is only a subscriber install when the file uses
# tracing_subscriber at all; elsewhere it is an ordinary method name.
INIT = re.compile(r"\.\s*(try_)?init\s*\(\s*\)")
USES_SUBSCRIBER = re.compile(r"\btracing_subscriber\b")


def strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
    return re.sub(r"//[^\n\r]*", "", text)


def scan(root: Path) -> List[str]:
    crates = root / "crates"
    tracing_crate = crates / "feedbackmonk-tracing"
    offenders: List[str] = []
    for path in sorted(crates.rglob("*.rs")):
        if path.is_relative_to(tracing_crate) or "target" in path.parts:
            continue
        code = strip_comments(path.read_text(encoding="utf-8", errors="replace"))
        uses = bool(USES_SUBSCRIBER.search(code))
        rel = path.relative_to(root).as_posix()
        for i, line in enumerate(code.splitlines(), start=1):
            labels = [label for pat, label in FORBIDDEN if pat.search(line)]
            if uses and INIT.search(line):
                labels.append(".init() in a file using tracing_subscriber")
            offenders += [f"{rel}:{i}  {label}" for label in labels]
    return offenders


def main(argv: List[str]) -> int:
    root = Path(argv[argv.index("--root") + 1]) if "--root" in argv else Path(__file__).resolve().parents[3]
    if not (root / "crates" / "feedbackmonk-tracing" / "src" / "lib.rs").is_file():
        print(f"FAIL pii-scrub-audit (no crates/feedbackmonk-tracing under {root})")
        return 2
    offenders = scan(root)
    if not offenders:
        print("PASS pii-scrub-audit: no subscriber built or installed outside crates/feedbackmonk-tracing")
        return 0
    print(f"FAIL pii-scrub-audit ({len(offenders)} offender(s)) -- a subscriber outside the scrubber crate writes unscrubbed lines:")
    for o in offenders:
        print(f"  {o}")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
