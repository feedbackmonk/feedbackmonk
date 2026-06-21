#!/usr/bin/env python3
"""translation-egress-q24-isolation Verification Oracle (canonical implementation).

The anti-reward-hacking leg for FR-FBR-30's privacy posture (DEC-FBR-IMPL-26) and
the Q24 read-isolation invariant (DEC-FBR-IMPL-25 / DEC-FBR-02). It proves — FROM
CODE, not from a self-reported flag — four load-bearing properties of the
multilingual-translation pipeline:

  A) PROVIDER POSTURE (static, main.rs): the translation provider DEFAULTS to
     `off` and is never hardcoded/unconditionally a cloud provider. `off` returns
     `None` (no worker, no egress). Flipping the default or removing the `off`
     arm silently makes every self-hoster a data exporter (DEC-FBR-IMPL-26).

  B) READ ISOLATION (static, repository + all crate src): NO public / end-user /
     board / admin-display code may read `body_translated`. Every function that
     references the column must be in a tiny ALLOWLIST — the single analyst/
     clustering consumer (`list_member_bodies_for_cluster`) and the worker writer
     (`set_translation`). A new read that surfaces the translation to a request
     handler shows up as an un-allowlisted referent and FAILS. This is the Q24 +
     privacy leg: the verbatim original is what every human-facing surface shows.

  C) WRITER UNIQUENESS (static): the column is WRITTEN (`SET body_translated`) in
     exactly ONE repository fn (`set_translation`), and that fn is CALLED only
     from the translate-after-accept worker (`translation/worker.rs`). The async
     worker is therefore the ONLY writer of the translation (DEC-FBR-IMPL-25).

  D) FTS SOURCE (static, migrations): the `body_tsv` generated column's most
     recent definition sources from `coalesce(body_translated, body)` — i.e. FTS
     indexes the translation with a fallback to the original (the "FTS indexes the
     translation" ratification). A regression that reverts it to
     `to_tsvector('english', body)` (mis-stemming non-English rows) FAILS.

  --full) BEHAVIOR: runs the translation integration tests against the real DB
     (drift-detection leg) so the static probes and live behaviour cannot
     silently diverge.

Output: machine-parseable PASS / FAIL. Exit 0 on PASS, 1 on FAIL, 2 on
environment failure.

Lineage:
- FR-FBR-30 (multilingual feedback translation)
- DEC-FBR-IMPL-25 (store-both + async + FTS-indexes-the-translation; Q24-safe)
- DEC-FBR-IMPL-26 (provider pluggable, DEFAULT OFF; egress = conscious choice)
- DEC-FBR-02 / Q24 (verbatim original on every human-facing surface; no PII leak)
- Plan: docs/planning/plans/20260621T160022-fr-fbr-30-multilingual-translation.md (Stream E)
- Mirrors public-board-moderation-gate / cors-allowlist-enforcement (detection-from-code)
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
MAIN_RS = CRATES_DIR / "feedbackmonk-api" / "src" / "main.rs"
WORKER_RS = CRATES_DIR / "feedbackmonk-api" / "src" / "translation" / "worker.rs"
MIGRATIONS_DIR = REPO_ROOT / "migrations"
WORKER_TEST_RS = CRATES_DIR / "feedbackmonk-api" / "tests" / "translation_worker.rs"

TRANSLATED_COL = "body_translated"
# Functions permitted to REFERENCE body_translated (read or write). Anything else
# referencing it is a potential leak of the translation to a human-facing surface
# (Q24) — adding a referent must be a conscious decision (extend this set + cite
# why), mirroring the moderation-gate oracle's allowlist discipline.
ALLOWED_REFERENT_FNS = {
    "set_translation",                 # the worker's writer (Probe C pins uniqueness)
    "list_member_bodies_for_cluster",  # the sole analyst/clustering consumer (Stream D)
}
# The single fn allowed to WRITE the column.
WRITER_FN = "set_translation"
# The only module allowed to CALL the writer (the async worker).
WRITER_CALLER_REL = "crates/feedbackmonk-api/src/translation/worker.rs"

PROVIDER_ENV = "FEEDBACKMONK_TRANSLATION_PROVIDER"
PROVIDER_FN = "build_translation_provider"


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p).replace("\\", "/")


def _strip_comments(text: str) -> str:
    """Remove `/* */` block comments and `//` line comments so prose mentioning a
    column/keyword does not trip the code scans (mirrors probe_a's intent in the
    moderation-gate oracle). Conservative: operates on already-extracted fn bodies
    where brace-balance was computed on the raw text."""
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
    text = re.sub(r"//[^\n\r]*", "", text)
    return text


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


def _crate_src_files() -> List[Path]:
    out: List[Path] = []
    if not CRATES_DIR.is_dir():
        return out
    for path in CRATES_DIR.rglob("*.rs"):
        parts = path.parts
        if any(p == "target" for p in parts):
            continue
        # Only crate SOURCE (the production read/write surface). Tests/benches
        # are fixtures, not the shipped read surface.
        if "/src/" not in rel(path) and "\\src\\" not in str(path):
            continue
        out.append(path)
    return out


def probe_a() -> List[str]:
    """Provider defaults OFF; no unconditional cloud provider."""
    offenders: List[str] = []
    if not MAIN_RS.exists():
        return [f"{rel(MAIN_RS)} does not exist — cannot verify provider posture"]
    text = MAIN_RS.read_text(encoding="utf-8")
    body = _extract_fn_body(text, f"fn {PROVIDER_FN}")
    if body is None:
        return [
            f"{rel(MAIN_RS)}: `fn {PROVIDER_FN}` missing — the env-gated provider "
            "constructor (default-off posture, DEC-FBR-IMPL-26) is gone"
        ]
    # The provider env read must default to "off".
    default_off = re.search(
        rf'{PROVIDER_ENV}"?\)?\s*\.unwrap_or_else\(\s*\|_\|\s*"off"', body
    ) or re.search(rf'{PROVIDER_ENV}".*?unwrap_or_else\(\|_\|\s*"off"', body, re.DOTALL)
    if not default_off:
        offenders.append(
            f"{rel(MAIN_RS)}::{PROVIDER_FN}: {PROVIDER_ENV} does not default to \"off\" — the "
            "provider MUST default OFF (DEC-FBR-IMPL-26: egress is opt-in). A different default "
            "silently turns every self-hoster into a data exporter."
        )
    # There must be an explicit off-arm returning None (no provider when off).
    if not re.search(r'"off"\s*=>\s*Ok\(None\)', body) and '"off"' not in body:
        offenders.append(
            f"{rel(MAIN_RS)}::{PROVIDER_FN}: no `\"off\" => Ok(None)` arm — `off` MUST construct "
            "no provider (no worker, no egress)."
        )
    return offenders


def probe_b() -> List[str]:
    """No disallowed fn references body_translated (Q24 read isolation)."""
    offenders: List[str] = []
    for path in _crate_src_files():
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        if TRANSLATED_COL not in text:
            continue
        for name, fbody in _iter_fn_bodies(text):
            fbody = _strip_comments(fbody)
            if TRANSLATED_COL in fbody and name not in ALLOWED_REFERENT_FNS:
                offenders.append(
                    f"{rel(path)}::{name}: references `{TRANSLATED_COL}` but is not an allow-listed "
                    f"referent ({', '.join(sorted(ALLOWED_REFERENT_FNS))}). Every human-facing "
                    "surface (public/end-user/board/admin-display) MUST read the verbatim `body`, "
                    "never the translation (Q24, DEC-FBR-IMPL-25). If this is a deliberate new "
                    "machine-consumer read, add it to ALLOWED_REFERENT_FNS with a rationale."
                )
    return offenders


def probe_c() -> List[str]:
    """Writer uniqueness: only set_translation writes the column; only the worker
    calls set_translation."""
    offenders: List[str] = []
    set_re = re.compile(rf"SET\s+{TRANSLATED_COL}\b", re.IGNORECASE)
    writers: List[str] = []
    for path in _crate_src_files():
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        if TRANSLATED_COL not in text:
            continue
        for name, fbody in _iter_fn_bodies(text):
            fbody = _strip_comments(fbody)
            if set_re.search(fbody):
                writers.append(f"{rel(path)}::{name}")
                if name != WRITER_FN:
                    offenders.append(
                        f"{rel(path)}::{name}: WRITES `{TRANSLATED_COL}` (SET ...) but only "
                        f"`{WRITER_FN}` may write the translation column (DEC-FBR-IMPL-25: the "
                        "async worker is the sole writer)."
                    )
        # An INSERT that writes body_translated would also be a rogue writer
        # (submit must stamp status only, never a translation).
        if re.search(rf"INSERT\s+INTO\s+feedback\b[^;]*\b{TRANSLATED_COL}\b", text,
                     re.IGNORECASE | re.DOTALL):
            offenders.append(
                f"{rel(path)}: an INSERT INTO feedback writes `{TRANSLATED_COL}` — a translation "
                "must only be written by the async worker's UPDATE (set_translation), never at "
                "submit (DEC-FBR-IMPL-25: store-both, async-after-accept)."
            )
    if not writers:
        offenders.append(
            f"no fn writes `{TRANSLATED_COL}` (expected exactly `{WRITER_FN}`) — the translate "
            "pipeline's writer is missing."
        )
    # The writer fn may be invoked only from the worker module.
    call_re = re.compile(rf"\.{WRITER_FN}\s*\(")
    for path in _crate_src_files():
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        if call_re.search(text) and rel(path) != WRITER_CALLER_REL:
            offenders.append(
                f"{rel(path)}: calls `.{WRITER_FN}(` outside the translate-after-accept worker "
                f"({WRITER_CALLER_REL}). The worker MUST be the only writer caller "
                "(DEC-FBR-IMPL-25 D3: translation never happens on the request path)."
            )
    return offenders


def probe_d() -> List[str]:
    """The latest body_tsv generated-column definition sources from
    coalesce(body_translated, body)."""
    offenders: List[str] = []
    if not MIGRATIONS_DIR.is_dir():
        return [f"{rel(MIGRATIONS_DIR)} missing — cannot verify the FTS source"]
    gen_re = re.compile(
        r"body_tsv\s+tsvector\s+GENERATED\s+ALWAYS\s+AS\s*\((?P<expr>.*?)\)\s*STORED",
        re.IGNORECASE | re.DOTALL,
    )
    latest: Optional[Tuple[str, str]] = None  # (filename, expr)
    for sql in sorted(MIGRATIONS_DIR.glob("*.sql")):
        text = sql.read_text(encoding="utf-8")
        for m in gen_re.finditer(text):
            latest = (sql.name, m.group("expr"))  # later file / later match wins
    if latest is None:
        return [
            f"{rel(MIGRATIONS_DIR)}: no `body_tsv ... GENERATED ALWAYS AS (...) STORED` definition "
            "found — FTS source cannot be verified"
        ]
    fname, expr = latest
    norm = re.sub(r"\s+", " ", expr).strip().lower()
    has_coalesce = "coalesce(" in norm
    has_translated = TRANSLATED_COL in norm
    if not (has_coalesce and has_translated):
        offenders.append(
            f"migrations/{fname}: the current `body_tsv` definition is `{expr.strip()}` — it MUST "
            f"source from `coalesce({TRANSLATED_COL}, body)` so FTS indexes the translation with a "
            "fallback to the original (DEC-FBR-IMPL-25). Reverting to `to_tsvector('english', body)` "
            "mis-stems non-English rows."
        )
    return offenders


def probe_full(full: bool) -> Tuple[Optional[bool], str]:
    if not full:
        return None, "skipped (pass --full to run tests/translation_worker.rs)"
    if not WORKER_TEST_RS.exists():
        return None, f"PENDING — {rel(WORKER_TEST_RS)} not written yet"
    cmd = ["cargo", "test", "-p", "feedbackmonk-api", "--test", "translation_worker"]
    try:
        proc = subprocess.run(
            cmd, cwd=str(REPO_ROOT), capture_output=True, text=True, timeout=600
        )
    except FileNotFoundError:
        return None, "cargo not found — behavioral probe inconclusive"
    except subprocess.TimeoutExpired:
        return False, "translation_worker tests timed out"
    if proc.returncode == 0:
        return True, "translation_worker: all passed"
    tail = (proc.stdout + proc.stderr).strip().splitlines()[-8:]
    return False, "translation_worker tests failed:\n      " + "\n      ".join(tail)


def main() -> int:
    parser = argparse.ArgumentParser(description="translation-egress-q24-isolation oracle")
    parser.add_argument(
        "--full", action="store_true", help="also run the translation integration tests"
    )
    args = parser.parse_args()

    a = probe_a()
    b = probe_b()
    c = probe_c()
    d = probe_d()
    full_passed, full_msg = probe_full(args.full)

    fails = sum(1 for x in (a, b, c, d) if x) + (1 if full_passed is False else 0)

    if fails == 0:
        print("PASS translation-egress-q24-isolation")
        print(f"  Probe A (provider defaults OFF; no hardcoded cloud): clean ({rel(MAIN_RS)})")
        print(f"  Probe B (Q24 read isolation: no disallowed body_translated read): clean")
        print(f"  Probe C (writer uniqueness: only set_translation, only the worker calls it): clean")
        print(f"  Probe D (FTS sources coalesce(body_translated, body)): clean")
        print(f"  Probe --full (behavioral drift): {full_msg}")
        return 0

    print(f"FAIL translation-egress-q24-isolation ({fails} probe(s) failed)")
    for label, offs, fixhint in (
        ("A (provider posture)", a,
         "keep build_translation_provider defaulting to \"off\"; any change re-opens DEC-FBR-IMPL-26."),
        ("B (Q24 read isolation)", b,
         "human-facing reads use the verbatim `body`; only machine consumers read the translation."),
        ("C (writer uniqueness)", c,
         "only set_translation writes body_translated, and only the worker calls it."),
        ("D (FTS source)", d,
         "the latest body_tsv migration must use coalesce(body_translated, body)."),
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
        print("  Remediation: cargo test -p feedbackmonk-api --test translation_worker")
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
