#!/usr/bin/env python3
"""feedback-erasure-completeness Verification Oracle (canonical implementation).

Proves FROM CODE that the right-to-erasure paths leave no residual submitter
data behind: the per-item `DELETE /api/v1/projects/{project}/me/feedback/{id}`
(Phase A A1, D-A1) and the user-level "forget me" `DELETE .../me` (P1-16 / M1).
A happy-path "the row is gone" test cannot see any of the failures below.

  A) ERASURE ORDER + OWNERSHIP (handlers/me_feedback.rs, repository attachments):
     both handlers purge attachment OBJECT BYTES (`storage.delete`, error
     propagated with `?`, inside a loop over the listed keys) BEFORE the row
     delete. The per-item handler runs its ownership check
     (`get_for_end_user(.., claims.sub, ..).await?`) BEFORE it lists or purges
     anything — `resolve_feedback_uuid` is not sub-scoped, so without the gate a
     caller could purge another user's screenshots. The bulk path lists keys via
     `list_storage_keys_for_end_user(.., claims.sub)`, whose SQL is scoped by
     tenant + project + `end_user_sub` bound to the sub parameter.

  B) CASCADE COMPLETENESS (migrations, parsed into a final schema): every FK to
     `feedback` — `REFERENCES feedback(id)`, `REFERENCES feedback` (no column),
     inline or as a table/ALTER constraint — is ON DELETE CASCADE or SET NULL;
     a raw count of `REFERENCES feedback` must equal the parsed FKs, so a form
     the parser misses fails instead of passing; and every `*feedback_id`
     column outside `feedback` carries an FK to feedback (a feedback-id column
     with no FK survives erasure pointing at nothing, or at a recycled id).

  C) SCOPED DELETES (repository feedback.rs): every `DELETE` statement inside
     `delete_for_end_user` / `erase_all_for_end_user` is scoped IN ITS OWN
     WHERE by `tenant_id = $n` (scope.tenant_id()), `project_id = $n`
     (scope.project_id()) and an end-user key bound to the `end_user_sub`
     parameter, with no top-level OR — or it is `WHERE id = $n` bound to
     `<row>.id` where `<row>` comes from an earlier `SELECT ... FROM feedback`
     scoped the same way. A scope column that appears only in a comment or in
     some other statement of the fn no longer counts.

  D) DERIVED-TEXT SCRUB (repository feedback.rs, scrutiny P0-1): the shared
     `scrub_cluster_derived_text` helper UPDATE-scrubs feedback_clusters /
     recommendations / work_orders / analysis_sweeps, and both erasure fns call it.

  E) END-USER TABLE COVERAGE (migrations x erase_all_for_end_user): every table
     in the final schema carrying an end-user identity column
     (`end_user_*`, `*_sub`, `voter_id`, `submitter_id`) is erased by a scoped
     DELETE in `erase_all_for_end_user` keyed on that column — or is listed in
     CASCADE_COVERED with a rationale and verified to hang off feedback by a
     NOT NULL `ON DELETE CASCADE` FK. A new user-keyed table fails until erased.

  --full) runs tests/me_feedback_delete.rs against the real DB (behavioral drift).

Exit 0 PASS, 1 FAIL, 2 environment error. `--root <path>` points the probes at
another tree (used by the adversarial self-test; defaults to the repo root).

Lineage: Phase A A1 (D-A1), P1-16 / M1, scrutiny P0-1, DEC-FBR-04 / DEC-FBR-03,
DEC-FBR-02 / Q24. Mirrors translation-egress-q24-isolation (detection-from-code).
"""
from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, List, Optional, Tuple

SCRIPT_DIR = Path(__file__).resolve().parent

DELETE_HANDLER_FN = "delete_my_feedback"
BULK_HANDLER_FN = "erase_all_my_feedback"
ROW_DELETE_FN = "delete_for_end_user"
BULK_ERASE_FN = "erase_all_for_end_user"
SCRUB_HELPER_FN = "scrub_cluster_derived_text"
KEYS_FOR_USER_FN = "list_storage_keys_for_end_user"

# End-user identity columns: a table carrying one holds data keyed to a person.
IDENTITY_COL_RE = re.compile(r"^(end_user_\w+|sub|\w+_sub|voter_id|submitter_id)$")

