#!/usr/bin/env python3
"""public-board-moderation-gate Verification Oracle (canonical implementation).

THE trust boundary between public submission and public EXPOSURE (FR-FBR-25a
sibling, applied to the public feedback board). It proves FROM CODE that no
public-board endpoint can return or act on a feedback row whose
`moderation_status != approved`, and that the board wire shape leaks no
submitter PII.

Probes:

  B) BOARD READ PATH (static, default run). BOARD READ SCOPE = the board.rs
     handler + every repository fn named `*board*` that queries `FROM feedback`.
       (0)  board.rs and at least one board read fn must exist — a missing target
            is a FAIL (the code exists; there is no PENDING state any more).
       (0b) board.rs may reach the feedback repository ONLY through the explicit
            BOARD_SAFE_READS allowlist below, and every allowlisted method must
            itself be a discovered board read fn (so check (1) pins its SQL).
            Any other `.feedback.<method>` call, a bare `.feedback` handle, or
            raw pool/sqlx access in board.rs FAILS.
            Board reads and writes through `.board_votes.signals()` /
            `.notes()` / `.sessions()` are seen too: each called method is
            found in the impl that backs the handle (the trait is read from
            `fn signals(&self) -> &dyn <Trait>` in the repository) and its
            body, with every same-file helper it calls, is VERIFIED gated
            (see `_verify_handle_method`). No method name is allowlisted.
       (1) EVERY SQL string literal in EACH board read fn must hard-filter
            `moderation_status = 'approved'` (a literal, not a bound param), once
            per `FROM/JOIN feedback` it contains. Rust comments and SQL `--`
            comments are stripped first, so a filter present only in a comment
            does not count.
       (1b) no non-approved moderation literal (`'pending'`/`'rejected'`) in scope.
       (2)  no submitter-PII field in scope (handler wire shape OR repo SELECT).
       (3)  no `feedback_replies` in scope.
       (4)  vote path: every board.rs handler using `.board_votes` runs
            `ensure_board_enabled` before its first use, and every DIRECT
            `.board_votes.<method>(` write is preceded by
            `resolve_approved_board_*`. A use through a sub-handle is gated by
            the repository method itself, verified in (0b). Any other
            `.board_votes` form (an aliased handle) FAILS.

  C) BEHAVIOR (--full): cargo-tests board_moderation_gate, board_privacy_isolation
     and board_vote_moderation_gate against a real DB (drift detection).

The former Probe A (moderation.rs `is_publicly_visible` classifies only Approved)
was retired: it duplicated the core unit test
`feedbackmonk-core/src/moderation.rs::only_approved_is_publicly_visible`, which
`cargo test` already runs on every push.

Exit 0 PASS, 1 FAIL, 2 environment failure.

Lineage: FR-FBR-25a sibling; DEC-FBR-02 / Q24; Contract C28 inv. 1 + C29 inv. 1 & 3;
plan docs/planning/plans/20260619T001105-public-feedback-board-moderation-gate.md.
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path
from typing import List, Optional, Tuple

SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_ROOT = SCRIPT_DIR.parents[2]

# Set in main() from --root.
REPO_ROOT = DEFAULT_ROOT
BOARD_HANDLER_RS = Path()
REPOSITORY_SRC = Path()
BOARD_TESTS: List[Path] = []


def _set_root(root: Path) -> None:
    global REPO_ROOT, BOARD_HANDLER_RS, REPOSITORY_SRC, BOARD_TESTS
    REPO_ROOT = root
    BOARD_HANDLER_RS = root / "crates" / "feedbackmonk-api" / "src" / "handlers" / "board.rs"
    REPOSITORY_SRC = root / "crates" / "feedbackmonk-repository" / "src"
    tests = root / "crates" / "feedbackmonk-api" / "tests"
    BOARD_TESTS = [
        tests / "board_moderation_gate.rs",
        tests / "board_privacy_isolation.rs",
        tests / "board_vote_moderation_gate.rs",
    ]


# The ONLY feedback-repository methods board.rs may call. Each is a board read fn
# whose every SQL literal check (1) pins to `moderation_status = 'approved'`; the
# oracle additionally FAILS if an entry here is not a discovered board read fn, so
# this list cannot launder an unfiltered read. Adding a method here requires it to
# be (a) named *board*, (b) in the repository layer, (c) approved-filtered in SQL.
BOARD_SAFE_READS = {
    "list_public_board": "board list (C29 inv. 1)",
    "get_public_board_item": "board single item (C29 inv. 1)",
    "resolve_approved_board_feedback_id": "vote-path gate resolution (plan D2)",
}

# A board read MUST hard-filter the literal 'approved'. Tolerant of alias prefix,
# whitespace, quotes, ::cast, and IN ('approved'). A bound param does NOT match.
APPROVED_SQL_RE = re.compile(
    r"moderation_status\s*(?:::\s*\w+)?\s*(?:=|\bIN\b\s*\()\s*['\"]approved['\"]",
    re.IGNORECASE,
)
NON_APPROVED_LITERAL_RE = re.compile(r"['\"](?:pending|rejected)['\"]", re.IGNORECASE)
PII_FIELDS = [
    "end_user_email",
    "end_user_name",
    "end_user_sub",
    "anon_token_hash",
    "external_metadata",
    "crash_event_id",
    # FR-FBR-37: PII-adjacent (narrows a population); admin-read only (C37).
    "submitter_locale",
]
REPLY_TABLE_TOKEN = "feedback_replies"
BOARD_VOTE_REPO_TOKEN = ".board_votes"
BOARD_ENABLED_FN = "ensure_board_enabled"
APPROVED_RESOLVE_RE = re.compile(r"resolve_approved_board\w*")
BOARD_READS_FEEDBACK_RE = re.compile(r"FROM\s+feedback\b", re.IGNORECASE)
FEEDBACK_TABLE_REF_RE = re.compile(r"\b(?:FROM|JOIN)\s+feedback\b(?!_)", re.IGNORECASE)
SQL_LITERAL_RE = re.compile(r"\b(?:SELECT|INSERT|UPDATE|DELETE|WITH)\b", re.IGNORECASE)
# Member access to the feedback repository handle in board.rs (not `.feedback_x`).
FEEDBACK_HANDLE_RE = re.compile(r"\.\s*feedback\b(?!_)")
FEEDBACK_CALL_RE = re.compile(r"\.\s*feedback\s*\.\s*(\w+)\s*\(")
# Board sub-handles: `.board_votes.signals().<m>(` (gated inside the repository,
# FR-FBR-61 / C43) vs a direct `.board_votes.<m>(` vote write (gated in the handler).
BOARD_HANDLE_CALL_RE = re.compile(
    r"\.\s*board_votes\s*\.\s*(signals|notes|sessions)\s*\(\s*\)\s*\.\s*(\w+)\s*\("
)
BOARD_DIRECT_CALL_RE = re.compile(r"\.\s*board_votes\s*\.\s*(\w+)\s*\(")
BOARD_SUB_HANDLES = {"signals", "notes", "sessions"}
HANDLE_TRAIT_RE = re.compile(
    r"\bfn\s+(signals|notes|sessions)\s*\(\s*&\s*self\s*\)\s*->\s*&\s*dyn\s+(\w+)"
)
POOL_GATE_FN = "resolve_pool_board_items"
SQL_WRITE_RE = re.compile(r"\b(?:INSERT|UPDATE|DELETE)\b", re.IGNORECASE)
LOCAL_CALL_RE = re.compile(r"(?<![.\w])(?:Self\s*::\s*)?(\w+)\s*\(")
RAW_DB_IN_HANDLER_RE = re.compile(r"\bsqlx\b|\.\s*pool\b|\bPgPool\b|\bquery(?:_as|_scalar)?!?\s*\(")


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p)


_LEX_TOKEN_RE = re.compile(
    r"//|/\*|(?<![\w])b?r(#*)\"|(?<![\w])b?\"|'(?:\\.[^'\n]{0,8}|[^\\'\n])'"
)


def lex_rust(text: str) -> Tuple[str, List[str]]:
    """Return (code_without_comments, string_literals).

    Comments (line, nested block) are replaced by spaces of equal length (so
    offsets into the result are offsets into `text`); string literals stay in the
    code AND are returned separately (their contents). Handles raw strings
    r#"..."#, byte strings, escapes, and char literals vs lifetimes."""
    out: List[str] = []
    lits: List[str] = []
    i, n = 0, len(text)
    while i < n:
        m = _LEX_TOKEN_RE.search(text, i)
        if not m:
            out.append(text[i:])
            break
        out.append(text[i:m.start()])
        tok = m.group(0)
        if tok == "//":
            j = text.find("\n", m.start())
            j = n if j == -1 else j
            out.append(" " * (j - m.start()))
            i = j
        elif tok == "/*":
            depth, j = 1, m.end()
            while j < n and depth:
                if text.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif text.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            out.append("".join(ch if ch == "\n" else " " for ch in text[m.start():j]))
            i = j
        elif tok.endswith('"') and m.group(1) is not None:  # raw string
            close = '"' + m.group(1)
            j = text.find(close, m.end())
            j = n if j == -1 else j
            lits.append(text[m.end():j])
            end = min(n, j + len(close))
            out.append(text[m.start():end])
            i = end
        elif tok.endswith('"'):  # ordinary / byte string with escapes
            j = m.end()
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            lits.append(text[m.end():j])
            out.append(text[m.start():j + 1])
            i = j + 1
        else:  # char literal
            out.append(tok)
            i = m.end()
    return "".join(out), lits


