#!/usr/bin/env python3
"""tier-enforcement-status Verification Oracle (canonical implementation).

Question: can any code path create a chargeable row -- a project, or a
feedback submission -- without first consulting `check_tier_quota` and
acting on its verdict? (FR-FBR-14, Contract C17/C19.)

Probe A (static, default):
  1. DISCOVER the chargeable repository functions: every non-test fn in
     `crates/feedbackmonk-repository/src/` whose body executes
     `INSERT INTO projects (` or `INSERT INTO feedback (`, closed over the
     same trait's methods that call them through `self.` (so the trait's
     default `submit_anonymous` -> `submit_anonymous_full` wrapper counts).
     Discovering zero writers for either table is a FAIL: the code exists,
     so an empty set means the probe has gone blind.
  2. FIND every call site of those functions in every other crate's `src/`
     (`#[cfg(test)]` items excluded). A method name no other repository file
     defines is matched by name alone (`.name(` or `::name(`); a shared name
     (`create`) is matched only through a receiver bound to the owning trait:
     a field/param typed `ProjectRepo`/`SqlxProjectRepo` (`state.projects`),
     a `Sqlx*Repo::new(..)` local, a `let x = &state.projects;` alias, or
     UFCS `ProjectRepo::create(`. Zero call sites for either table is a FAIL.
  3. GUARD: the fn enclosing each call must, EARLIER in its body (comments
     stripped), bind `let v = ...check_tier_quota(..., ResourceKind::<K>)...;`
     with the ResourceKind matching the table (projects -> Project,
     feedback -> FeedbackInRollingMonth), AND test `v.allowed` before the
     call. A fn that calls without that guard is itself chargeable: its own
     callers must carry the guard (this is how `submit` guards the private
     `submit_*_path` helpers). An unguarded fn with no callers -- a route
     handler, or anything referenced only as a value -- is a FAIL.
  4. No `INSERT INTO projects|feedback (` outside the repository crate.

Probe C (--full): `cargo test -p feedbackmonk-api --test tier_enforcement_smoke`
  drives the cap-firing HTTP path end-to-end. The test target exists, so a
  missing target is a FAIL (no vacuous pass).

The tier_quotas() C19 shape is NOT re-checked here: the core unit tests
`c19_*_tier_shape` in crates/feedbackmonk-core/src/tier.rs assert it value
by value.

Exit 0 PASS, 1 FAIL, 2 environment error.
"""
from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_ROOT = SCRIPT_DIR.parents[2]
ALLOWLIST = SCRIPT_DIR / "allowlist.toml"

TABLE_KIND = {"projects": "Project", "feedback": "FeedbackInRollingMonth"}
INSERT_RE = re.compile(r"INSERT\s+INTO\s+(projects|feedback)\s*\(", re.IGNORECASE)


# --------------------------------------------------------------------------
# Rust lexing (just enough): comments -> spaces; a second view also blanks
# string/char literal contents. Offsets and newlines are preserved.
# --------------------------------------------------------------------------

_TOKEN_RE = re.compile(
    r"//[^\n]*"                                   # line comment
    r"|/\*(?:[^*]|\*(?!/))*\*/"                   # block comment (non-nested)
    r"|\br(#*)\"(?:.|\n)*?\"\1"                   # raw string
    r"|b?\"(?:\\.|[^\"\\])*\""                    # string
    r"|b?'(?:\\.[^'\n]{0,8}|[^'\\\n])'",          # char literal (not a lifetime)
    re.DOTALL,
)
_NONNL = re.compile(r"[^\n]")


def _blank(t: str) -> str:
    return " " * len(t) if "\n" not in t else _NONNL.sub(" ", t)