# Tables whose end-user identity column is removed by the feedback FK cascade
# rather than by an explicit DELETE in erase_all_for_end_user. Each entry is
# VERIFIED (Probe E): the named FK column must be NOT NULL and
# REFERENCES feedback ... ON DELETE CASCADE. Never add an entry for a table
# whose identity column can name someone OTHER than the referenced feedback's
# submitter (votes are the counter-example: a user votes on others' feedback).
CASCADE_COVERED: Dict[str, Tuple[str, str]] = {
    "submit_idempotency": (
        "feedback_id",
        "submitter_id is the submitter of the feedback row feedback_id names "
        "(00022: the same identity the row is attributed to), so erasing the "
        "row cascades the only rows carrying that submitter_id.",
    ),
}


@dataclass
class Paths:
    root: Path
    me_feedback: Path = field(init=False)
    repo_feedback: Path = field(init=False)
    repo_attachments: Path = field(init=False)
    migrations: Path = field(init=False)
    delete_test: Path = field(init=False)

    def __post_init__(self) -> None:
        c = self.root / "crates"
        self.me_feedback = c / "feedbackmonk-api" / "src" / "handlers" / "me_feedback.rs"
        self.repo_feedback = c / "feedbackmonk-repository" / "src" / "feedback.rs"
        self.repo_attachments = c / "feedbackmonk-repository" / "src" / "attachments.rs"
        self.migrations = self.root / "migrations"
        self.delete_test = c / "feedbackmonk-api" / "tests" / "me_feedback_delete.rs"

    def rel(self, p: Path) -> str:
        try:
            return str(p.relative_to(self.root)).replace("\\", "/")
        except ValueError:
            return str(p).replace("\\", "/")


# ---------------------------------------------------------------------------
# Rust lexing: comments blanked (nc), and comments + string bodies blanked
# (code) — both the same length as the source, so offsets line up.
# ---------------------------------------------------------------------------

_TOKEN = re.compile(
    r"//[^\n]*"
    r"|/\*"
    r'|(?<![\w])b?r(#*)"'
    r'|"(?:[^"\\]|\\.)*"'
    r"|'(?:\\(?:u\{[0-9a-fA-F]+\}|x..|.)|[^\\'\n])'",
    re.S,
)
_NONNL = re.compile(r"[^\r\n]")


def _blank(seg: str) -> str:
    return _NONNL.sub(" ", seg)


class RustSrc:
    def __init__(self, text: str):
        self.text = text
        self.strings: List[Tuple[int, int]] = []  # (content_start, content_end)
        nc: List[str] = []
        code: List[str] = []
        n, pos = len(text), 0
        while True:
            m = _TOKEN.search(text, pos)
            if not m:
                nc.append(text[pos:])
                code.append(text[pos:])
                break
            nc.append(text[pos:m.start()])
            code.append(text[pos:m.start()])
            tok = m.group(0)
            if tok.startswith("//"):
                end = m.end()
                nc.append(_blank(tok))
                code.append(_blank(tok))
            elif tok == "/*":
                depth, j = 0, m.start()
                while j < n:
                    if text.startswith("/*", j):
                        depth, j = depth + 1, j + 2
                    elif text.startswith("*/", j):
                        depth, j = depth - 1, j + 2
                        if depth == 0:
                            break
                    else:
                        j += 1
                end = j
                nc.append(_blank(text[m.start():end]))
                code.append(_blank(text[m.start():end]))
            elif tok.endswith('"') and m.group(1) is not None and not tok.startswith('"'):
                hashes = m.group(1)
                cs = m.end()
                ce = text.find('"' + hashes, cs)
                ce = n if ce == -1 else ce
                end = min(n, ce + 1 + len(hashes))
                self.strings.append((cs, ce))
                nc.append(text[m.start():end])
                code.append(tok + _blank(text[cs:ce]) + text[ce:end])
            else:  # "..." string or char literal
                end = m.end()
                if tok.startswith('"'):
                    self.strings.append((m.start() + 1, end - 1))
                nc.append(tok)
                code.append(tok[0] + _blank(tok[1:-1]) + tok[-1])
            pos = end
        self.nc = "".join(nc)
        self.code = "".join(code)
        assert len(self.nc) == len(self.code) == n

    def match_close(self, open_idx: int) -> int:
        """Index of the bracket closing the one at open_idx (in code), or -1."""
        pairs = {"(": ")", "{": "}", "[": "]"}
        o = self.code[open_idx]
        c = pairs[o]
        depth = 0
        for k in range(open_idx, len(self.code)):
            if self.code[k] == o:
                depth += 1
            elif self.code[k] == c:
                depth -= 1
                if depth == 0:
                    return k
        return -1

    def fn_bodies(self, name: str) -> List[Tuple[int, int]]:
        """(start, end) of every body of `fn name` (skips `fn name(..);` decls)."""
        out = []
        for m in re.finditer(rf"\bfn\s+{re.escape(name)}\s*[(<]", self.code):
            k = m.end() - 1
            if self.code[k] == "<":
                k = self._skip_angle(k)
                k = self.code.find("(", k)
            close = self.match_close(k)
            if close == -1:
                continue
            j = close + 1
            while j < len(self.code) and self.code[j] not in "{;":
                j += 1
            if j >= len(self.code) or self.code[j] == ";":
                continue
            end = self.match_close(j)
            if end != -1:
                out.append((j, end + 1))
        return out

    def _skip_angle(self, k: int) -> int:
        depth = 0
        for j in range(k, len(self.code)):
            if self.code[j] == "<":
                depth += 1
            elif self.code[j] == ">" and self.code[j - 1] != "-":
                depth -= 1
                if depth == 0:
                    return j + 1
        return len(self.code)

    def split_args(self, open_idx: int) -> List[Tuple[int, int]]:
        close = self.match_close(open_idx)
        args, depth, start = [], 0, open_idx + 1
        for k in range(open_idx + 1, close):
            ch = self.code[k]
            if ch in "([{":
                depth += 1
            elif ch in ")]}":
                depth -= 1
            elif ch == "," and depth == 0:
                args.append((start, k))
                start = k + 1
        if self.code[start:close].strip():
            args.append((start, close))
        return args

    def literal_in(self, a: int, b: int) -> Optional[str]:
        for s, e in self.strings:
            if a <= s and e <= b:
                return self.text[s:e]
        return None

    def arg_text(self, span: Tuple[int, int]) -> str:
        return re.sub(r"\s+", "", self.nc[span[0]:span[1]])


