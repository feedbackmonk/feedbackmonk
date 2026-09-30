#!/usr/bin/env python3
"""multi-tenant-isolation-check Verification Oracle (canonical implementation).

Defends DEC-FBR-03: the tenant-scoped repository crate is the SOLE query path.

Probe A -- nothing outside crates/feedbackmonk-repository/ can reach the database
except through a repository method:
  A1  every `sqlx::` path and every `use sqlx::...` leaf outside the repository
      crate is on a closed whitelist (PgPool, PgPool::connect,
      postgres::PgPoolOptions, Error, test). raw_sql, query*, QueryBuilder,
      Executor, Acquire, Transaction, PgConnection, globs and crate aliases FAIL.
  A2  bare `query(` / `query_as(` / `query_scalar(` / `*_with(` / `raw_sql(` in any
      file that references sqlx; bare `query!`-family macros and `raw_sql(` anywhere.
  A3  `.execute(` / `.fetch*(` / `.prepare(` / `.describe(` whose first argument
      is SQL text (a string literal or `format!`) or an executor (pool / tx / conn
      / `&mut *x`).
  A4  executor methods called directly on a pool (`pool.acquire/execute/fetch*`)
      plus the legacy forbidden-token list.
  A5  transaction discipline: every `.begin()` must be bound by a `let`, and the
      bound variable may only be (a) handed as `&mut tx` / `tx` straight to a
      repository method that takes an executor (discovered from the repository
      crate's signatures), or (b) `.commit()` / `.rollback()`-ed. `&mut *tx`,
      `tx.execute(..)`, passing the tx to anything else: FAIL.
  A6  the repository crate does not `pub use` anything from sqlx (no re-export
      laundering of the query API).

Probe B -- every public repository-crate fn (trait methods, trait impls, inherent
`pub fn`, generic fns included) takes &TenantScope / &ProjectScope as its first
non-self argument, or is allow-listed in allowlist.toml with a rationale.
Allowlist hygiene is enforced: entries must have a rationale, must not repeat,
and must name a method that exists (a stale entry is a pre-signed exemption).

A missing crates/ tree or repository crate is a FAIL, not a vacuous PASS.

Exit: 0 PASS, 1 FAIL, 2 environment error.
Usage: oracle.py [--root <repo-root>] [--full]
  --root  scan another tree with the repo layout (adversarial self-test).
  --full  accepted for suite uniformity; this oracle is static-only.
Spec: oracle.json. Lineage: P0 plan section C1, DEC-FBR-03.
"""
from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

SCRIPT_DIR = Path(__file__).resolve().parent
ALLOWLIST = SCRIPT_DIR / "allowlist.toml"

REPO_ROOT = SCRIPT_DIR.parents[2]
CRATES_DIR = REPO_ROOT / "crates"
REPO_CRATE = CRATES_DIR / "feedbackmonk-repository"


def set_root(root: Path) -> None:
    global REPO_ROOT, CRATES_DIR, REPO_CRATE
    REPO_ROOT = root.resolve()
    CRATES_DIR = REPO_ROOT / "crates"
    REPO_CRATE = CRATES_DIR / "feedbackmonk-repository"


# --------------------------------------------------------------------------
# Lexing: blank comments (and, for the masked view, literal contents) while
# preserving offsets and newlines so line numbers stay exact.
# --------------------------------------------------------------------------
_TOKEN = re.compile(
    r"""
    (?=[/"'rb])
    (?:
     (?P<lc>//[^\n]*)
    |(?P<bc>/\*)
    |(?P<rs>\bb?r(?P<h>\#*)".*?"(?P=h))
    |(?P<s>b?"(?:\\.|[^"\\])*")
    |(?P<c>b?'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'\n])')
    )
    """,
    re.S | re.X,
)


def _blank(s: str) -> str:
    if "\n" not in s:
        return " " * len(s)
    return "\n".join(" " * len(part) for part in s.split("\n"))