def strip_sql_comments(sql: str) -> str:
    sql = re.sub(r"--[^\n]*", " ", sql)
    return re.sub(r"/\*.*?\*/", " ", sql, flags=re.DOTALL)


def _iter_fn_bodies(text: str, name_re: str):
    """Yield (fn_name, body, body_offset) for every fn whose name matches `name_re` and has a
    body; skips signature-only trait declarations. Call on comment-stripped text."""
    for m in re.finditer(rf"\bfn\s+({name_re})\s*[(<]", text, re.IGNORECASE):
        brace = text.find("{", m.end())
        semi = text.find(";", m.end())
        if brace == -1 or (semi != -1 and semi < brace):
            continue
        depth = 0
        for i in range(brace, len(text)):
            ch = text[i]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    yield m.group(1), text[brace:i + 1], brace
                    break


def _board_read_fns() -> List[Tuple[str, str, str, str]]:
    """(relpath, fn_name, raw_body, code_body) for every repository fn named
    *board* whose (comment-stripped) body queries `FROM feedback`."""
    out = []
    for src in sorted(REPOSITORY_SRC.rglob("*.rs")):
        raw = src.read_text(encoding="utf-8")
        if not re.search(r"\bfn\s+\w*board", raw, re.IGNORECASE):
            continue  # no *board* fn can be discovered here; skip the lexer
        code, _ = lex_rust(raw)
        # lex_rust preserves offsets (comments -> spaces), so the same slice of
        # the raw text is the raw body.
        for name, body, start in _iter_fn_bodies(code, r"\w*board\w*"):
            if BOARD_READS_FEEDBACK_RE.search(body):
                out.append((rel(src), name, raw[start:start + len(body)], body))
    return out