@dataclass
class SqlCall:
    pos: int
    sql: str
    args: List[str]  # $1 -> args[0]
    binding: Optional[str]  # `let Some(row) =` / `let row =` target, if any


def sql_calls(src: RustSrc, a: int, b: int) -> List[SqlCall]:
    """Every sqlx query in [a, b): macro form (`query!` / `query_as!` /
    `query_scalar!`) or function form with a `.bind(..)` chain."""
    calls = []
    for m in re.finditer(r"\b(query|query_as|query_scalar)(_unchecked)?\s*(!)?\s*(::\s*<)?", src.code[a:b]):
        start = a + m.start()
        k = a + m.end()
        if m.group(4):
            k = src._skip_angle(k - 1)
        while k < b and src.code[k].isspace():
            k += 1
        if k >= b or src.code[k] != "(":
            continue
        spans = src.split_args(k)
        lit_idx = next((i for i, sp in enumerate(spans) if src.literal_in(*sp) is not None), None)
        if lit_idx is None:
            continue
        sql = strip_sql_comments(src.literal_in(*spans[lit_idx]))  # `--`/`/* */` inside SQL
        if m.group(3):  # macro: args follow the literal
            args = [src.arg_text(sp) for sp in spans[lit_idx + 1:]]
        else:
            args = []
            j = src.match_close(k) + 1
            while True:
                bm = re.compile(r"\s*\.\s*bind\s*\(").match(src.code, j)
                if not bm:
                    break
                op = bm.end() - 1
                args.append(src.arg_text((op + 1, src.match_close(op))))
                j = src.match_close(op) + 1
        pre = src.code[max(a, start - 120):start]
        lm = re.search(r"let\s+(?:Some\s*\(\s*(\w+)\s*\)|(?:mut\s+)?(\w+))\s*=\s*(?:sqlx\s*::\s*)?$", pre)
        binding = (lm.group(1) or lm.group(2)) if lm else None
        calls.append(SqlCall(start, sql, args, binding))
    return calls


def where_clause(sql: str) -> Optional[str]:
    m = re.search(r"\bWHERE\b(.*)$", sql, re.IGNORECASE | re.DOTALL)
    return m.group(1) if m else None


def where_bindings(where: str) -> Dict[str, List[int]]:
    """column -> [$n, ...] for every `[alias.]col = $n` in the WHERE."""
    out: Dict[str, List[int]] = {}
    for m in re.finditer(r"(?:\w+\.)?(\w+)\s*=\s*\$(\d+)", where):
        out.setdefault(m.group(1).lower(), []).append(int(m.group(2)))
    return out


def arg_for(call: SqlCall, n: int) -> Optional[str]:
    return call.args[n - 1] if 1 <= n <= len(call.args) else None