_LEX_CACHE: Dict[str, Tuple[str, str]] = {}


def lex_views(text: str) -> Tuple[str, str]:
    hit = _LEX_CACHE.get(text)
    if hit is not None:
        return hit
    res = _lex_views(text)
    _LEX_CACHE[text] = res
    return res


def _lex_views(text: str) -> Tuple[str, str]:
    """Return (nc, masked): nc = comments blanked; masked = comments blanked
    and string/char literal *contents* blanked (delimiters kept)."""
    nc: List[str] = []
    mk: List[str] = []
    pos = 0
    n = len(text)
    while pos < n:
        m = _TOKEN.search(text, pos)
        if not m:
            nc.append(text[pos:])
            mk.append(text[pos:])
            break
        nc.append(text[pos:m.start()])
        mk.append(text[pos:m.start()])
        if m.group("lc") is not None:
            b = _blank(m.group(0))
            nc.append(b)
            mk.append(b)
            pos = m.end()
        elif m.group("bc") is not None:
            depth, i = 1, m.end()
            while i < n and depth:
                if text.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif text.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
            b = _blank(text[m.start():i])
            nc.append(b)
            mk.append(b)
            pos = i
        else:
            tok = m.group(0)
            nc.append(tok)
            q = '"' if m.group("c") is None else "'"
            first = tok.index(q)
            last = tok.rindex(q)
            mk.append(tok[: first + 1] + _blank(tok[first + 1:last]) + tok[last:])
            pos = m.end()
    return "".join(nc), "".join(mk)


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


def skip_generics(text: str, i: int) -> int:
    """If text[i] (after whitespace) is '<', return index just past its matching
    '>' (ignoring the '>' of '->'); else return i unchanged."""
    j = i
    while j < len(text) and text[j].isspace():
        j += 1
    if j >= len(text) or text[j] != "<":
        return i
    depth = 0
    k = j
    while k < len(text):
        c = text[k]
        if c == "<":
            depth += 1
        elif c == ">" and text[k - 1] != "-":
            depth -= 1
            if depth == 0:
                return k + 1
        k += 1
    return i


def line_no(text: str, idx: int) -> int:
    return text.count("\n", 0, max(0, min(idx, len(text)))) + 1


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p).replace("\\", "/")


def rust_files(base: Path, exclude: Optional[Path] = None) -> List[Path]:
    out: List[Path] = []
    excl = str(exclude.resolve()) if exclude else None
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = [
            d for d in dirnames
            if d != "target" and not (excl and os.path.join(dirpath, d) == excl)
            and not (excl and str(Path(dirpath, d).resolve()) == excl)
        ]
        for f in filenames:
            if f.endswith(".rs"):
                out.append(Path(dirpath, f))
    return sorted(out)


_READ_CACHE: Dict[Path, str] = {}


def read(path: Path, errors: List[str]) -> Optional[str]:
    if path in _READ_CACHE:
        return _READ_CACHE[path]
    try:
        _READ_CACHE[path] = path.read_text(encoding="utf-8")
        return _READ_CACHE[path]
    except (OSError, UnicodeDecodeError) as e:
        errors.append(f"{rel(path)}  unreadable ({e}) -- an unscannable file is not a clean file")
        return None