def _blank(code: str, start: int, end: int) -> str:
    return code[:start] + "".join(c if c == "\n" else " " for c in code[start:end]) + code[end:]


def _block_end(code: str, open_brace: int) -> int:
    depth = 0
    for j in range(open_brace, len(code)):
        if code[j] == "{":
            depth += 1
        elif code[j] == "}":
            depth -= 1
            if depth == 0:
                return j
    return len(code) - 1


def _strip_test_mods(code: str) -> str:
    """Blank out `#[cfg(test)] mod .. { }` blocks (offsets kept) so a test fn
    never stands in for a production one."""
    for m in list(re.finditer(r"#\[cfg\(test\)\]\s*mod\s+\w+\s*\{", code)):
        code = _blank(code, m.start(), _block_end(code, m.end() - 1) + 1)
    return code


def _handle_impls() -> dict:
    """{handle: (relpath, file_code, impl_block)}: the production impl backing
    `.board_votes.<handle>()`, found through the trait the accessor returns
    (`fn signals(&self) -> &dyn BoardSignalRepo`), never by a hard-coded file."""
    traits, files = {}, []
    for src in sorted(REPOSITORY_SRC.rglob("*.rs")):
        code = _strip_test_mods(lex_rust(src.read_text(encoding="utf-8"))[0])
        files.append((src, code))
        for m in HANDLE_TRAIT_RE.finditer(code):
            traits[m.group(1)] = m.group(2)
    out = {}
    for handle, trait in traits.items():
        for src, code in files:
            m = re.search(rf"\bimpl\s+{re.escape(trait)}\s+for\s+\w+\s*\{{", code)
            if m:
                out[handle] = (rel(src), code, code[m.end() - 1:_block_end(code, m.end() - 1) + 1])
                break
    return out