def scope_problems(call: SqlCall, identity_arg: str, identity_cols: Optional[set] = None) -> List[str]:
    """Why this statement's own WHERE is NOT scoped to (tenant, project, user)."""
    where = where_clause(call.sql)
    if where is None:
        return ["has no WHERE clause"]
    probs = []
    if re.search(r"\bOR\b", re.sub(r"\([^()]*\)", "", where), re.IGNORECASE):
        probs.append("has a top-level OR in its WHERE (widens the erased set)")
    binds = where_bindings(where)
    for col, want in (("tenant_id", "scope.tenant_id()"), ("project_id", "scope.project_id()")):
        if not any(arg_for(call, n) == want for n in binds.get(col, [])):
            probs.append(f"WHERE does not bind `{col} = $n` to `{want}`")
    ids = [c for c in binds if (identity_cols is None and IDENTITY_COL_RE.match(c)) or (identity_cols and c in identity_cols)]
    if not any(arg_for(call, n) == identity_arg for c in ids for n in binds[c]):
        probs.append(f"WHERE does not bind an end-user key column to `{identity_arg}`")
    return probs


# ---------------------------------------------------------------------------
# SQL schema (migrations) parsing
# ---------------------------------------------------------------------------

@dataclass
class Column:
    name: str
    definition: str
    not_null: bool
    fk_table: Optional[str] = None
    on_delete: Optional[str] = None


@dataclass
class Fk:
    where: str
    table: str
    column: str
    target: str
    on_delete: Optional[str]


def strip_sql_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", " ", text, flags=re.DOTALL)
    return re.sub(r"--[^\n]*", "", text)


def split_top(s: str, sep: str = ",") -> List[str]:
    parts, depth, start = [], 0, 0
    for k, ch in enumerate(s):
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        elif ch == sep and depth == 0:
            parts.append(s[start:k])
            start = k + 1
    parts.append(s[start:])
    return [p.strip() for p in parts if p.strip()]


REF_RE = re.compile(r"\bREFERENCES\s+(\w+)\s*(?:\(\s*(\w+)\s*\))?", re.IGNORECASE)
ON_DEL_RE = re.compile(r"\bON\s+DELETE\s+(CASCADE|SET\s+NULL|SET\s+DEFAULT|RESTRICT|NO\s+ACTION)", re.IGNORECASE)


def _fk_of(text: str) -> Tuple[Optional[str], Optional[str]]:
    m = REF_RE.search(text)
    if not m:
        return None, None
    od = ON_DEL_RE.search(text, m.end())
    return m.group(1).lower(), (re.sub(r"\s+", " ", od.group(1).upper()) if od else None)


def parse_schema(mig_dir: Path) -> Tuple[Dict[str, Dict[str, Column]], List[Fk], int]:
    tables: Dict[str, Dict[str, Column]] = {}
    fks: List[Fk] = []
    raw_refs = 0

    def add_col(table: str, item: str, where: str):
        m = re.match(r'(?:IF\s+NOT\s+EXISTS\s+)?"?(\w+)"?\s+(.*)$', item, re.IGNORECASE | re.DOTALL)
        if not m:
            return
        name, rest = m.group(1).lower(), m.group(2)
        tgt, od = _fk_of(rest)
        col = Column(name, rest, bool(re.search(r"\bNOT\s+NULL\b|\bPRIMARY\s+KEY\b", rest, re.I)), tgt, od)
        tables.setdefault(table, {})[name] = col
        if tgt:
            fks.append(Fk(where, table, name, tgt, od))

    def add_constraint(table: str, item: str, where: str):
        m = re.search(r"\bFOREIGN\s+KEY\s*\(([^)]*)\)", item, re.IGNORECASE)
        tgt, od = _fk_of(item)
        if m and tgt:
            for c in split_top(m.group(1)):
                c = c.strip('" ').lower()
                fks.append(Fk(where, table, c, tgt, od))
                col = tables.get(table, {}).get(c)
                if col:
                    col.fk_table, col.on_delete = tgt, od

    for sql in sorted(mig_dir.glob("*.sql")):
        text = strip_sql_comments(sql.read_text(encoding="utf-8"))
        raw_refs += len(re.findall(r"\bREFERENCES\s+feedback\b", text, re.IGNORECASE))
        where = f"migrations/{sql.name}"
        for stmt in text.split(";"):
            s = stmt.strip()
            m = re.match(r"CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?(\w+)\s*\((.*)\)\s*$", s, re.I | re.S)
            if m:
                t = m.group(1).lower()
                tables.setdefault(t, {})
                for item in split_top(m.group(2)):
                    if re.match(r"(CONSTRAINT|PRIMARY|UNIQUE|CHECK|FOREIGN|EXCLUDE)\b", item, re.I):
                        add_constraint(t, item, where)
                    else:
                        add_col(t, item, where)
                continue
            m = re.match(r"ALTER\s+TABLE\s+(?:IF\s+EXISTS\s+)?(?:ONLY\s+)?(\w+)\s+(.*)$", s, re.I | re.S)
            if m:
                t = m.group(1).lower()
                for act in split_top(m.group(2)):
                    am = re.match(r"ADD\s+COLUMN\s+(.*)$", act, re.I | re.S)
                    if am:
                        add_col(t, am.group(1), where)
                        continue
                    if re.match(r"ADD\s+(CONSTRAINT|FOREIGN)\b", act, re.I):
                        add_constraint(t, act, where)
                        continue
                    dm = re.match(r"DROP\s+COLUMN\s+(?:IF\s+EXISTS\s+)?(\w+)", act, re.I)
                    if dm:
                        tables.get(t, {}).pop(dm.group(1).lower(), None)
                        continue
                    nm = re.match(r"ALTER\s+COLUMN\s+(\w+)\s+(SET|DROP)\s+NOT\s+NULL", act, re.I)
                    if nm and nm.group(1).lower() in tables.get(t, {}):
                        tables[t][nm.group(1).lower()].not_null = nm.group(2).upper() == "SET"
                continue
            m = re.match(r"DROP\s+TABLE\s+(?:IF\s+EXISTS\s+)?(\w+)", s, re.I)
            if m:
                tables.pop(m.group(1).lower(), None)
    return tables, fks, raw_refs


