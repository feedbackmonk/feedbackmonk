#!/usr/bin/env python3
"""multi-tenant-isolation-check Verification Oracle (canonical implementation).

Two probes:
  A) No raw SQL or Connection-grabbing OUTSIDE crates/feedbackmonk-repository/.
  B) Every public repository-crate function signature accepts &TenantScope or
     &ProjectScope as the first non-&self argument, OR is allow-listed in
     allowlist.toml as a documented pre-auth exception.

Output: machine-parseable PASS / FAIL with file:line offenders on FAIL.
Exit: 0 on PASS, 1 on FAIL.

Invoked by oracle.ps1 (Windows) and oracle.sh (Unix). Python 3.8+ is the only
runtime dependency (CI ubuntu-latest ships it; local dev machines have it).
Spec: see manifest.json. Lineage: P0 plan section C1, DEC-FBR-03.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path
from typing import List, Optional, Tuple


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[2]
CRATES_DIR = REPO_ROOT / "crates"
REPO_CRATE = CRATES_DIR / "feedbackmonk-repository"
ALLOWLIST = SCRIPT_DIR / "allowlist.toml"


FORBIDDEN = [
    (r"sqlx::query!\s*\(",        "sqlx::query!"),
    (r"sqlx::query_as!\s*\(",     "sqlx::query_as!"),
    (r"sqlx::query_scalar!\s*\(", "sqlx::query_scalar!"),
    (r"sqlx::query_as\b",         "sqlx::query_as"),
    (r"sqlx::query_scalar\b",     "sqlx::query_scalar"),
    (r"sqlx::query\b",            "sqlx::query"),
    (r"pool\.acquire\s*\(",       "pool.acquire"),
    (r"&mut\s+Connection\b",      "&mut Connection"),
    (r"&mut\s+PgConnection\b",    "&mut PgConnection"),
    (r"&mut\s+Transaction\b",     "&mut Transaction"),
    (r"Pool<Postgres>",           "Pool<Postgres>"),
]


def strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.DOTALL)
    text = re.sub(r"//[^\n\r]*", "", text)
    return text


def find_matching(text: str, open_idx: int, opener: str, closer: str) -> int:
    depth = 0
    for i in range(open_idx, len(text)):
        c = text[i]
        if c == opener:
            depth += 1
        elif c == closer:
            depth -= 1
            if depth == 0:
                return i
    return -1


def line_no(text: str, idx: int) -> int:
    return text.count("\n", 0, max(0, min(idx, len(text)))) + 1


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p).replace("\\", "/")


def load_allowlist() -> Tuple[set, set]:
    """Return (trait_method_keys, inherent_method_keys) sets."""
    trait_keys = set()
    inherent_keys = set()
    if not ALLOWLIST.exists():
        return trait_keys, inherent_keys

    text = ALLOWLIST.read_text()

    def block_iter(header: str):
        # Iterate non-overlapping blocks for `[[header]]`.
        pat = re.compile(r"\[\[" + re.escape(header) + r"\]\]([^\[]*)", re.DOTALL)
        for m in pat.finditer(text):
            yield m.group(1)

    for body in block_iter("methods"):
        tr = re.search(r'trait\s*=\s*"([^"]+)"', body)
        me = re.search(r'method\s*=\s*"([^"]+)"', body)
        if tr and me:
            trait_keys.add(f"{tr.group(1)}::{me.group(1)}")

    for body in block_iter("inherent_methods"):
        tn = re.search(r'type_name\s*=\s*"([^"]+)"', body)
        me = re.search(r'method\s*=\s*"([^"]+)"', body)
        if tn and me:
            inherent_keys.add(f"{tn.group(1)}::{me.group(1)}")

    return trait_keys, inherent_keys


def probe_a() -> List[str]:
    """Forbidden patterns outside the repository crate."""
    offenders = []
    if not CRATES_DIR.exists():
        return offenders
    for path in CRATES_DIR.rglob("*.rs"):
        # Skip files inside the repo crate or any target/ build output.
        try:
            path.relative_to(REPO_CRATE)
            continue
        except ValueError:
            pass
        if any(part == "target" for part in path.parts):
            continue
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except UnicodeDecodeError:
            continue
        for i, raw in enumerate(lines, start=1):
            stripped = re.sub(r"//.*$", "", raw)
            for pat, label in FORBIDDEN:
                if re.search(pat, stripped):
                    offenders.append(
                        f"{rel(path)}:{i}  forbidden pattern '{label}' outside crates/feedbackmonk-repository/"
                    )
    return offenders


def first_non_self_arg(sig: str) -> Optional[str]:
    """Strip self forms and return the first top-level argument, or None."""
    s = sig.strip()
    s = re.sub(r"^\s*&\s*mut\s+self\s*,?\s*", "", s)
    s = re.sub(r"^\s*&\s*self\s*,?\s*", "", s)
    s = re.sub(r"^\s*mut\s+self\s*,?\s*", "", s)
    s = re.sub(r"^\s*self\s*,?\s*", "", s)
    s = s.strip()
    if not s:
        return None
    depth = 0
    out = []
    for c in s:
        if c in "<([":
            depth += 1
        elif c in ">)]":
            depth -= 1
        elif c == "," and depth == 0:
            break
        out.append(c)
    return "".join(out).strip()


def is_scope_arg(arg: str) -> bool:
    return bool(re.search(r"&\s*TenantScope\b", arg) or re.search(r"&\s*ProjectScope\b", arg))


def find_inherent_impl_ranges(content: str):
    """Return list of (type_name, body_start_brace_idx, body_close_brace_idx)
    for `impl Type { ... }` blocks (NOT `impl Trait for Type { ... }`)."""
    ranges = []
    # Greedy scan for `impl <Type> {` (no `for`); generics on the type are tolerated.
    for m in re.finditer(r"\bimpl(?:\s*<[^>]*>)?\s+(\w+)(?:\s*<[^>]*>)?\s*\{", content):
        type_name = m.group(1)
        brace_open = m.start() + len(m.group(0)) - 1
        brace_close = find_matching(content, brace_open, "{", "}")
        if brace_close < 0:
            continue
        ranges.append((type_name, brace_open, brace_close))
    return ranges


def probe_b() -> List[str]:
    """Repository-method scope discipline."""
    offenders = []
    if not REPO_CRATE.exists():
        return offenders

    trait_keys, inherent_keys = load_allowlist()

    src = REPO_CRATE / "src"
    if not src.exists():
        return offenders

    for path in src.rglob("*.rs"):
        raw = path.read_text(encoding="utf-8")
        content = strip_comments(raw)

        # 1) Public trait blocks: methods inside are public-by-virtue.
        for tm in re.finditer(r"\bpub\s+trait\s+(\w+)[^{]*\{", content, flags=re.DOTALL):
            trait_name = tm.group(1)
            brace_open = tm.start() + len(tm.group(0)) - 1
            brace_close = find_matching(content, brace_open, "{", "}")
            if brace_close < 0:
                continue
            body_start = brace_open + 1
            body = content[body_start:brace_close]
            for fm in re.finditer(r"(?:^|[^A-Za-z0-9_])(?:async\s+)?fn\s+(\w+)\s*\(", body):
                fn_name = fm.group(1)
                # Locate the '(' in the absolute content.
                paren_rel = fm.end() - 1
                paren_abs = body_start + paren_rel
                paren_close = find_matching(content, paren_abs, "(", ")")
                if paren_close < 0:
                    continue
                sig = content[paren_abs + 1:paren_close]
                arg = first_non_self_arg(sig)
                if arg is None or is_scope_arg(arg):
                    continue
                if f"{trait_name}::{fn_name}" in trait_keys:
                    continue
                fn_kw_pos = fm.start() + fm.group(0).index("fn ")
                ln = line_no(content, body_start + fn_kw_pos)
                offenders.append(
                    f"{rel(path)}:{ln}  {trait_name}::{fn_name}  first non-self arg is '{arg}' (expected &TenantScope or &ProjectScope)"
                )

        # 2) impl Trait for Type blocks: methods inherit trait visibility.
        for im in re.finditer(
            r"\bimpl(?:\s*<[^>]*>)?\s+(\w+)(?:\s*<[^>]*>)?\s+for\s+(\w+)[^{]*\{",
            content,
            flags=re.DOTALL,
        ):
            trait_name = im.group(1)
            brace_open = im.start() + len(im.group(0)) - 1
            brace_close = find_matching(content, brace_open, "{", "}")
            if brace_close < 0:
                continue
            body_start = brace_open + 1
            body = content[body_start:brace_close]
            for fm in re.finditer(r"(?:^|[^A-Za-z0-9_])(?:async\s+)?fn\s+(\w+)\s*\(", body):
                fn_name = fm.group(1)
                paren_rel = fm.end() - 1
                paren_abs = body_start + paren_rel
                paren_close = find_matching(content, paren_abs, "(", ")")
                if paren_close < 0:
                    continue
                sig = content[paren_abs + 1:paren_close]
                arg = first_non_self_arg(sig)
                if arg is None or is_scope_arg(arg):
                    continue
                if f"{trait_name}::{fn_name}" in trait_keys:
                    continue
                fn_kw_pos = fm.start() + fm.group(0).index("fn ")
                ln = line_no(content, body_start + fn_kw_pos)
                offenders.append(
                    f"{rel(path)}:{ln}  {trait_name}::{fn_name}  first non-self arg is '{arg}' (expected &TenantScope or &ProjectScope)"
                )

        # 3) Inherent `pub fn` / `pub async fn` (excluding pub(crate)/pub(super)).
        # Determine enclosing inherent-impl type for each match.
        inherent_ranges = find_inherent_impl_ranges(content)

        # Also gather all known impl-trait ranges so we can exclude pub fns
        # that already belong to an `impl Trait for Type` block (they were
        # processed in pass 2 above and SHOULD NOT match `pub` since trait
        # impl methods carry no `pub` keyword anyway -- but defense in depth).
        for pm in re.finditer(r"\bpub(?!\s*\()\s+(?:async\s+)?fn\s+(\w+)\s*\(", content):
            fn_name = pm.group(1)
            paren_idx = pm.end() - 1
            paren_close = find_matching(content, paren_idx, "(", ")")
            if paren_close < 0:
                continue
            sig = content[paren_idx + 1:paren_close]
            arg = first_non_self_arg(sig)
            if arg is None or is_scope_arg(arg):
                continue

            fn_kw_pos = pm.start() + pm.group(0).index("fn ")

            # Locate enclosing inherent-impl type (innermost wins).
            enclosing_type = ""
            for type_name, start_b, end_b in inherent_ranges:
                if start_b < fn_kw_pos < end_b:
                    enclosing_type = type_name

            if enclosing_type and f"{enclosing_type}::{fn_name}" in inherent_keys:
                continue

            ln = line_no(content, fn_kw_pos)
            ctx = enclosing_type if enclosing_type else "<free>"
            offenders.append(
                f"{rel(path)}:{ln}  {ctx}::{fn_name}  first non-self arg is '{arg}' (expected &TenantScope or &ProjectScope)"
            )

    # Deduplicate while preserving order.
    seen = set()
    unique = []
    for o in offenders:
        if o not in seen:
            seen.add(o)
            unique.append(o)
    return unique


def main() -> int:
    if not CRATES_DIR.exists():
        print("PASS (no crates/ directory yet - probes vacuously satisfied)")
        return 0

    a = probe_a()
    b = probe_b()
    total = len(a) + len(b)
    if total == 0:
        print("PASS multi-tenant-isolation-check")
        print("  Probe A (raw SQL outside repository): clean")
        print("  Probe B (repository-method scope discipline): clean")
        return 0

    print(f"FAIL multi-tenant-isolation-check ({total} offender(s))")
    if a:
        print()
        print("Probe A offenders (raw SQL or Connection outside crates/feedbackmonk-repository/):")
        for o in a:
            print(f"  {o}")
    if b:
        print()
        print("Probe B offenders (public repository fn missing &TenantScope/&ProjectScope):")
        for o in b:
            print(f"  {o}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