# --------------------------------------------------------------------------
# Probe A
# --------------------------------------------------------------------------
LEGACY_FORBIDDEN = [
    (r"sqlx::query!\s*\(", "sqlx::query!"),
    (r"sqlx::query_as!\s*\(", "sqlx::query_as!"),
    (r"sqlx::query_scalar!\s*\(", "sqlx::query_scalar!"),
    (r"pool(?<!\wpool)\s*\.\s*(?:try_)?acquire\s*\(", "pool.acquire"),
    (r"&\s*mut\s+(?:sqlx\s*::\s*)?Connection\b", "&mut Connection"),
    (r"&\s*mut\s+(?:sqlx\s*::\s*)?PgConnection\b", "&mut PgConnection"),
    (r"&\s*mut\s+(?:sqlx\s*::\s*)?Transaction\b", "&mut Transaction"),
    (r"Pool(?<!\wPool)\s*<\s*(?:sqlx\s*::\s*)?Postgres\s*>", "Pool<Postgres>"),
    (r"extern(?<!\wextern)\s+crate\s+sqlx\b", "extern crate sqlx (macro_use / alias hides query paths)"),
]
LEGACY_RE = [(re.compile(p), label) for p, label in LEGACY_FORBIDDEN]

# The closed whitelist of sqlx items non-repository code may name. Each is
# inert: a pool handle (constructed, cloned, handed to Sqlx*Repo::new), its
# builder, the error type, and the #[sqlx::test] harness attribute.
SQLX_ALLOWED = ("PgPool", "postgres::PgPoolOptions", "Error", "test")

# Regexes lead with a literal (word-boundary checks come after it as a
# fixed-width lookbehind) so re's literal-prefix scan keeps the run fast.
SQLX_PATH_RE = re.compile(r"sqlx(?<!\wsqlx)\s*::\s*(\w+(?:\s*::\s*\w+)*)(\s*::\s*[{*])?")
SQLX_USE_RE = re.compile(r"use(?<!\wuse)\s+(?:::\s*)?sqlx\b([^;]*);")
BARE_QUERY_RE = re.compile(
    r"(query|query_as|query_scalar|query_with|query_as_with|query_scalar_with|"
    r"query_file|query_file_as|query_file_scalar|raw_sql)\b\s*(?:!|\(|::\s*<)"
)
ALWAYS_BARE_RE = re.compile(
    r"(?:raw_sql\s*\(|(?:query|query_as|query_scalar|query_file\w*)\s*!\s*\()"
)
EXEC_METHOD_RE = re.compile(
    r"\.\s*(execute|execute_many|fetch|fetch_one|fetch_all|fetch_optional|fetch_many|"
    r"prepare|prepare_with|describe)\s*\("
)
POOL_EXEC_RE = re.compile(
    r"pool(?<!\wpool)\s*\.\s*(execute\w*|fetch\w*|prepare\w*|describe|close_event)\s*\("
)
BEGIN_RE = re.compile(r"\.\s*(begin|try_begin|begin_with)\s*\(")
EXECUTOR_ARG_RE = re.compile(
    r"^\s*&?\s*(?:mut\s+)?\*|(?:\bpool|\btx|\btxn|\btransaction|\bconn|\bconnection|\bexecutor)\b",
    re.I,
)


def expand_use_tree(tree: str) -> List[str]:
    """Expand a use-tree tail like `::{query, postgres::{PgRow as R}}` to
    leaf paths ['query', 'postgres::PgRow']. Aliases are dropped (the leaf is
    what matters). A bare `use sqlx;` / `use sqlx as x;` yields ['<crate>']."""
    t = re.sub(r"\s+", " ", tree).strip()
    if not t.startswith("::"):
        return ["<crate>"]  # `use sqlx;` or `use sqlx as foo;`

    def expand(s: str) -> List[str]:
        s = s.strip()
        if not s:
            return []
        if "{" not in s:
            s = re.sub(r"\s+as\s+\w+$", "", s).strip()
            return [s.replace(" ", "")]
        i = s.index("{")
        prefix = s[:i].strip().rstrip(":").strip()
        close = find_matching(s, i, "{", "}")
        inner = s[i + 1:close]
        parts, depth, cur = [], 0, ""
        for ch in inner:
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
            if ch == "," and depth == 0:
                parts.append(cur)
                cur = ""
            else:
                cur += ch
        parts.append(cur)
        out: List[str] = []
        for p in parts:
            for leaf in expand(p):
                if leaf in ("self", ""):
                    out.append(prefix)
                else:
                    out.append(f"{prefix}::{leaf}" if prefix else leaf)
        return out

    return expand(t[2:])