# ---------------------------------------------------------------------------
# Probes
# ---------------------------------------------------------------------------

_CACHE: Dict[Path, RustSrc] = {}


def _load(p: Path, P: Paths, offs: List[str]) -> Optional[RustSrc]:
    if not p.exists():
        offs.append(f"{P.rel(p)} does not exist — the erasure code it holds is gone")
        return None
    if p not in _CACHE:
        _CACHE[p] = RustSrc(p.read_text(encoding="utf-8"))
    return _CACHE[p]


def _one_body(src: RustSrc, fn: str, path: str, offs: List[str]) -> Optional[Tuple[int, int]]:
    bodies = src.fn_bodies(fn)
    if not bodies:
        offs.append(f"{path}: `fn {fn}` (with a body) not found — a required erasure step is gone")
        return None
    return bodies[-1]  # the impl, not a trait default


def _calls_with_arg(src: RustSrc, a: int, b: int, method: str, arg: str) -> List[int]:
    """Positions of `method(` calls in [a,b) that pass `arg` (ignoring `&`)."""
    out = []
    for m in re.finditer(rf"\b{method}\s*\(", src.code[a:b]):
        op = a + m.end() - 1
        args = [x.lstrip("&") for x in (src.arg_text(sp) for sp in src.split_args(op))]
        out.append((a + m.start(), arg in args))
    return out


def _stmt_propagates(src: RustSrc, pos: int) -> bool:
    """The statement containing pos ends in `?;` (its error is propagated)."""
    end = src.code.find(";", pos)
    seg = src.code[pos:end].rstrip()
    return seg.endswith("?")