def lex(text: str) -> Tuple[str, str]:
    """Return (code_with_strings, code_blank). Both have comments blanked;
    code_blank also blanks string/char literal contents. Offsets and
    newlines are preserved, so line numbers stay true."""
    a_parts, b_parts, last = [], [], 0
    for m in _TOKEN_RE.finditer(text):
        s, e = m.span()
        a_parts.append(text[last:s])
        b_parts.append(text[last:s])
        tok = m.group(0)
        if tok.startswith("/"):
            bl = _blank(tok)
            a_parts.append(bl)
            b_parts.append(bl)
        else:
            a_parts.append(tok)
            q = tok.index(tok[-1]) if tok[-1] == "'" else tok.index('"')
            b_parts.append(tok[: q + 1] + _blank(tok[q + 1 : -1]) + tok[-1])
        last = e
    a_parts.append(text[last:])
    b_parts.append(text[last:])
    return "".join(a_parts), "".join(b_parts)


_BRACE_RE = re.compile(r"[{}]")


def match_brace(t: str, open_idx: int) -> Optional[int]:
    depth = 0
    for m in _BRACE_RE.finditer(t, open_idx):
        if m.group(0) == "{":
            depth += 1
        else:
            depth -= 1
            if depth == 0:
                return m.start()
    return None


def blank_test_items(blank_view: str, *views: str) -> List[str]:
    """Blank every `#[cfg(test)]`-gated mod/fn in all views."""
    spans = []
    for m in re.finditer(r"#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:mod|fn)\b", blank_view):
        ob = blank_view.find("{", m.end())
        semi = blank_view.find(";", m.end())
        if ob < 0 or (0 <= semi < ob):
            continue
        cb = match_brace(blank_view, ob)
        if cb is not None:
            spans.append((m.start(), cb + 1))
    out = []
    for v in (blank_view,) + views:
        assert len(v) == len(blank_view)
        for s, e in spans:
            v = v[:s] + _NONNL.sub(" ", v[s:e]) + v[e:]
        out.append(v)
    return out


class Fn:
    __slots__ = ("name", "start", "body_open", "body_close", "path")

    def __init__(self, name, start, body_open, body_close, path):
        self.name, self.start, self.body_open, self.body_close, self.path = name, start, body_open, body_close, path


def find_fns(t: str, path: Path) -> List[Fn]:
    """Every `fn name[<generics>](...) [-> T] [where ..] { body }` (generic
    fns such as `fn x<'t>(` included). Bodiless trait decls are skipped."""
    out = []
    for m in re.finditer(r"\bfn\s+(\w+)\s*[<(]", t):
        i, depth = m.end() - 1, 0
        body_open = None
        while i < len(t):
            c = t[i]
            if c in "(":
                depth += 1
            elif c == ")":
                depth -= 1
            elif depth == 0 and c == "{":
                body_open = i
                break
            elif depth == 0 and c == ";":
                break
            i += 1
        if body_open is None:
            continue
        close = match_brace(t, body_open)
        if close is None:
            continue
        out.append(Fn(m.group(1), m.start(), body_open, close, path))
    return out


def enclosing(fns: List[Fn], pos: int) -> Optional[Fn]:
    best = None
    for f in fns:
        if f.body_open < pos < f.body_close and (best is None or f.body_open > best.body_open):
            best = f
    return best


def line_no(t: str, idx: int) -> int:
    return t.count("\n", 0, idx) + 1


class Src:
    """A source file, lexed lazily: `raw` is always read (cheap substring
    pre-filters); `blank`/`code`/`fns` are built on first use."""

    def __init__(self, path: Path):
        self.path = path
        self.raw = path.read_text(encoding="utf-8", errors="replace")
        self._blank = self._code = self._fns = None

    def _lex(self):
        code, blank = lex(self.raw)
        self._blank, self._code = blank_test_items(blank, code)

    @property
    def blank(self) -> str:
        if self._blank is None:
            self._lex()
        return self._blank

    @property
    def code(self) -> str:
        if self._code is None:
            self._lex()
        return self._code

    @property
    def fns(self) -> List[Fn]:
        if self._fns is None:
            self._fns = find_fns(self.blank, self.path)
        return self._fns