def sqlx_path_ok(path: str) -> bool:
    path = re.sub(r"\s+", "", path)
    return any(path == a or path.startswith(a + "::") for a in SQLX_ALLOWED)


def executor_methods() -> Set[str]:
    """Repository methods whose signature takes an executor/connection/tx:
    the only legal destinations for a transaction opened outside the crate."""
    names: Set[str] = set()
    src = REPO_CRATE / "src"
    for path in rust_files(src):
        text = read(path, [])
        if text is None:
            continue  # reported by the probe that reads it again
        nc, _ = lex_views(text)
        for fm in re.finditer(r"\bfn\s+(\w+)", nc):
            j = skip_generics(nc, fm.end())
            while j < len(nc) and nc[j].isspace():
                j += 1
            if j >= len(nc) or nc[j] != "(":
                continue
            close = find_matching(nc, j, "(", ")")
            sig = nc[j:close]
            if re.search(r"PgConnection|Transaction\s*<|Executor|Acquire", sig):
                names.add(fm.group(1))
    return names


def enclosing_call_name(m: str, idx: int) -> Optional[str]:
    """Name of the call whose argument list directly contains idx, or None."""
    depth = 0
    i = idx - 1
    while i >= 0:
        c = m[i]
        if c in ")]}":
            depth += 1
        elif c in "([{":
            if depth == 0:
                if c != "(":
                    return None
                head = m[max(0, i - 256):i].rstrip()
                head = head[: skip_turbofish_back(head)]
                mm = re.search(r"\.\s*(\w+)\s*$", head)
                return mm.group(1) if mm else None
            depth -= 1
        i -= 1
    return None


def skip_turbofish_back(head: str) -> int:
    mm = re.search(r"::\s*<[^()]*>\s*$", head)
    return mm.start() if mm else len(head)


def block_end(m: str, idx: int) -> int:
    """Index of the '}' closing the block that contains idx."""
    depth = 0
    for i in range(idx, len(m)):
        c = m[i]
        if c == "{":
            depth += 1
        elif c == "}":
            if depth == 0:
                return i
            depth -= 1
    return len(m)


def first_arg(m: str, open_idx: int) -> str:
    close = find_matching(m, open_idx, "(", ")")
    inner = m[open_idx + 1: close if close > 0 else len(m)]
    depth, out = 0, []
    for c in inner:
        if c in "([{<":
            depth += 1
        elif c in ")]}>":
            depth -= 1
        elif c == "," and depth == 0:
            break
        out.append(c)
    return "".join(out)