def probe_a(P: Paths) -> List[str]:
    offs: List[str] = []
    src = _load(P.me_feedback, P, offs)
    if src is None:
        return offs
    path = P.rel(P.me_feedback)

    for handler, list_fn, row_fn, needs_own in (
        (DELETE_HANDLER_FN, "list_storage_keys_for_feedback", ROW_DELETE_FN, True),
        (BULK_HANDLER_FN, KEYS_FOR_USER_FN, BULK_ERASE_FN, False),
    ):
        span = _one_body(src, handler, path, offs)
        if span is None:
            continue
        a, b = span
        where = f"{path}::{handler}"
        body = src.code[a:b]

        auth = re.search(r"\bauthenticate_parts\s*\(", body)
        list_m = re.search(rf"\blet\s+(\w+)\s*=[^;]*?\b{list_fn}\s*\(", body, re.S)
        purge = re.search(r"\bstorage\s*\.\s*delete\s*\(", body)
        rows = _calls_with_arg(src, a, b, row_fn, "claims.sub")

        if not auth:
            offs.append(f"{where}: no `authenticate_parts(` — the caller's identity is never established.")
        if not list_m:
            offs.append(f"{where}: no `let <keys> = ...{list_fn}(..)` — it cannot know which object keys to purge.")
        if not purge:
            offs.append(f"{where}: no `storage.delete(` — attachment object BYTES are never purged "
                        "(the DB cascade removes only metadata rows).")
        if not rows:
            offs.append(f"{where}: no `{row_fn}(` call — the rows are never erased.")
        elif not all(ok for _, ok in rows):
            offs.append(f"{where}: `{row_fn}(` is not passed `claims.sub` — erasure is not keyed to the caller.")
        if not (list_m and purge and rows):
            continue
        list_pos, purge_pos, row_pos = a + list_m.start(), a + purge.start(), rows[0][0]
        if list_fn == KEYS_FOR_USER_FN and not all(ok for _, ok in _calls_with_arg(src, a, b, list_fn, "claims.sub")):
            offs.append(f"{where}: `{list_fn}(` is not passed `claims.sub` — it could list another user's keys.")
        if auth and a + auth.start() > list_pos:
            offs.append(f"{where}: `authenticate_parts` runs AFTER the key listing.")
        loop = re.search(rf"\bfor\s+[\w(), ]+\s+in\s+&?\s*{list_m.group(1)}\b", body)
        if not loop or a + loop.start() > purge_pos:
            offs.append(f"{where}: `storage.delete(` is not inside a `for .. in {list_m.group(1)}` loop over "
                        "the listed keys — not every object is purged.")
        if not _stmt_propagates(src, purge_pos):
            offs.append(f"{where}: the `storage.delete(` result is not propagated with `?` — a failed purge "
                        "would still proceed to delete the rows, orphaning the bytes.")
        if not (list_pos < purge_pos < row_pos):
            offs.append(f"{where}: order must be list keys -> purge bytes -> `{row_fn}`; found list@{list_pos} "
                        f"purge@{purge_pos} row-delete@{row_pos} (bytes-before-rows, D-A1).")
        if needs_own:
            owns = _calls_with_arg(src, a, b, "get_for_end_user", "claims.sub")
            if not owns or not owns[0][1]:
                offs.append(f"{where}: no ownership check `get_for_end_user(.., claims.sub, ..)` — "
                            "`resolve_feedback_uuid` is not sub-scoped, so another user's bytes could be purged.")
            else:
                own_pos = owns[0][0]
                if not _stmt_propagates(src, own_pos):
                    offs.append(f"{where}: the ownership check's result is not propagated with `?` — "
                                "an un-owned id would still reach the byte purge.")
                if own_pos > list_pos or own_pos > purge_pos:
                    offs.append(f"{where}: the ownership check `get_for_end_user` runs AFTER the key "
                                "listing / byte purge — un-owned rows get their bytes purged (DEC-FBR-04).")

    # The bulk key listing itself must be scoped to the sub.
    asrc = _load(P.repo_attachments, P, offs)
    if asrc is not None:
        apath = P.rel(P.repo_attachments)
        span = _one_body(asrc, KEYS_FOR_USER_FN, apath, offs)
        if span:
            calls = sql_calls(asrc, *span)
            if not calls:
                offs.append(f"{apath}::{KEYS_FOR_USER_FN}: no sqlx query found.")
            for c in calls:
                for p in scope_problems(c, "end_user_sub", {"end_user_sub"}):
                    offs.append(f"{apath}::{KEYS_FOR_USER_FN}: key listing {p} — the bulk purge "
                                "could delete another user's objects.")
    return offs


def probe_b(P: Paths, schema) -> List[str]:
    tables, fks, raw_refs = schema
    offs: List[str] = []
    to_fb = [f for f in fks if f.target == "feedback"]
    for f in to_fb:
        if f.on_delete not in ("CASCADE", "SET NULL"):
            offs.append(f"{f.where}: `{f.table}.{f.column}` REFERENCES feedback with "
                        f"ON DELETE {f.on_delete or 'NO ACTION (default)'} — blocks erasure or orphans child rows.")
    if raw_refs != len(to_fb):
        offs.append(f"migrations: {raw_refs} `REFERENCES feedback` occurrences but only {len(to_fb)} parsed "
                    "as FKs — a form this oracle cannot read; extend the parser rather than pass blind.")
    for t, cols in sorted(tables.items()):
        if t == "feedback":
            continue
        for c in cols.values():
            if re.search(r"feedback_(id|uuid)$", c.name) and c.fk_table != "feedback":
                offs.append(f"migrations: `{t}.{c.name}` looks like a feedback id but has no FK to feedback — "
                            "erasure neither removes nor nulls it.")
    return offs