def receiver_at(t: str, dot_pos: int) -> Optional[str]:
    """The identifier immediately before the `.` (or `::`, for UFCS
    `ProjectRepo::create(..)`) at dot_pos."""
    m = re.search(r"(\w+)\s*$", t[max(0, dot_pos - 80):dot_pos])
    return m.group(1) if m else None


def is_fn_call(t: str, pos: int) -> bool:
    """A call of a *derived* api fn at pos: a free/path call (`f(`, `m::f(`,
    `Self::f(`) or `self.f(` -- but not `other.f(`, which is some other
    type's method that merely shares the name."""
    j = pos - 1
    while j >= 0 and t[j].isspace():
        j -= 1
    if j < 0 or t[j] != ".":
        return True
    return receiver_at(t, j) == "self"


def load_allowlist() -> Dict[str, str]:
    """`[[callers]] file = "...", function = "...", rationale = "..."`.
    An entry without a non-empty rationale is itself a FAIL."""
    out: Dict[str, str] = {}
    if not ALLOWLIST.exists():
        return out
    text = ALLOWLIST.read_text(encoding="utf-8")
    for m in re.finditer(r"\[\[callers\]\]([^\[]*)", text, re.DOTALL):
        body = m.group(1)
        f = re.search(r'file\s*=\s*"([^"]+)"', body)
        fn = re.search(r'function\s*=\s*"([^"]+)"', body)
        r = re.search(r'rationale\s*=\s*"([^"]+)"', body)
        if f and fn:
            out[f"{f.group(1)}::{fn.group(1)}"] = (r.group(1).strip() if r else "")
    return out


# --------------------------------------------------------------------------
# Probe A
# --------------------------------------------------------------------------