def probe_a(errors: List[str]) -> List[str]:
    offenders: List[str] = []
    if not CRATES_DIR.is_dir():
        return [f"{rel(CRATES_DIR)}  missing -- nothing to scan is a FAIL, not a vacuous PASS"]
    if not (REPO_CRATE / "src").is_dir():
        return [f"{rel(REPO_CRATE)}/src  missing -- the repository crate is the reference point"]
    exec_fns = executor_methods()
    if not exec_fns:
        offenders.append(
            f"{rel(REPO_CRATE)}/src  no executor-taking repository fn found -- A5 would have "
            "no legal transaction destinations; the discovery regex has drifted"
        )

    files = rust_files(CRATES_DIR, exclude=REPO_CRATE)
    if not files:
        offenders.append(f"{rel(CRATES_DIR)}  no .rs files outside the repository crate -- scan found nothing")
    for path in files:
        raw = read(path, errors)
        if raw is None:
            continue
        nc, m = lex_views(raw)
        where = rel(path)

        def add(idx: int, msg: str) -> None:
            offenders.append(f"{where}:{line_no(m, idx)}  {msg}")

        # Legacy forbidden tokens.
        for rx, label in LEGACY_RE:
            for hit in rx.finditer(m):
                add(hit.start(), f"forbidden pattern '{label}' outside crates/feedbackmonk-repository/")

        # A1 -- sqlx use-trees and paths.
        use_spans = []
        for u in SQLX_USE_RE.finditer(m):
            use_spans.append((u.start(), u.end()))
            for leaf in expand_use_tree(u.group(1)):
                if leaf == "<crate>":
                    add(u.start(), "`use sqlx` / crate alias -- hides every sqlx path from this probe")
                elif "*" in leaf or not sqlx_path_ok(leaf):
                    add(u.start(), f"imports `sqlx::{leaf}` -- not on the non-repository whitelist {SQLX_ALLOWED}")
        for p in SQLX_PATH_RE.finditer(m):
            if any(s <= p.start() < e for s, e in use_spans):
                continue
            if p.group(2):  # `sqlx::x::{..}` / `sqlx::*` outside a use: odd, but check the head
                add(p.start(), f"`sqlx::{p.group(1)}::{{..}}` -- unparseable sqlx path outside a use")
                continue
            if not sqlx_path_ok(p.group(1)):
                add(p.start(), f"`sqlx::{re.sub(chr(32), '', p.group(1))}` -- not on the non-repository whitelist {SQLX_ALLOWED}")

        # A2 -- bare query builders.
        mentions_sqlx = "sqlx" in m
        if mentions_sqlx:
            for q in BARE_QUERY_RE.finditer(m):
                if q.start() and (m[q.start() - 1].isalnum() or m[q.start() - 1] in "_.:"):
                    continue  # part of a longer ident, a method, or a qualified path (A1 judges those)
                if re.search(r"\bfn\s*$", m[max(0, q.start() - 16):q.start()]):
                    continue  # a local fn definition named `query`
                add(q.start(), f"bare `{q.group(1)}` in a file that references sqlx -- raw SQL builder")
        for q in ALWAYS_BARE_RE.finditer(m):
            if q.start() and (m[q.start() - 1].isalnum() or m[q.start() - 1] in "_."):
                continue
            if not (mentions_sqlx and BARE_QUERY_RE.match(m, q.start())):
                add(q.start(), f"`{q.group(0).strip()}` -- raw SQL outside the repository crate")

        # A3 -- executor methods fed SQL text or an executor.
        for e in EXEC_METHOD_RE.finditer(m):
            open_idx = e.end() - 1
            arg_nc = first_arg(nc, open_idx)
            arg_m = first_arg(m, open_idx)
            if '"' in arg_nc or "format!" in arg_m or "concat!" in arg_m:
                add(e.start(), f"`.{e.group(1)}(\"...\")` -- SQL text handed straight to an executor")
            elif EXECUTOR_ARG_RE.search(arg_m):
                add(e.start(), f"`.{e.group(1)}({arg_m.strip()})` -- executor used outside the repository crate")

        # A4 -- executor methods straight on a pool.
        for e in POOL_EXEC_RE.finditer(m):
            add(e.start(), f"`pool.{e.group(1)}(` -- pool used as an executor outside the repository crate")

        # A5 -- transaction discipline.
        for b in BEGIN_RE.finditer(m):
            stmt_start = max(m.rfind(";", 0, b.start()), m.rfind("{", 0, b.start()), m.rfind("}", 0, b.start())) + 1
            head = m[stmt_start:b.start()]
            lm = re.fullmatch(r"\s*let\s+(?:mut\s+)?(\w+)\s*(?::[^=]*)?=\s*[\w.\s]*?", head)
            semi = m.find(";", b.end())
            tail = m[b.end():semi if semi >= 0 else len(m)]
            tail_ok = re.fullmatch(
                r"\s*\)\s*\.\s*await\s*(?:\?|\.\s*unwrap\s*\(\s*\)|\.\s*expect\s*\([^()]*\))?\s*", tail
            )
            if not lm or not tail_ok:
                add(b.start(), f"`.{b.group(1)}()` not bound as `let [mut] <tx> = <pool>.begin().await?;` -- "
                    "an unbound transaction escapes the discipline check")
                continue
            var = lm.group(1)
            end = block_end(m, semi)
            for use in re.finditer(r"\b" + re.escape(var) + r"\b", m[semi:end]):
                at = semi + use.start()
                after = m[at + len(var):at + len(var) + 64]
                if re.match(r"\s*\.\s*(commit|rollback)\s*\(\s*\)", after):
                    continue
                win_lo = max(0, at - 64)
                amp = re.search(r"&\s*mut\s+$", m[win_lo:at])
                arg_start = win_lo + amp.start() if amp else at
                prev = m[max(0, arg_start - 64):arg_start].rstrip()
                nxt = m[at + len(var):at + len(var) + 64].lstrip()
                if prev.endswith(("(", ",")) and nxt[:1] in (",", ")"):
                    call = enclosing_call_name(m, arg_start)
                    if call in exec_fns:
                        continue
                    add(at, f"transaction `{var}` handed to `{call or '<non-method call>'}` -- only "
                        "executor-taking repository methods may receive it")
                    continue
                add(at, f"transaction `{var}` used as `{m[max(stmt_start, at - 6):at + len(var) + 12].strip()}` -- "
                    "only `&mut {v}` into a repository method, `.commit()` or `.rollback()` are allowed".replace("{v}", var))

    # A6 -- the repository crate must not re-export the query API.
    for path in rust_files(REPO_CRATE / "src"):
        raw = read(path, errors)
        if raw is None:
            continue
        _, m = lex_views(raw)
        for u in re.finditer(r"\bpub(?:\s*\([^)]*\))?\s+use\s+(?:::\s*)?sqlx\b", m):
            offenders.append(f"{rel(path)}:{line_no(m, u.start())}  repository crate re-exports sqlx -- launders the query API past Probe A")
    return offenders