def probe_c(P: Paths) -> List[str]:
    offs: List[str] = []
    src = _load(P.repo_feedback, P, offs)
    if src is None:
        return offs
    path = P.rel(P.repo_feedback)
    for fn in (ROW_DELETE_FN, BULK_ERASE_FN):
        span = _one_body(src, fn, path, offs)
        if span is None:
            continue
        calls = sql_calls(src, *span)
        deletes = [c for c in calls if re.match(r"\s*DELETE\b", c.sql, re.I)]
        if not any(re.match(r"\s*DELETE\s+FROM\s+feedback\b", c.sql, re.I) for c in deletes):
            offs.append(f"{path}::{fn}: no `DELETE FROM feedback` — the rows are never hard-deleted.")
        for c in deletes:
            probs = scope_problems(c, "end_user_sub")
            if probs:
                probs = _via_scoped_select(c, calls, probs)
            tbl = re.match(r"\s*DELETE\s+FROM\s+(\w+)", c.sql, re.I)
            for p in probs:
                offs.append(f"{path}::{fn}: `DELETE FROM {tbl.group(1) if tbl else '?'}` {p} "
                            "(DEC-FBR-04 / DEC-FBR-03).")
    return offs


def _via_scoped_select(c: SqlCall, calls: List[SqlCall], probs: List[str]) -> List[str]:
    """Accept `DELETE .. WHERE id = $n` whose $n is `<row>.id`, <row> bound by an
    earlier scoped `SELECT .. FROM feedback`."""
    where = where_clause(c.sql) or ""
    m = re.fullmatch(r"\s*id\s*=\s*\$(\d+)\s*", where)
    if not m or not re.match(r"\s*DELETE\s+FROM\s+feedback\b", c.sql, re.I):
        return probs
    arg = arg_for(c, int(m.group(1))) or ""
    am = re.fullmatch(r"(\w+)\.id", arg)
    if not am:
        return probs + [f"(`id = ${m.group(1)}` bound to `{arg}`, not a scoped `<row>.id`)"]
    src_sel = [s for s in calls if s.binding == am.group(1) and s.pos < c.pos]
    if not src_sel:
        return probs + [f"(`{arg}`: no earlier `let {am.group(1)} = sqlx::query!(SELECT ..)` in this fn)"]
    sel = src_sel[-1]
    if not re.search(r"\bFROM\s+feedback\b", sel.sql, re.I):
        return [f"— the row `{am.group(1)}` it deletes by id is not read FROM feedback"]
    sp = scope_problems(sel, "end_user_sub", {"end_user_sub"})
    return [f"— deletes by `{arg}`, whose SELECT {p}" for p in sp]


def probe_d(P: Paths) -> List[str]:
    offs: List[str] = []
    src = _load(P.repo_feedback, P, offs)
    if src is None:
        return offs
    path = P.rel(P.repo_feedback)
    span = _one_body(src, SCRUB_HELPER_FN, path, offs)
    if span is None:
        return offs
    sqls = "\n".join(c.sql for c in sql_calls(src, *span))
    for table, upd_re, scrub_re in (
        ("feedback_clusters", r"UPDATE\s+feedback_clusters\b", r"\bsummary\s*=\s*NULL"),
        ("recommendations", r"UPDATE\s+recommendations\b", r"\bbody\s*=\s*''"),
        ("work_orders", r"UPDATE\s+work_orders\b", r"\binstructions\s*=\s*''"),
        ("analysis_sweeps", r"UPDATE\s+analysis_sweeps\b", r"\bdigest_summary\s*=\s*NULL"),
    ):
        if not re.search(upd_re, sqls, re.I):
            offs.append(f"{path}::{SCRUB_HELPER_FN}: no `UPDATE {table}` — its derived text survives erasure.")
        elif not re.search(scrub_re, sqls, re.I):
            offs.append(f"{path}::{SCRUB_HELPER_FN}: `UPDATE {table}` does not clear its free text (`{scrub_re}`).")
    for caller in (ROW_DELETE_FN, BULK_ERASE_FN):
        cs = src.fn_bodies(caller)
        if not cs:
            offs.append(f"{path}: erasure fn `{caller}` missing.")
        elif not re.search(rf"\b{SCRUB_HELPER_FN}\s*\(", src.code[cs[-1][0]:cs[-1][1]]):
            offs.append(f"{path}::{caller}: does not call `{SCRUB_HELPER_FN}` — this path skips the P0-1 scrub.")
    return offs