def probe_a(root: Path) -> Tuple[List[str], List[str]]:
    offenders: List[str] = []
    info: List[str] = []
    crates = root / "crates"
    repo_src = crates / "feedbackmonk-repository" / "src"
    api_src = crates / "feedbackmonk-api" / "src"
    for need in (repo_src, api_src):
        if not need.is_dir():
            raise EnvironmentError(f"missing {need}")

    def rel(p: Path) -> str:
        try:
            return p.relative_to(root).as_posix()
        except ValueError:
            return p.as_posix()

    repo_files = [Src(p) for p in sorted(repo_src.rglob("*.rs"))]
    other_files = [Src(p) for p in sorted(crates.glob("*/src/**/*.rs"))
                   if repo_src not in p.parents]

    # ---- 1. discover chargeable repository fns ----------------------------
    # Only repository files that textually contain an INSERT into a charged
    # table are lexed (the rest cannot seed); the closure over `self.` calls
    # runs within those files, which is where a trait's default methods live.
    seed_files = [s for s in repo_files if INSERT_RE.search(s.raw)]
    trait_blocks: Dict[str, List[Tuple[int, int, str]]] = {}
    for src in seed_files:
        blocks = []
        for m in re.finditer(r"\b(?:pub\s+)?trait\s+(\w+)[^{;]*\{|\bimpl\s*(?:<[^>]*>\s*)?(\w+)\s+for\s+(\w+)[^{;]*\{", src.blank):
            ob = m.end() - 1
            cb = match_brace(src.blank, ob)
            if cb:
                blocks.append((ob, cb, m.group(1) or m.group(2)))
        trait_blocks[str(src.path)] = blocks

    def trait_of(src: Src, pos: int) -> Optional[str]:
        best = None
        for ob, cb, name in trait_blocks[str(src.path)]:
            if ob < pos < cb and (best is None or ob > best[0]):
                best = (ob, name)
        return best[1] if best else None

    impl_names: Dict[str, Set[str]] = {}
    for s in repo_files:
        for m in re.finditer(r"\bimpl\s*(?:<[^>]*>\s*)?(\w+)\s+for\s+(\w+)", s.raw):
            impl_names.setdefault(m.group(1), set()).add(m.group(2))

    charge: Dict[Tuple[str, str], str] = {}  # (trait, method) -> table
    charge_file: Dict[Tuple[str, str], Path] = {}
    for s in seed_files:
        for f in s.fns:
            body = s.code[f.body_open:f.body_close]
            m = INSERT_RE.search(body)
            if m:
                t = trait_of(s, f.start)
                if t is None:
                    offenders.append(f"{rel(s.path)}:{line_no(s.blank, f.start)}  fn {f.name} inserts into `{m.group(1)}` outside any repository trait impl -- cannot be tracked to its callers")
                    continue
                charge[(t, f.name)] = m.group(1).lower()
                charge_file[(t, f.name)] = s.path
    changed = True
    while changed:
        changed = False
        for s in seed_files:
            for f in s.fns:
                t = trait_of(s, f.start)
                if not t:
                    continue
                body = s.blank[f.body_open:f.body_close]
                for (ct, cm), table in list(charge.items()):
                    if ct == t and (t, f.name) not in charge and re.search(rf"\bself\s*\.\s*{cm}\s*\(", body):
                        charge[(t, f.name)] = table
                        charge_file[(t, f.name)] = s.path
                        changed = True

    # A method name is "shared" when another repository file also defines a
    # fn of that name (e.g. `create`); shared names are matched only through
    # a receiver bound to the owning trait. Unique names match by name alone.
    def is_shared(key: Tuple[str, str]) -> bool:
        mth = key[1]
        fre = re.compile(rf"\bfn\s+{mth}\b")
        return any(fre.search(s.raw) for s in repo_files if s.path != charge_file[key])

    for table in TABLE_KIND:
        if table not in charge.values():
            offenders.append(f"discovery: no repository fn inserts into `{table}` -- the probe is blind (renamed table? moved insert?)")
    info.append("chargeable repository fns: " + ", ".join(f"{t}::{m} ({tb})" for (t, m), tb in sorted(charge.items())))

    # receivers bound to each trait, across all non-repository sources
    receivers: Dict[str, Set[str]] = {}
    traits = {k[0] for k in charge if is_shared(k)}
    for t in traits:
        types = {t} | impl_names.get(t, set())
        tp = "|".join(sorted(types))
        recv: Set[str] = set()
        type_re = re.compile(rf"\b(?:{tp})\b")
        for s in other_files:
            if not any(ty in s.raw for ty in types):
                continue
            for tm in type_re.finditer(s.blank):
                # `name: <type expr ending in the trait/impl>` (field or param)
                back = s.blank[max(0, tm.start() - 120):tm.start()]
                bm = re.search(r"\b(\w+)\s*:\s*[^,;={}()]*$", back)
                if bm:
                    recv.add(bm.group(1))
                # `let x = [Arc::new(]SqlxFooRepo::new(..)`
                lb = s.blank[max(0, tm.start() - 160):tm.start()]
                lm = re.search(r"\blet\s+(?:mut\s+)?(\w+)\s*(?::[^=;]*)?=[^;]*$", lb)
                if lm and re.match(r"\s*::\s*new\b", s.blank[tm.end():tm.end() + 12]):
                    recv.add(lm.group(1))
        # aliases of a bound field: `let x = &state.projects;`
        if recv:
            alias_re = re.compile(rf"\blet\s+(?:mut\s+)?(\w+)\s*=\s*&?\s*(?:[\w.]+\s*\.\s*)?(?:{'|'.join(sorted(recv))})\s*(?:\.\s*clone\s*\(\s*\))?\s*;")
            for s in other_files:
                if not any(r in s.raw for r in recv):
                    continue
                for m in alias_re.finditer(s.blank):
                    recv.add(m.group(1))
        receivers[t] = recv | types  # types: UFCS `Trait::m(` / `SqlxImpl::m(`

    # ---- 2/3. call sites and guards ---------------------------------------
    allow = load_allowlist()
    for k, r in allow.items():
        if not r:
            offenders.append(f"allowlist.toml  {k} has no rationale")

    guard_re = re.compile(r"\blet\s+(?:mut\s+)?(\w+)\s*(?::[^=;]*)?=\s*[^;]*?\bcheck_tier_quota\s*\([^;]*;")

    def guarded(src: Src, fn: Fn, call_pos: int, table: str) -> bool:
        pre = src.blank[fn.body_open:call_pos]
        for g in guard_re.finditer(pre):
            if not re.search(rf"ResourceKind\s*::\s*{TABLE_KIND[table]}\b", g.group(0)):
                continue
            after = pre[g.end():]
            if re.search(rf"\b{g.group(1)}\s*\.\s*allowed\b", after):
                return True
        return False

    # work items: (label, method-name, receiver-filter or None, table, chain)
    work: List[Tuple[str, str, Optional[Set[str]], str, List[str]]] = []
    for key, table in sorted(charge.items()):
        t, mth = key
        recv_filter: Optional[Set[str]] = None
        if is_shared(key):
            recv_filter = receivers.get(t, set())
            if not recv_filter:
                offenders.append(f"discovery: no receiver bound to {t} found outside the repository -- cannot locate callers of the shared name `{mth}`")
                continue
        work.append((f"{t}::{mth}", mth, recv_filter, table, [f"{t}::{mth}"]))

    for t, r in sorted(receivers.items()):
        info.append(f"receivers bound to {t}: {', '.join(sorted(r))}")
    seen: Set[Tuple[str, int]] = set()
    total_sites = 0
    direct_sites: Dict[str, int] = {tb: 0 for tb in TABLE_KIND}
    while work:
        label, needle, recv_filter, table, chain = work.pop()
        if len(chain) == 1:
            # repository method: `recv.name(` or UFCS `Trait::name(`
            pat = re.compile(rf"(?:\.|::)\s*{needle}\s*\(")
        else:
            # derived (unguarded) fn: any call shape -- `f(`, `self.f(`, `m::f(`, `f::<T>(`
            pat = re.compile(rf"(?<![\w]){needle}\s*(?:::<[^>]*>\s*)?\(")
        for s in other_files:
            if needle not in s.raw:
                continue
            if recv_filter is not None and not any(receiver_at(s.raw, rm.start()) in recv_filter for rm in pat.finditer(s.raw)):
                continue
            for m in pat.finditer(s.blank):
                if recv_filter is not None and receiver_at(s.blank, m.start()) not in recv_filter:
                    continue
                if re.search(r"\bfn\s+$", s.blank[max(0, m.start() - 8):m.start()]):
                    continue  # the definition, not a call
                if len(chain) > 1 and not is_fn_call(s.blank, m.start()):
                    continue  # `other.name(` is some other type's method, not this fn
                fn = enclosing(s.fns, m.start())
                where = f"{rel(s.path)}:{line_no(s.blank, m.start())}"
                total_sites += 1
                if len(chain) == 1:
                    direct_sites[table] += 1
                if fn is None:
                    offenders.append(f"{where}  call to {label} outside any fn")
                    continue
                if guarded(s, fn, m.start(), table):
                    continue
                key = f"{rel(s.path)}::{fn.name}"
                if key in allow and allow[key]:
                    continue
                if (str(s.path), fn.start) in seen:
                    continue
                seen.add((str(s.path), fn.start))
                # Unguarded: its own callers must carry the guard.
                fpat = re.compile(rf"(?<!fn )(?<![\w]){fn.name}\s*(?:::<[^>]*>\s*)?\(")
                has_caller = False
                for s2 in other_files:
                    if fn.name not in s2.raw:
                        continue
                    for m2 in fpat.finditer(s2.blank):
                        # skip the definition itself
                        if re.search(r"\bfn\s+$", s2.blank[max(0, m2.start() - 8):m2.start()]):
                            continue
                        if not is_fn_call(s2.blank, m2.start()):
                            continue
                        has_caller = True
                        break
                    if has_caller:
                        break
                new_chain = chain + [f"{key} ({where})"]
                if not has_caller:
                    offenders.append(
                        f"{where}  {fn.name} creates a `{table}` row without a prior "
                        f"`let v = check_tier_quota(.., ResourceKind::{TABLE_KIND[table]}) ..; if !v.allowed` "
                        f"guard, and nothing calls it that could guard it (route handler / entry point). "
                        f"chain: {' <- '.join(new_chain)}"
                    )
                else:
                    work.append((key, fn.name, None, table, new_chain))
    info.append(f"call sites examined: {total_sites}")
    for tb, n in direct_sites.items():
        if n == 0:
            offenders.append(f"discovery: zero call sites of the `{tb}`-creating repository fns outside the repository -- the probe is blind (receiver renamed?)")

    # ---- 4. inserts outside the repository crate --------------------------
    for s in other_files:
        if not INSERT_RE.search(s.raw):
            continue
        for m in INSERT_RE.finditer(s.code):
            offenders.append(f"{rel(s.path)}:{line_no(s.code, m.start())}  `INSERT INTO {m.group(1)}` outside feedbackmonk-repository bypasses the tracked chargeable fns")
    return offenders, info