def _sql_lits(code_body: str) -> List[str]:
    _, lits = lex_rust(code_body)
    return [strip_sql_comments(x) for x in lits if SQL_LITERAL_RE.search(x)]


def _unfiltered_feedback_read(sql: str) -> bool:
    refs = len(FEEDBACK_TABLE_REF_RE.findall(sql))
    return refs > 0 and len(APPROVED_SQL_RE.findall(sql)) < refs


def _is_ungated_write(sql: str) -> bool:
    return bool(SQL_WRITE_RE.search(sql)) and not APPROVED_SQL_RE.search(sql)


def _verify_handle_method(handle: str, method: str, impls: dict, discarded: bool,
                          read_names: set) -> Tuple[Optional[str], List[str]]:
    """Return (problem or None, bodies verified). A method reached through a board
    sub-handle is GATED iff, over its body and every same-file helper it calls
    (transitively):
      (g1) no SQL literal reads `FROM/JOIN feedback` without the literal
           `moderation_status = 'approved'` once per reference; and
      (g2) every write (an own INSERT/UPDATE/DELETE literal that does not itself
           carry the approved literal, or a call to a helper that makes one)
           comes AFTER a call to `resolve_pool_board_items`, which must itself be
           a discovered board read fn, so check (1) pins its approved-only SQL.
    The single exception is a WITHDRAW: every literal is a `DELETE` with no
    `RETURNING`, no helper is called, and the handler discards the result
    (`state.board_votes.<h>().<m>(..).await?;` as a bare statement), so the
    response cannot depend on whether a hidden row exists."""
    if handle not in impls:
        return (f"no production impl backs `.board_votes.{handle}()` under "
                f"{rel(REPOSITORY_SRC)}, so the method's gate cannot be verified"), []
    relpath, file_code, impl_block = impls[handle]
    bodies = [b for _, b, _ in _iter_fn_bodies(impl_block, re.escape(method) + r"(?!\w)")]
    if not bodies:
        return f"`{method}` has no body in the impl backing `.board_votes.{handle}()` ({relpath})", []
    body = bodies[0]
    file_fns = {n: b for n, b, _ in _iter_fn_bodies(file_code, r"\w+")}
    label = f"{relpath}::{method}"

    def helpers(b: str) -> List[Tuple[str, int]]:
        return [(m.group(1), m.start()) for m in LOCAL_CALL_RE.finditer(b)
                if m.group(1) in file_fns and m.group(1) != method]

    lits = _sql_lits(body)
    if (discarded and lits and not helpers(body)
            and all(re.match(r"\s*DELETE\b", x, re.IGNORECASE) for x in lits)
            and not any(re.search(r"\bRETURNING\b", x, re.IGNORECASE) for x in lits)):
        return None, [body]

    # (g1) over the body and the transitive closure of same-file helpers.
    seen, stack, verified = {method}, [body], []
    while stack:
        b = stack.pop()
        verified.append(b)
        if any(_unfiltered_feedback_read(x) for x in _sql_lits(b)):
            return (f"{label}: reaches a SQL literal that reads `feedback` without the literal "
                    "`moderation_status = 'approved'` (FR-FBR-61, C29 inv. 1)"), verified
        for h, _ in helpers(b):
            if h not in seen:
                seen.add(h)
                stack.append(file_fns[h])

    def writes(b: str, path: frozenset) -> bool:
        if any(_is_ungated_write(x) for x in _sql_lits(b)):
            return True
        return any(writes(file_fns[h], path | {h}) for h, _ in helpers(b) if h not in path)

    # (g2) every write site in the body comes after the pool gate.
    write_sites = []
    _, raw_lits = lex_rust(body)
    for x in raw_lits:
        if SQL_LITERAL_RE.search(x) and _is_ungated_write(strip_sql_comments(x)):
            write_sites.append(body.find(x))
    for h, pos in helpers(body):
        if h != POOL_GATE_FN and writes(file_fns[h], frozenset({h})):
            write_sites.append(pos)
    if write_sites:
        gate = re.search(rf"(?<![.\w]){POOL_GATE_FN}\s*\(", body)
        if gate is None or gate.start() > min(write_sites):
            return (f"{label}: writes before (or without) calling `{POOL_GATE_FN}`, an ungated "
                    "signal write on a possibly hidden item (FR-FBR-61, C43)"), verified
        if POOL_GATE_FN not in read_names:
            return (f"`{POOL_GATE_FN}` is not a discovered board read fn, so check (1) does not "
                    "pin its approved-only SQL and the write gate is unproven"), verified
    return None, verified