# --------------------------------------------------------------------------
# Probe B
# --------------------------------------------------------------------------
def load_allowlist(problems: List[str]) -> Tuple[Dict[str, str], Dict[str, str]]:
    """Return (trait_keys, inherent_keys) as key -> rationale."""
    trait_keys: Dict[str, str] = {}
    inherent_keys: Dict[str, str] = {}
    if not ALLOWLIST.exists():
        return trait_keys, inherent_keys
    try:
        import tomllib  # type: ignore[import-not-found]
        data = tomllib.loads(ALLOWLIST.read_text(encoding="utf-8"))
    except ModuleNotFoundError:  # Python < 3.11: comment-aware fallback
        data = {"methods": [], "inherent_methods": []}
        text = "\n".join(l for l in ALLOWLIST.read_text(encoding="utf-8").splitlines()
                         if not l.lstrip().startswith("#"))
        for hm in re.finditer(r"^\[\[(\w+)\]\]\s*\n(.*?)(?=^\[\[|\Z)", text, re.S | re.M):
            entry = dict(re.findall(r'^(\w+)\s*=\s*"((?:\\.|[^"\\])*)"', hm.group(2), re.M))
            data.setdefault(hm.group(1), []).append(entry)
    except Exception as e:  # malformed TOML
        problems.append(f"allowlist.toml  unparseable ({e})")
        return trait_keys, inherent_keys

    for table, owner_field, target in (("methods", "trait", trait_keys),
                                       ("inherent_methods", "type_name", inherent_keys)):
        for entry in data.get(table, []):
            owner, method = entry.get(owner_field), entry.get("method")
            if not owner or not method:
                problems.append(f"allowlist.toml  [[{table}]] entry missing `{owner_field}`/`method`: {entry}")
                continue
            key = f"{owner}::{method}"
            if not str(entry.get("rationale", "")).strip():
                problems.append(f"allowlist.toml  {key}  has no rationale -- every exemption must say why")
            if key in target:
                problems.append(f"allowlist.toml  {key}  listed twice")
            target[key] = entry.get("rationale", "")
    for table in data:
        if table not in ("methods", "inherent_methods"):
            problems.append(f"allowlist.toml  unknown table [[{table}]] -- this oracle reads only methods / inherent_methods")
    return trait_keys, inherent_keys