def probe_c(root: Path, full: bool) -> Tuple[Optional[bool], str]:
    if not full:
        return None, "skipped (pass --full to run integration smoke)"
    cmd = ["cargo", "test", "--manifest-path", str(root / "Cargo.toml"), "-p", "feedbackmonk-api",
           "--test", "tier_enforcement_smoke", "--", "--include-ignored"]
    env = os.environ.copy()
    env.setdefault("DATABASE_URL", "postgres://postgres:dev@localhost:5433/feedbackmonk_dev")
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=600, cwd=str(root), env=env)
    except FileNotFoundError:
        return False, "cargo not on PATH"
    except subprocess.TimeoutExpired:
        return False, "cargo test exceeded 600s timeout"
    if proc.returncode == 0:
        return True, "cargo test --test tier_enforcement_smoke: GREEN"
    err = (proc.stderr or "") + (proc.stdout or "")
    return False, f"cargo test failed:\n{err.strip()[-2000:]}"


def main() -> int:
    ap = argparse.ArgumentParser(description="tier-enforcement-status Verification Oracle")
    ap.add_argument("--full", action="store_true", help="also run the tier_enforcement_smoke integration test")
    ap.add_argument("--root", type=Path, default=DEFAULT_ROOT, help="repository root to scan (default: this repo)")
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()
    root = args.root.resolve()
    try:
        a_off, a_info = probe_a(root)
    except (EnvironmentError, OSError) as e:
        print(f"ERROR tier-enforcement-status: {e}")
        return 2
    c_ok, c_msg = probe_c(root, args.full)
    if not a_off and c_ok is not False:
        print("PASS tier-enforcement-status")
        print("  Probe A (every chargeable-row creation is tier-guarded): clean")
        for line in a_info:
            print(f"    {line}")
        print(f"  Probe C (integration smoke): {c_msg}")
        return 0
    print("FAIL tier-enforcement-status")
    if a_off:
        print("\nProbe A failures (chargeable row created without a tier-cap guard):")
        for o in a_off:
            print(f"  {o}")
        if args.verbose:
            for line in a_info:
                print(f"    {line}")
        print("  Remediation: consult `state.tier_quotas.check_tier_quota(scope, ResourceKind::*)` and "
              "return on `!v.allowed` BEFORE the create/submit call (FR-FBR-14, Contract C17).")
    if c_ok is False:
        print("\nProbe C failure (integration smoke):")
        print(f"  {c_msg}")
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