def _discarded_statement(fn_body: str, pos: int) -> bool:
    """True when the `.board_votes` use at `pos` is a bare `state...await?;`
    statement whose value is thrown away."""
    start = max(fn_body.rfind(c, 0, pos) for c in ";{}")
    if fn_body[start + 1:pos].strip() != "state":
        return False
    end = fn_body.find(";", pos)
    return end != -1 and re.search(r"\.\s*await\s*\?\s*$", fn_body[pos:end]) is not None


def probe_b() -> List[str]:
    if not BOARD_HANDLER_RS.exists():
        return [f"{rel(BOARD_HANDLER_RS)} does not exist — the public board handler is gone "
                "or moved; the moderation gate cannot be verified (FAIL, not PENDING)"]
    if not REPOSITORY_SRC.is_dir():
        return [f"{rel(REPOSITORY_SRC)} does not exist — repository layer missing"]

    offenders: List[str] = []
    handler_raw = BOARD_HANDLER_RS.read_text(encoding="utf-8")
    handler_code, _ = lex_rust(handler_raw)
    read_fns = _board_read_fns()
    read_names = {name for _, name, _, _ in read_fns}
    hrel = rel(BOARD_HANDLER_RS)

    scope: List[Tuple[str, str]] = [(hrel, handler_raw)]
    scope += [(f"{p}::{name}", raw) for p, name, raw, _ in read_fns]

    # (0) board read fns must be discoverable.
    if not read_fns:
        offenders.append(
            f"no board READ fn found under {rel(REPOSITORY_SRC)} (a fn named *board* that "
            "queries `FROM feedback`) — the approved-only filter (C29 inv. 1) cannot be verified"
        )

    # (0b) board.rs reaches feedback ONLY through BOARD_SAFE_READS.
    calls = [(m.group(1), m.start()) for m in FEEDBACK_CALL_RE.finditer(handler_code)]
    call_spans = {m.start() for m in FEEDBACK_CALL_RE.finditer(handler_code)}
    for method, pos in calls:
        line = handler_code.count("\n", 0, pos) + 1
        if method not in BOARD_SAFE_READS:
            offenders.append(
                f"{hrel}:{line}: calls `.feedback.{method}(..)` — not in the board-safe read "
                f"allowlist ({', '.join(sorted(BOARD_SAFE_READS))}). An unlisted feedback read "
                "can return pending/rejected rows or PII to the public board (C29 inv. 1/3)."
            )
    for m in FEEDBACK_HANDLE_RE.finditer(handler_code):
        if m.start() not in call_spans:
            line = handler_code.count("\n", 0, m.start()) + 1
            offenders.append(
                f"{hrel}:{line}: takes the feedback repository handle without an immediate "
                "allowlisted method call — an aliased handle hides which read runs (C29 inv. 1)."
            )
    for m in RAW_DB_IN_HANDLER_RE.finditer(handler_code):
        line = handler_code.count("\n", 0, m.start()) + 1
        offenders.append(
            f"{hrel}:{line}: raw database access `{m.group(0).strip()}` in the board handler — "
            "board reads must go through the approved-only repository fns (DEC-FBR-03, C29)."
        )
    for method in sorted(BOARD_SAFE_READS):
        if method not in read_names:
            offenders.append(
                f"allowlisted board read `{method}` is not a discovered board read fn in "
                f"{rel(REPOSITORY_SRC)} (named *board*, queries `FROM feedback`) — its "
                "approved-only SQL cannot be verified, so the allowlist entry is unproven."
            )
    # (0b, continued) board reads/writes through the board sub-handles: each
    # called method's gate is verified in the repository, never allowlisted.
    impls = _handle_impls()
    handle_calls = 0
    for fname, fbody, fstart in _iter_fn_bodies(handler_code, r"\w+"):
        for m in BOARD_HANDLE_CALL_RE.finditer(fbody):
            handle_calls += 1
            line = handler_code.count("\n", 0, fstart + m.start()) + 1
            call = f".board_votes.{m.group(1)}().{m.group(2)}"
            problem, verified = _verify_handle_method(
                m.group(1), m.group(2), impls, _discarded_statement(fbody, m.start()), read_names)
            if problem:
                offenders.append(f"{hrel}:{line} ({fname}): `{call}(..)` is not verifiably "
                                 f"approved-gated: {problem}")
            else:
                scope += [(f"{hrel}:{line} via {call} [{k}]", vb) for k, vb in enumerate(verified)]
    if not calls and not handle_calls:
        offenders.append(f"{hrel}: invokes no board-safe feedback read — the board is wired "
                         "to some other read path (C29 inv. 1).")

    # (1) EVERY SQL literal in EACH board read fn carries the approved filter,
    #     once per `FROM/JOIN feedback` reference; comments do not count.
    for relpath, name, _raw, code_body in read_fns:
        _, lits = lex_rust(code_body)
        sql_lits = [strip_sql_comments(s) for s in lits if SQL_LITERAL_RE.search(s)]
        if not sql_lits:
            offenders.append(f"{relpath}::{name}: board read fn has no SQL string literal "
                             "to verify (SQL built elsewhere cannot be proven approved-only)")
        for k, sql in enumerate(sql_lits, 1):
            refs = len(FEEDBACK_TABLE_REF_RE.findall(sql))
            filters = len(APPROVED_SQL_RE.findall(sql))
            if filters == 0 or filters < refs:
                offenders.append(
                    f"{relpath}::{name}: SQL literal #{k} of {len(sql_lits)} reads `feedback` "
                    f"{refs}x but hard-filters `moderation_status = 'approved'` {filters}x "
                    "(comments excluded). EVERY board query must carry the literal filter "
                    "(C29 inv. 1) — not a bound param, not a comment, not a sibling query."
                )

    # (1b) no non-approved moderation literal in scope.
    for label, body in scope:
        mm = NON_APPROVED_LITERAL_RE.search(body)
        if mm:
            offenders.append(f"{label}: references non-approved moderation literal "
                             f"`{mm.group(0)}` — board reads filter EXACTLY = 'approved'.")

    # (2) no submitter PII in scope.
    for label, body in scope:
        for field in PII_FIELDS:
            if re.search(rf"\b{re.escape(field)}\b", body):
                offenders.append(f"{label}: references submitter-PII field `{field}` "
                                 "(C29 inv. 3, Q24 class).")

    # (3) no internal reply content.
    for label, body in scope:
        if REPLY_TABLE_TOKEN in body:
            offenders.append(f"{label}: references `{REPLY_TABLE_TOKEN}` — the board must not "
                             "surface internal/admin reply content.")

    # (4) vote path: board gate before any use; approved resolution before every
    #     DIRECT `.board_votes.<m>(` write. Sub-handle uses are verified in (0b).
    for name, body, _ in _iter_fn_bodies(handler_code, r"\w+"):
        uses = [m.start() for m in re.finditer(re.escape(BOARD_VOTE_REPO_TOKEN) + r"\b", body)]
        if not uses:
            continue
        en = body.find(BOARD_ENABLED_FN)
        if en == -1 or en > uses[0]:
            offenders.append(f"{hrel}::{name}: handler does not call `{BOARD_ENABLED_FN}` "
                             "before its first `.board_votes` use (C29 inv. 2, plan D2).")
        handle_uses = {m.start() for m in BOARD_HANDLE_CALL_RE.finditer(body)}
        direct = {m.start(): m.group(1) for m in BOARD_DIRECT_CALL_RE.finditer(body)
                  if m.group(1) not in BOARD_SUB_HANDLES}
        for pos in uses:
            if pos in handle_uses:
                continue
            if pos not in direct:
                offenders.append(f"{hrel}::{name}: takes `.board_votes` in a form that is neither "
                                 "a direct vote write nor a verified sub-handle call; an aliased "
                                 "handle hides which write runs (plan D2).")
                continue
            rm = APPROVED_RESOLVE_RE.search(body)
            if not rm or rm.start() > pos:
                offenders.append(f"{hrel}::{name}: vote handler does not resolve the target "
                                 f"through `resolve_approved_board_*` before the "
                                 f"`.board_votes.{direct[pos]}` write, an existence oracle for "
                                 "hidden feedback (plan D2).")
    return offenders