def first_non_self_arg(sig: str) -> Optional[str]:
    s = sig.strip()
    s = re.sub(r"^\s*(?:&\s*(?:'\w+\s+)?(?:mut\s+)?|mut\s+)?self\s*(?::[^,]*)?\s*,?\s*", "", s, count=1) \
        if re.match(r"^\s*(?:&\s*(?:'\w+\s+)?(?:mut\s+)?|mut\s+)?self\b", s) else s
    s = s.strip()
    if not s:
        return None
    depth = 0
    out = []
    prev = ""
    for c in s:
        if c in "<([":
            depth += 1
        elif c in ")]" or (c == ">" and prev != "-"):
            depth -= 1
        elif c == "," and depth == 0:
            break
        out.append(c)
        prev = c
    return "".join(out).strip()


def is_scope_arg(arg: str) -> bool:
    return bool(re.search(r"&\s*TenantScope\b", arg) or re.search(r"&\s*ProjectScope\b", arg))


def iter_fn_sigs(content: str, start: int, end: int, pattern: re.Pattern):
    """Yield (name, fn_kw_abs_index, sig_text) for fns matched by `pattern`
    (whose group 1 is the name and which ends right after the name) inside
    content[start:end]. Generic parameter lists are skipped, balanced."""
    for fm in pattern.finditer(content, start, end):
        j = skip_generics(content, fm.end())
        while j < end and content[j].isspace():
            j += 1
        if j >= end or content[j] != "(":
            continue
        close = find_matching(content, j, "(", ")")
        if close < 0:
            continue
        kw = content.index("fn", fm.start(), fm.end())
        yield fm.group(1), kw, content[j + 1:close]


TRAIT_FN_RE = re.compile(r"(?<![\w])fn\s+(\w+)")
PUB_FN_RE = re.compile(
    r"\bpub(?!\s*\()\s+(?:(?:const|async|unsafe)\s+|extern\s+\"[^\"]*\"\s+)*fn\s+(\w+)"
)


def find_inherent_impl_ranges(content: str):
    ranges = []
    for m in re.finditer(r"\bimpl\b", content):
        j = skip_generics(content, m.end())
        tm = re.match(r"\s*(\w+)", content[j:])
        if not tm:
            continue
        k = skip_generics(content, j + tm.end())
        if re.match(r"\s*\{", content[k:]):
            brace = content.index("{", k)
            close = find_matching(content, brace, "{", "}")
            if close > 0:
                ranges.append((tm.group(1), brace, close))
    return ranges


