#!/usr/bin/env python3
"""approval-gate-enforcement Verification Oracle (canonical implementation).

THE security boundary between public internet input and code execution
(FR-FBR-25a / FR-FBR-22, Contract C22 invariant 1): no work order may reach an
EXECUTION state (>= dispatched) without a prior owner-authored `approved` event
in the `work_order_events` ledger.

The state-machine TABLE itself (is_execution_state / legal_transitions_from) is
proven by the core unit tests `execution_states_are_exactly_dispatch_and_after`
and `approval_is_the_only_gate_into_execution` in
crates/feedbackmonk-core/src/work_order.rs — this oracle does not repeat them.
What a unit test cannot prove is the ABSENCE of a bypass path; that is what the
probes below read from source (comments stripped, so a gate that has been
commented out does not count):

  A) GATE (static): inside `transition_work_order`
     (crates/feedbackmonk-api/src/handlers/work_orders.rs), at the top level of
     the function body and BEFORE the state write, a transition whose target
     `to.is_execution_state()` is refused with `ApprovalRequired` unless
     `state.work_order_events.has_approved_event(scope, work_order_id)` is true;
     `to` is not rebound after the gate, and the write records `to_state: to`.

  B) SOLE WRITER (static): across every crate's production source
     (crates/*/src, `#[cfg(test)]` items excluded):
       - an SQL `UPDATE work_orders ... SET ... state =` appears ONLY in the
         repository's `transition_in_executor` / `update_state_in_executor`;
       - no `UPDATE work_orders` SQL at all outside the repository crate;
       - no `INSERT INTO work_orders` names the `state` column (a work order is
         born `draft` by the column default, migration 00014);
       - `.transition_in_executor(` is called only from `transition_work_order`;
       - `.update_state_in_executor(` (the unpaired, test-only setter) is called
         from no production code at all.

  C) BEHAVIOR (--full): runs tests/work_order_state_machine.rs against the real
     DB (the bypass-resistance corpus). A missing test file is a FAIL.

Exit 0 PASS, 1 FAIL, 2 environment failure.

Lineage: FR-FBR-25a / FR-FBR-22; Contract C22 inv. 1 + 3; P5a Testability Gate
Flag 1; DEC-FBR-IMPL-03 (canonical Python).
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path
from typing import Dict, List, Optional, Tuple

SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_ROOT = SCRIPT_DIR.parents[2]

HANDLER_REL = "crates/feedbackmonk-api/src/handlers/work_orders.rs"
REPO_WO_REL = "crates/feedbackmonk-repository/src/work_orders.rs"
REPO_CRATE_REL = "crates/feedbackmonk-repository/"
SM_TEST_REL = "crates/feedbackmonk-api/tests/work_order_state_machine.rs"

GATE_FN = "transition_work_order"
# Repository fns permitted to carry the `SET state =` SQL.
STATE_WRITER_FNS = {"transition_in_executor", "update_state_in_executor"}

# The gate, whitespace-insensitive, on comment-stripped source.
GATE_RE = re.compile(
    r"\bif\s+to\s*\.\s*is_execution_state\s*\(\s*\)\s*&&\s*!\s*state\s*"
    r"\.\s*work_order_events\s*\.\s*has_approved_event\s*\(\s*scope\s*,\s*"
    r"work_order_id\s*\)\s*\.\s*await\s*\?\s*\{\s*return\s+Err\s*\(\s*"
    r"TransitionFailure\s*::\s*Rule\s*\(\s*WorkOrderTransitionError\s*::\s*"
    r"ApprovalRequired\b"
)


# --------------------------------------------------------------------------
# Rust lexing: produce two same-length views of a file.
#   code      -- comments blanked (strings intact)
#   skeleton  -- comments AND string/char literal contents blanked
# Offsets are identical across views; newlines are preserved.
# --------------------------------------------------------------------------
_BLANK_RE = re.compile(r"[^\n]")


def _blank(s: str) -> str:
    return _BLANK_RE.sub(" ", s)


_TOKEN_RE = re.compile(
    r"//|/\*|(?<![\w])b?r(#*)\"|(?<![\w])b?\"|'(?:\\[^']{1,10}|[^\\'\n])'"
)


def lex(text: str) -> Tuple[str, str, List[Tuple[int, int]]]:
    """Return (code, skeleton, string_spans) -- spans are (start, end) of each
    string literal's CONTENT. Lifetimes (`'a`) are left alone."""
    code: List[str] = []
    skel: List[str] = []
    spans: List[Tuple[int, int]] = []
    i, n = 0, len(text)
    while i < n:
        m = _TOKEN_RE.search(text, i)
        if not m:
            code.append(text[i:]); skel.append(text[i:]); break
        t0 = m.start()
        if t0 > i:
            code.append(text[i:t0]); skel.append(text[i:t0])
        tok = m.group(0)
        if tok == "//":
            j = text.find("\n", t0)
            j = n if j == -1 else j
            bl = _blank(text[t0:j]); code.append(bl); skel.append(bl); i = j
        elif tok == "/*":
            depth, j = 0, t0
            while j < n:
                if text.startswith("/*", j):
                    depth += 1; j += 2
                elif text.startswith("*/", j):
                    depth -= 1; j += 2
                    if depth == 0:
                        break
                else:
                    j += 1
            bl = _blank(text[t0:j]); code.append(bl); skel.append(bl); i = j
        elif tok.startswith("'"):
            code.append(tok); skel.append("'" + _blank(tok[1:-1]) + "'"); i = m.end()
        elif m.group(1) is not None:  # raw string
            hashes = m.group(1)
            start = m.end()
            end = text.find('"' + hashes, start)
            end = n if end == -1 else end
            close = min(n, end + 1 + len(hashes))
            code.append(text[t0:close])
            skel.append(text[t0:start] + _blank(text[start:end]) + text[end:close])
            spans.append((start, end)); i = close
        else:  # "..." / b"..."
            start = m.end()
            j = start
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            j = min(j, n - 1)
            code.append(text[t0:j + 1])
            skel.append(text[t0:start] + _blank(text[start:j]) + text[j:j + 1])
            spans.append((start, j)); i = j + 1
    return "".join(code), "".join(skel), spans


def match_pairs(skel: str, o: str, cl: str) -> Dict[int, int]:
    """open-offset -> close-offset for every balanced pair (one pass)."""
    out: Dict[int, int] = {}
    stack: List[int] = []
    for m in re.finditer(re.escape(o) + "|" + re.escape(cl), skel):
        if m.group(0) == o:
            stack.append(m.start())
        elif stack:
            out[stack.pop()] = m.start()
    return out


def fn_spans(skel: str) -> List[Tuple[str, int, int]]:
    """(name, body_open, body_close) for every fn WITH a body. Generic
    signatures (`fn x<'t>(`) are handled: the arg list is the first `(`
    after the name, balanced."""
    braces = match_pairs(skel, "{", "}")
    parens = match_pairs(skel, "(", ")")
    out = []
    for m in re.finditer(r"(?<!\w)fn\s+(\w+)", skel):
        p = skel.find("(", m.end())
        if p == -1 or p not in parens:
            continue
        bm = re.compile(r"[{;]").search(skel, parens[p] + 1)
        if not bm or bm.group(0) == ";" or bm.start() not in braces:
            continue
        out.append((m.group(1), bm.start(), braces[bm.start()]))
    return out


def cfg_test_ranges(skel: str) -> List[Tuple[int, int]]:
    braces = match_pairs(skel, "{", "}")
    out = []
    for m in re.finditer(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]", skel):
        bm = re.compile(r"[{;]").search(skel, m.end())
        if bm and bm.group(0) == "{" and bm.start() in braces:
            out.append((m.start(), braces[bm.start()]))
    return out


def enclosing_fn(spans, off: int) -> Optional[str]:
    best = None
    for name, a, b in spans:
        if a <= off <= b and (best is None or a > best[1]):
            best = (name, a)
    return best[0] if best else None


def line_of(text: str, off: int) -> int:
    return text.count("\n", 0, off) + 1


# --------------------------------------------------------------------------
class Ctx:
    def __init__(self, root: Path):
        self.root = root
        self.cache: Dict[str, Tuple[str, str, list]] = {}

    def rel(self, p: Path) -> str:
        try:
            return str(p.relative_to(self.root)).replace("\\", "/")
        except ValueError:
            return str(p)

    def load(self, p: Path):
        k = str(p)
        if k not in self.cache:
            self.cache[k] = lex(p.read_text(encoding="utf-8"))
        return self.cache[k]


def probe_a(ctx: Ctx) -> List[str]:
    """The approval gate inside transition_work_order."""
    path = ctx.root / HANDLER_REL
    if not path.exists():
        return [f"{HANDLER_REL} does not exist — the transition core is missing"]
    code, skel, _ = ctx.load(path)
    fns = [s for s in fn_spans(skel) if s[0] == GATE_FN]
    if len(fns) != 1:
        return [f"{HANDLER_REL}: expected exactly one `fn {GATE_FN}`, found {len(fns)}"]
    _, a, b = fns[0]
    body = code[a:b + 1]
    off: List[str] = []
    gates = list(GATE_RE.finditer(body))
    if not gates:
        return [
            f"{HANDLER_REL}:{line_of(code, a)}: `{GATE_FN}` has no live approval gate "
            "`if to.is_execution_state() && !state.work_order_events.has_approved_event("
            "scope, work_order_id).await? { return Err(... ApprovalRequired ...) }` "
            "(comments are stripped — a commented-out gate does not count)"
        ]
    g = gates[0]
    # top level of the fn body: brace depth 1 relative to body start
    sk_body = skel[a:b + 1]
    depth = sk_body[:g.start()].count("{") - sk_body[:g.start()].count("}")
    if depth != 1:
        off.append(
            f"{HANDLER_REL}:{line_of(code, a + g.start())}: the approval gate is nested "
            f"(brace depth {depth}) — it must sit at the top level of `{GATE_FN}` so it "
            "runs unconditionally"
        )
    writes = [m.start() for m in re.finditer(r"\.\s*transition_in_executor\s*\(", body)]
    if not writes:
        off.append(f"{HANDLER_REL}: `{GATE_FN}` no longer writes via transition_in_executor")
    elif min(writes) < g.start():
        off.append(
            f"{HANDLER_REL}:{line_of(code, a + min(writes))}: the state write precedes the "
            "approval gate"
        )
    # only the stretch between the gate and the write matters (a tracing
    # macro's `to = ...` field after the write is not a rebind).
    tail = body[g.end():min(writes)] if writes else body[g.end():]
    rebind = re.search(r"\blet\s+(mut\s+)?\(?[^=;]*\bto\b[^=;]*=|(?<![\w.])to\s*=(?!=)", tail)
    if rebind:
        off.append(
            f"{HANDLER_REL}:{line_of(code, a + g.end() + rebind.start())}: `to` is rebound "
            "after the approval gate — the gated target is no longer the written target"
        )
    if writes and not re.search(r"\bto_state\s*:\s*to\s*,", body[min(writes):]):
        off.append(
            f"{HANDLER_REL}: the transition_in_executor event does not record "
            "`to_state: to` — the written state is not the gated state"
        )
    return off


def probe_b(ctx: Ctx) -> List[str]:
    """work_orders.state has a single audited writer."""
    off: List[str] = []
    crates = ctx.root / "crates"
    if not crates.is_dir():
        return [f"{ctx.rel(crates)} missing"]
    files = sorted(crates.glob("*/src/**/*.rs"))
    if not files:
        return ["no Rust sources under crates/*/src"]
    writer_found = set()
    for f in files:
        r = ctx.rel(f)
        raw = f.read_text(encoding="utf-8")
        if "work_orders" not in raw and "_in_executor" not in raw:
            continue
        code, skel, strs = ctx.load(f)
        excl = cfg_test_ranges(skel)
        spans = fn_spans(skel)

        def live(o: int) -> bool:
            return not any(s <= o <= e for s, e in excl)

        in_repo = r.startswith(REPO_CRATE_REL)
        for s, e in strs:
            if not live(s):
                continue
            lit = code[s:e]
            for um in re.finditer(r"\bUPDATE\s+work_orders\b", lit, re.I):
                where = lit[um.end():]
                wm = re.search(r"\bWHERE\b", where, re.I)
                set_clause = where[: wm.start()] if wm else where
                sets_state = re.search(r"(?<![\w.])state\s*=", set_clause, re.I)
                fn = enclosing_fn(spans, s)
                ln = line_of(code, s)
                if not in_repo:
                    off.append(
                        f"{r}:{ln}: `UPDATE work_orders` SQL outside the repository crate "
                        f"(in fn {fn}) — DEC-FBR-03 and C22 inv. 3 both forbid it"
                    )
                elif sets_state:
                    if r == REPO_WO_REL and fn in STATE_WRITER_FNS:
                        writer_found.add(fn)
                    else:
                        off.append(
                            f"{r}:{ln}: fn `{fn}` writes `work_orders.state` — only "
                            f"{sorted(STATE_WRITER_FNS)} in {REPO_WO_REL} may, and only "
                            f"`{GATE_FN}` may drive them (a second writer bypasses the "
                            "approval gate)"
                        )
            for im in re.finditer(r"\bINSERT\s+INTO\s+work_orders\s*\(([^)]*)\)", lit, re.I):
                if re.search(r"(?<![\w.])state\b", im.group(1), re.I):
                    off.append(
                        f"{r}:{line_of(code, s)}: `INSERT INTO work_orders` names the "
                        "`state` column — a work order must be born `draft` by the column "
                        "default"
                    )
        for cm in re.finditer(r"\.\s*(transition_in_executor|update_state_in_executor)\s*\(", skel):
            if not live(cm.start()):
                continue
            name = cm.group(1)
            fn = enclosing_fn(spans, cm.start())
            ln = line_of(code, cm.start())
            if name == "update_state_in_executor":
                off.append(
                    f"{r}:{ln}: production call to `update_state_in_executor` (in fn {fn}) "
                    "— the unpaired setter writes state with no ledger row and no gate; "
                    "it is for the bypass-resistance test corpus only"
                )
            elif not (r == HANDLER_REL and fn == GATE_FN):
                off.append(
                    f"{r}:{ln}: `transition_in_executor` called from fn `{fn}` — only "
                    f"`{GATE_FN}` (which holds the approval gate) may call it"
                )
    missing = STATE_WRITER_FNS - writer_found
    if missing:
        off.append(
            f"{REPO_WO_REL}: expected state writer(s) {sorted(missing)} not found — the "
            "oracle's model of the sole writer is stale; re-read the repository"
        )
    return off


def probe_c(ctx: Ctx, full: bool) -> Tuple[Optional[bool], str]:
    if not full:
        return None, "skipped (pass --full to run tests/work_order_state_machine.rs)"
    t = ctx.root / SM_TEST_REL
    if not t.exists():
        return False, f"{SM_TEST_REL} is missing — the bypass-resistance corpus is gone"
    try:
        proc = subprocess.run(
            ["cargo", "test", "-p", "feedbackmonk-api", "--test", "work_order_state_machine"],
            cwd=str(ctx.root), capture_output=True, text=True, timeout=600,
        )
    except FileNotFoundError:
        return None, "cargo not found — Probe C inconclusive"
    except subprocess.TimeoutExpired:
        return False, "cargo test --test work_order_state_machine timed out"
    if proc.returncode == 0:
        return True, "cargo test --test work_order_state_machine: all passed"
    tail = (proc.stdout + proc.stderr).strip().splitlines()[-8:]
    return False, "work_order_state_machine failed:\n      " + "\n      ".join(tail)


def main() -> int:
    ap = argparse.ArgumentParser(description="approval-gate-enforcement oracle")
    ap.add_argument("--full", action="store_true",
                    help="also run the work_order_state_machine integration test (Probe C)")
    ap.add_argument("--root", type=Path, default=DEFAULT_ROOT,
                    help="repository root to scan (default: this repo; used by the "
                         "adversarial self-test against a mutated copy)")
    args = ap.parse_args()
    ctx = Ctx(args.root.resolve())
    if not (ctx.root / "crates").is_dir():
        print(f"ERROR approval-gate-enforcement: {ctx.root}/crates not found", file=sys.stderr)
        return 2

    a = probe_a(ctx)
    b = probe_b(ctx)
    c_ok, c_msg = probe_c(ctx, args.full)
    fails = (1 if a else 0) + (1 if b else 0) + (1 if c_ok is False else 0)

    if fails == 0:
        print("PASS approval-gate-enforcement")
        print(f"  Probe A (approval gate live in {GATE_FN}, before the write): clean")
        print("  Probe B (work_orders.state has one audited writer): clean")
        print(f"  Probe C (behavioral): {c_msg}")
        return 0

    print(f"FAIL approval-gate-enforcement ({fails} probe(s) failed)")
    if a:
        print("\nProbe A failures (approval gate):")
        for o in a:
            print(f"  {o}")
        print("  Remediation: keep the has_approved_event gate at the top of "
              f"{GATE_FN}, before transition_in_executor, refusing with ApprovalRequired.")
    if b:
        print("\nProbe B failures (sole writer of work_orders.state):")
        for o in b:
            print(f"  {o}")
        print(f"  Remediation: every state change goes through {GATE_FN} -> "
              "WorkOrderRepo::transition_in_executor.")
    if c_ok is False:
        print("\nProbe C failure (behavioral):")
        print(f"  {c_msg}")
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