def probe_e(P: Paths, schema) -> List[str]:
    tables, _fks, _ = schema
    offs: List[str] = []
    src = _load(P.repo_feedback, P, offs)
    if src is None:
        return offs
    path = P.rel(P.repo_feedback)
    span = _one_body(src, BULK_ERASE_FN, path, offs)
    if span is None:
        return offs
    calls = sql_calls(src, *span)
    user_tables = {t: sorted(c for c in cols if IDENTITY_COL_RE.match(c)) for t, cols in tables.items()}
    user_tables = {t: c for t, c in user_tables.items() if c}
    if "feedback" not in user_tables:
        offs.append("migrations: `feedback.end_user_sub` not found — the schema parse is broken.")
    for t, cols in sorted(user_tables.items()):
        erased = False
        for c in calls:
            if re.match(rf"\s*DELETE\s+FROM\s+{t}\b", c.sql, re.I) and not scope_problems(c, "end_user_sub", set(cols)):
                erased = True
        if erased:
            continue
        if t in CASCADE_COVERED:
            fkcol = CASCADE_COVERED[t][0]
            col = tables[t].get(fkcol)
            if col and col.fk_table == "feedback" and col.on_delete == "CASCADE" and col.not_null:
                continue
            offs.append(f"migrations: `{t}` is CASCADE_COVERED via `{fkcol}`, but that column is not a NOT NULL "
                        "`REFERENCES feedback ON DELETE CASCADE` — its end-user rows survive erasure.")
            continue
        offs.append(f"{path}::{BULK_ERASE_FN}: table `{t}` carries end-user key column(s) {cols} but is not "
                    "erased by a DELETE scoped (tenant, project, key = end_user_sub) — 'forget me' leaves it behind.")
    return offs


def probe_full(P: Paths, full: bool) -> Tuple[Optional[bool], str]:
    if not full:
        return None, "skipped (pass --full to run tests/me_feedback_delete.rs)"
    if not P.delete_test.exists():
        return False, f"{P.rel(P.delete_test)} is missing — the behavioral erasure test is gone"
    import subprocess  # only the --full leg needs it

    cmd = ["cargo", "test", "-p", "feedbackmonk-api", "--test", "me_feedback_delete"]
    try:
        proc = subprocess.run(cmd, cwd=str(P.root), capture_output=True, text=True, timeout=600)
    except FileNotFoundError:
        return None, "ENV: cargo not found"
    except subprocess.TimeoutExpired:
        return False, "me_feedback_delete tests timed out"
    if proc.returncode == 0:
        return True, "me_feedback_delete: all passed"
    tail = (proc.stdout + proc.stderr).strip().splitlines()[-8:]
    return False, "me_feedback_delete tests failed:\n      " + "\n      ".join(tail)


def main() -> int:
    ap = argparse.ArgumentParser(description="feedback-erasure-completeness oracle")
    ap.add_argument("--full", action="store_true", help="also run tests/me_feedback_delete.rs")
    ap.add_argument("--root", type=Path, default=SCRIPT_DIR.parents[2],
                    help="tree to inspect (default: this repository)")
    args = ap.parse_args()
    P = Paths(args.root.resolve())
    if not P.migrations.is_dir():
        print(f"ENV feedback-erasure-completeness: {P.rel(P.migrations)} not found under {P.root}")
        return 2
    schema = parse_schema(P.migrations)

    results = [
        ("A (bytes-before-rows + ownership)", probe_a(P),
         "list keys (sub-scoped) -> ownership gate -> `storage.delete(..)?` in a loop -> row delete."),
        ("B (FK cascade completeness)", probe_b(P, schema),
         "every FK to feedback is ON DELETE CASCADE/SET NULL; every *feedback_id column has one."),
        ("C (scoped DELETEs)", probe_c(P),
         "each erasure DELETE's own WHERE binds tenant_id, project_id and the end-user key to the caller."),
        ("D (derived-text scrub)", probe_d(P),
         "scrub_cluster_derived_text UPDATEs the four P5 tables; both erasure fns call it."),
        ("E (end-user table coverage)", probe_e(P, schema),
         "erase_all_for_end_user DELETEs from every end-user-keyed table (or it is CASCADE_COVERED)."),
    ]
    full_passed, full_msg = probe_full(P, args.full)
    failed = [r for r in results if r[1]]
    nfail = len(failed) + (1 if full_passed is False else 0)
    if nfail == 0:
        print("PASS feedback-erasure-completeness")
        for label, _, _ in results:
            print(f"  Probe {label}: clean")
        print(f"  Probe --full (behavioral drift): {full_msg}")
        return 2 if (args.full and full_passed is None) else 0
    print(f"FAIL feedback-erasure-completeness ({nfail} probe(s) failed)")
    for label, offs, hint in failed:
        print(f"\nProbe {label} failures:")
        for o in offs:
            print(f"  {o}")
        print(f"  Remediation: {hint}")
    if full_passed is False:
        print(f"\nProbe --full (behavioral drift):\n  {full_msg}")
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