def probe_b(errors: List[str]) -> Tuple[List[str], List[str]]:
    offenders: List[str] = []
    problems: List[str] = []
    src = REPO_CRATE / "src"
    if not src.is_dir():
        return [f"{rel(src)}  missing -- repository crate not found; nothing to check is a FAIL"], problems

    trait_keys, inherent_keys = load_allowlist(problems)
    seen_trait: Set[str] = set()
    seen_inherent: Set[str] = set()

    def check(where: Path, content: str, owner: str, name: str, kw: int, sig: str, allowed: Dict[str, str]):
        arg = first_non_self_arg(sig)
        if arg is None or is_scope_arg(arg):
            return
        if f"{owner}::{name}" in allowed:
            return
        offenders.append(
            f"{rel(where)}:{line_no(content, kw)}  {owner}::{name}  first non-self arg is "
            f"'{' '.join(arg.split())}' (expected &TenantScope or &ProjectScope)"
        )

    files = rust_files(src)
    if not files:
        offenders.append(f"{rel(src)}  contains no .rs files")
    for path in files:
        raw = read(path, errors)
        if raw is None:
            continue
        _, content = lex_views(raw)

        # 1) pub trait blocks.
        for tm in re.finditer(r"\bpub\s+trait\s+(\w+)[^{;]*\{", content):
            trait_name = tm.group(1)
            brace = tm.end() - 1
            close = find_matching(content, brace, "{", "}")
            if close < 0:
                continue
            for name, kw, sig in iter_fn_sigs(content, brace + 1, close, TRAIT_FN_RE):
                seen_trait.add(f"{trait_name}::{name}")
                check(path, content, trait_name, name, kw, sig, trait_keys)

        # 2) impl Trait for Type blocks.
        for im in re.finditer(r"\bimpl\b", content):
            j = skip_generics(content, im.end())
            hm = re.match(r"\s*(?:[\w:]+::)?(\w+)", content[j:])
            if not hm:
                continue
            k = skip_generics(content, j + hm.end())
            fm = re.match(r"\s+for\s+[^{;]*\{", content[k:])
            if not fm:
                continue
            trait_name = hm.group(1)
            brace = k + fm.end() - 1
            close = find_matching(content, brace, "{", "}")
            if close < 0:
                continue
            for name, kw, sig in iter_fn_sigs(content, brace + 1, close, TRAIT_FN_RE):
                seen_trait.add(f"{trait_name}::{name}")
                check(path, content, trait_name, name, kw, sig, trait_keys)

        # 3) inherent / free `pub fn` (pub(crate)/pub(super) excluded).
        inherent = find_inherent_impl_ranges(content)
        for name, kw, sig in iter_fn_sigs(content, 0, len(content), PUB_FN_RE):
            enclosing = ""
            for type_name, s, e in inherent:
                if s < kw < e:
                    enclosing = type_name
            owner = enclosing or "<free>"
            seen_inherent.add(f"{owner}::{name}")
            check(path, content, owner, name, kw, sig, inherent_keys if enclosing else {})

    for key in trait_keys:
        if key not in seen_trait:
            problems.append(f"allowlist.toml  [[methods]] {key}  names no trait method in the repository crate -- stale exemption")
    for key in inherent_keys:
        if key not in seen_inherent:
            problems.append(f"allowlist.toml  [[inherent_methods]] {key}  names no inherent pub fn in the repository crate -- stale exemption")

    seen, unique = set(), []
    for o in offenders:
        if o not in seen:
            seen.add(o)
            unique.append(o)
    return unique, problems


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--root", type=Path, default=None, help="repo root to scan (default: this checkout)")
    ap.add_argument("--full", action="store_true", help="accepted for suite uniformity; static-only oracle")
    args = ap.parse_args()
    if args.root is not None:
        if not args.root.is_dir():
            print(f"ERROR multi-tenant-isolation-check: --root {args.root} is not a directory")
            return 2
        set_root(args.root)

    errors: List[str] = []
    a = probe_a(errors)
    b, allow_problems = probe_b(errors)
    total = len(a) + len(b) + len(allow_problems) + len(errors)
    if total == 0:
        print("PASS multi-tenant-isolation-check")
        print("  Probe A (DB access outside repository: sqlx whitelist, bare builders, string SQL, pool/tx executors): clean")
        print("  Probe B (repository-method scope discipline + allowlist hygiene): clean")
        return 0

    print(f"FAIL multi-tenant-isolation-check ({total} offender(s))")
    for title, items in (
        ("Probe A offenders (database reachable outside crates/feedbackmonk-repository/):", a),
        ("Probe B offenders (public repository fn missing &TenantScope/&ProjectScope):", b),
        ("Allowlist problems:", allow_problems),
        ("Unscannable files:", errors),
    ):
        if items:
            print()
            print(title)
            for o in items:
                print(f"  {o}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