def probe_c(full: bool) -> Tuple[Optional[bool], str]:
    if not full:
        return None, "skipped (pass --full to run the three board_* integration tests)"
    missing = [rel(t) for t in BOARD_TESTS if not t.exists()]
    if missing:
        return False, "behavioral test file(s) missing: " + ", ".join(missing)
    cmd = ["cargo", "test", "-p", "feedbackmonk-api"]
    for t in BOARD_TESTS:
        cmd += ["--test", t.stem]
    try:
        proc = subprocess.run(cmd, cwd=str(REPO_ROOT), capture_output=True, text=True, timeout=900)
    except FileNotFoundError:
        return None, "cargo not found — Probe C inconclusive"
    except subprocess.TimeoutExpired:
        return False, "board tests timed out"
    if proc.returncode == 0:
        return True, " + ".join(t.stem for t in BOARD_TESTS) + ": all passed"
    tail = (proc.stdout + proc.stderr).strip().splitlines()[-8:]
    return False, "board tests failed:\n      " + "\n      ".join(tail)


def main() -> int:
    parser = argparse.ArgumentParser(description="public-board-moderation-gate oracle")
    parser.add_argument("--full", action="store_true",
                        help="also run the board gate/privacy/vote integration tests (Probe C)")
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT,
                        help="repository root to scan (default: this repo; used by the self-test)")
    args = parser.parse_args()
    if not args.root.is_dir():
        print(f"ERROR public-board-moderation-gate: --root {args.root} is not a directory")
        return 2
    _set_root(args.root.resolve())

    b = probe_b()
    c_passed, c_message = probe_c(args.full)
    if c_passed is None and args.full and "inconclusive" in c_message:
        print(f"ERROR public-board-moderation-gate: {c_message}")
        return 2

    fails = (1 if b else 0) + (1 if c_passed is False else 0)
    if fails == 0:
        print("PASS public-board-moderation-gate")
        print(f"  Probe B (board read + vote path approved-only, board-safe reads only, no PII): "
              f"clean ({rel(BOARD_HANDLER_RS)})")
        print(f"  Probe C (behavioral drift-detection): {c_message}")
        return 0

    print(f"FAIL public-board-moderation-gate ({fails} probe(s) failed)")
    if b:
        print("\nProbe B failures (board read path):")
        for o in b:
            print(f"  {o}")
    if c_passed is False:
        print("\nProbe C failure (behavioral drift):")
        print(f"  {c_message}")
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
