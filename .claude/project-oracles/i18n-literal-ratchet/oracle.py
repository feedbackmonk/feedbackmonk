#!/usr/bin/env python3
"""i18n-literal-ratchet Verification Oracle -- v1.0.0.

Assertion: see manifest.json `assertion` (frozen before this probe existed).

Static scan for hard-coded, user-facing English literals in the two localized
surfaces:

  widget/src/**/*.ts   -- textContent/innerText assignments, createTextNode(),
                          setAttribute("aria-label"|"title"|"placeholder", ...),
                          notify(...)
  admin-ui/src/**/*.tsx -- JSX text nodes (`>text<` containing a letter),
                          aria-label=/title=/placeholder= string attributes,
                          notify("...")

Each match is identified by (file, normalized text) -- NOT (file, line), since
line numbers drift with unrelated edits but the ratchet cares about content.
The current match set must be a SUBSET of i18n/literal-baseline.json; the
baseline shrinks only, via `--freeze` (refuses when the set grew).

Exit 0 PASS (subset of baseline, or baseline absent -- vacuous), 1 FAIL (new
literals beyond the baseline), 2 environment failure.
`--freeze`: exit 0 if written (baseline absent, or the new set did not grow),
1 if refused (the set grew -- nothing written).

Python 3.8+, stdlib only.
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

ROOT = Path(__file__).resolve().parents[3]
WIDGET_SRC = ROOT / "widget" / "src"
ADMIN_SRC = ROOT / "admin-ui" / "src"
BASELINE_PATH = ROOT / "i18n" / "literal-baseline.json"

# Brand names, and other exemptions the manifest names explicitly. Kept
# conservative and reviewed against a live scan of this tree (see README
# "measured false-positive rate").
ALLOWED_LITERALS = {
    "feedbackmonk", "FeedbackMonk", "GitCellar", "DeepL",
    # notify()'s ToastKind enum (admin-ui/src/components/Toast.tsx) -- the
    # severity-level second argument, never display text. Measured: when
    # notify's first arg is a variable (`notify(msg, "error")`), _literal_after
    # has no earlier literal to prefer and grabs this instead.
    "info", "success", "error",
}
SINGLE_PUNCTUATION = {"×", "…", "-", "–", "—", "·", "/", "|", ":", "*", "+", "="}
URL_RE = re.compile(r"^https?://\S+$")
# A dotted lower-camelCase identifier ("widget.attach.listAria") is a catalog
# KEY passed to t(), never real display prose -- real English text always
# has a space, capital start, or punctuation. Filters the single largest
# false-positive class measured in this tree: `el.setAttribute("aria-label",
# t("widget.foo.bar"))` grabs "widget.foo.bar" as the first quoted literal
# after the trigger unless excluded here.
TRANSLATION_KEY_SHAPE_RE = re.compile(r"^[a-z][a-zA-Z0-9]*(\.[a-zA-Z0-9]+){1,6}$")
STRING_LITERAL_RE = re.compile(r"(['\"])((?:\\.|(?!\1).)*)\1")

TEXTCONTENT_TRIGGER = re.compile(r"\.(?:textContent|innerText)\s*=")
CREATE_TEXT_NODE_TRIGGER = re.compile(r"\bcreateTextNode\(")
NOTIFY_TRIGGER = re.compile(r"\bnotify\(")
SET_ATTRIBUTE_TRIGGER = re.compile(r"\.setAttribute\(\s*(['\"])(aria-label|title|placeholder)\1\s*,")

# Excludes `=`, `&`, `|` from the captured span: without this, a plain TS
# comparison like `rung >= 0 && rung <= 3` reads as JSX text between a `>`
# and a `<` (measured: WorkOrderDetail.tsx, WorkOrderList.tsx). Real JSX
# prose essentially never needs a literal `=`/`&`/`|` in raw source (an
# ampersand in real copy is written `&amp;` in JSX text anyway).
JSX_TEXT_NODE_RE = re.compile(r">([^<>{}\n=&|]*[A-Za-z][^<>{}\n=&|]*)<")
JSX_ATTR_RE = re.compile(r"\b(?:aria-label|title|placeholder)=(['\"])((?:(?!\1).)*)\1")


def normalize(text: str) -> str:
    return re.sub(r"\s+", " ", text).strip()


def is_allowed(text: str) -> bool:
    if not text:
        return True
    if len(text) == 1 and (text in SINGLE_PUNCTUATION or not text.isalnum()):
        return True
    if URL_RE.match(text):
        return True
    if text in ALLOWED_LITERALS:
        return True
    if TRANSLATION_KEY_SHAPE_RE.match(text):
        return True
    return False


def _line_of(content: str, pos: int) -> int:
    return content.count("\n", 0, pos) + 1


def _literal_after(content: str, start: int) -> Optional[Tuple[str, int]]:
    """First `'`/`"`-quoted string literal from `start` to the end of the
    statement (next `;` or newline, whichever comes first) -- but if a
    template literal (a backtick string) opens FIRST, the real argument is a
    template (very likely interpolated), so this returns None rather than
    leaping past it to an unrelated later argument (measured:
    notify(`...${x}...`, "success") was grabbing the severity string
    "success" as if it were the message, because it is the first
    plain-quoted literal in the statement)."""
    stop = len(content)
    semi = content.find(";", start)
    nl = content.find("\n", start)
    for idx in (semi, nl):
        if idx != -1:
            stop = min(stop, idx)
    window = content[start:stop]
    positions = [(window.find(q), q) for q in ("'", '"', "`")]
    positions = [(p, q) for p, q in positions if p != -1]
    if not positions:
        return None
    pos, quote_char = min(positions, key=lambda pq: pq[0])
    if quote_char == "`":
        return None
    m = STRING_LITERAL_RE.match(window, pos)
    if not m:
        return None
    return m.group(2), start + pos


def scan_widget_ts(content: str) -> List[Tuple[int, str]]:
    hits: List[Tuple[int, str]] = []
    for trigger in (TEXTCONTENT_TRIGGER, CREATE_TEXT_NODE_TRIGGER, NOTIFY_TRIGGER):
        for m in trigger.finditer(content):
            found = _literal_after(content, m.end())
            if found:
                text, pos = found
                hits.append((_line_of(content, pos), text))
    for m in SET_ATTRIBUTE_TRIGGER.finditer(content):
        found = _literal_after(content, m.end())
        if found:
            text, pos = found
            hits.append((_line_of(content, pos), text))
    return hits


def scan_admin_tsx(content: str) -> List[Tuple[int, str]]:
    hits: List[Tuple[int, str]] = []
    for m in JSX_TEXT_NODE_RE.finditer(content):
        hits.append((_line_of(content, m.start(1)), m.group(1)))
    for m in JSX_ATTR_RE.finditer(content):
        hits.append((_line_of(content, m.start(2)), m.group(2)))
    for m in NOTIFY_TRIGGER.finditer(content):
        found = _literal_after(content, m.end())
        if found:
            text, pos = found
            hits.append((_line_of(content, pos), text))
    return hits


def scan_tree() -> Dict[str, List[Tuple[int, str]]]:
    """rel-posix-path -> [(line, normalized_text), ...] for every non-allowed
    literal found, deduplicated within a file (same text, first line kept)."""
    results: Dict[str, List[Tuple[int, str]]] = {}
    if WIDGET_SRC.exists():
        for path in sorted(WIDGET_SRC.rglob("*.ts")):
            if path.name.endswith(".test.ts") or path.name == "locales.gen.ts":
                continue
            content = path.read_text(encoding="utf-8")
            hits = scan_widget_ts(content)
            _accumulate(results, path, hits)
    if ADMIN_SRC.exists():
        for path in sorted(ADMIN_SRC.rglob("*.tsx")):
            if path.name.endswith(".test.tsx"):
                continue
            content = path.read_text(encoding="utf-8")
            hits = scan_admin_tsx(content)
            _accumulate(results, path, hits)
    return results


def _accumulate(results: Dict[str, List[Tuple[int, str]]], path: Path, hits: List[Tuple[int, str]]) -> None:
    rel = path.relative_to(ROOT).as_posix()
    by_text: Dict[str, int] = {}
    for line, raw_text in hits:
        text = normalize(raw_text)
        if is_allowed(text):
            continue
        if text not in by_text:
            by_text[text] = line
    if by_text:
        results.setdefault(rel, [])
        for text, line in by_text.items():
            results[rel].append((line, text))


def load_baseline() -> Optional[Dict[str, List[str]]]:
    if not BASELINE_PATH.exists():
        return None
    return json.loads(BASELINE_PATH.read_text(encoding="utf-8"))


def as_pairs(matches: Dict[str, List[Tuple[int, str]]]) -> Set[Tuple[str, str]]:
    return {(f, text) for f, hits in matches.items() for _, text in hits}


def baseline_pairs(baseline: Dict[str, List[str]]) -> Set[Tuple[str, str]]:
    return {(f, text) for f, texts in baseline.items() for text in texts}


def write_baseline(matches: Dict[str, List[Tuple[int, str]]]) -> None:
    data = {f: sorted({text for _, text in hits}) for f, hits in sorted(matches.items())}
    BASELINE_PATH.write_text(
        json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")


def self_test() -> int:
    """Invertible: must catch a real literal and must NOT flag the three
    measured false-positive shapes (translation key, notify severity enum,
    JSX comparison operators). Exercises scan_widget_ts/scan_admin_tsx
    directly -- no filesystem, no i18n/literal-baseline.json involved."""
    ok = True

    def check(name: str, condition: bool) -> None:
        nonlocal ok
        print(f"  self-test [{name}]: {'PASS' if condition else 'FAIL'}")
        if not condition:
            ok = False

    widget_src = (
        'el.setAttribute("aria-label", t("widget.attach.listAria"));\n'
        'it.redactBtn.textContent = prev || "Redact";\n'
    )
    widget_hits = {normalize(t) for _, t in scan_widget_ts(widget_src)}
    widget_hits = {t for t in widget_hits if not is_allowed(t)}
    check("real widget literal detected (\"Redact\")", "Redact" in widget_hits)
    check("translation-key shape excluded (\"widget.attach.listAria\")",
          "widget.attach.listAria" not in widget_hits)

    notify_src = 'notify(`Token "${token.label}" revoked.`, "success");\n'
    notify_hits = {normalize(t) for _, t in scan_widget_ts(notify_src)}
    notify_hits = {t for t in notify_hits if not is_allowed(t)}
    check("notify() severity enum excluded, template-literal message not misattributed",
          "success" not in notify_hits and not notify_hits)

    admin_src = (
        "  if (rung >= 0 && rung <= 3) return X;\n"
        "  return <p>Genuine paragraph text here</p>;\n"
    )
    admin_hits = {normalize(t) for _, t in scan_admin_tsx(admin_src)}
    admin_hits = {t for t in admin_hits if not is_allowed(t)}
    check("JSX comparison-operator false positive excluded", "= 0 && rung" not in admin_hits)
    check("genuine JSX text still detected", "Genuine paragraph text here" in admin_hits)

    print("self-test:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def main(argv: List[str]) -> int:
    if "--self-test" in argv:
        return self_test()
    freeze = "--freeze" in argv
    matches = scan_tree()
    current = as_pairs(matches)
    baseline = load_baseline()

    if freeze:
        if baseline is not None:
            new = current - baseline_pairs(baseline)
            if new:
                print(f"FAIL i18n-literal-ratchet --freeze refused: {len(new)} new literal(s) beyond baseline")
                for f, text in sorted(new):
                    print(f"    {f}: \"{text}\"")
                return 1
        write_baseline(matches)
        print(f"i18n-literal-ratchet: baseline written ({len(current)} literal(s) across {len(matches)} file(s))")
        return 0

    if baseline is None:
        print("PASS i18n-literal-ratchet (vacuous -- i18n/literal-baseline.json absent; run --freeze to establish it)")
        return 0

    new = current - baseline_pairs(baseline)
    if not new:
        print(f"PASS i18n-literal-ratchet ({len(current)} literal(s), all in baseline)")
        return 0

    print(f"FAIL {len(new)} i18n-literal-ratchet")
    line_lookup = {(f, text): line for f, hits in matches.items() for line, text in hits}
    for f, text in sorted(new):
        line = line_lookup.get((f, text), "?")
        print(f'  {f}:{line}: "{text}"')
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Exception as exc:  # noqa: BLE001 -- environment failure is exit 2 by contract
        print(f"FAIL i18n-literal-ratchet (environment: {exc})")
        sys.exit(2)
