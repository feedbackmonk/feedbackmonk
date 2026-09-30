#!/usr/bin/env python3
"""translation-egress-q24-isolation Verification Oracle (canonical implementation).

The anti-reward-hacking leg for FR-FBR-30's privacy posture (DEC-FBR-IMPL-26) and
the Q24 read-isolation invariant (DEC-FBR-IMPL-25 / DEC-FBR-02). It proves — FROM
CODE, not from a self-reported flag — five load-bearing properties of the
multilingual-translation pipeline:

  A) PROVIDER POSTURE (static, main.rs + crate src): the provider env var
     DEFAULTS to `off` at the env read; the match arm carrying `"off"` evaluates
     to `Ok(None)`; the catch-all arm (unknown value) errors; and no concrete
     provider (`DeepLTranslator` / `LibreTranslateTranslator`) is constructed
     anywhere in shipped source except inside `build_translation_provider`.

  B) READ ISOLATION (static, all crate src): every occurrence of
     `body_translated` (comments stripped) sits inside an allow-listed fn body
     or an allow-listed struct declaration. Occurrence-based: a module-level
     `const` SQL string, a new wire/row struct field, or an impl item counts.

  C) WRITER UNIQUENESS (static): the column is WRITTEN (`SET body_translated`) in
     exactly ONE repository fn (`set_translation`), no INSERT INTO feedback
     writes it, and `.set_translation(` is called only from the worker.

  D) FTS SOURCE (static, migrations): the latest `body_tsv` generated column
     sources from `coalesce(body_translated, body)`.

  E) PUBLIC-ROUTER FILES (static, main.rs build_app + api src): every router
     merged in `build_app` that is NOT wrapped in `bind_admin_routes` is public;
     the file defining it (aliases resolved through lib.rs/main.rs `use .. as`)
     must not name `body_translated`, a translation-bearing struct, or a
     repository translation reader.

  --full) BEHAVIOR: runs tests/translation_worker.rs against the real DB.

Exit 0 on PASS, 1 on FAIL, 2 on environment failure. `--root <path>` scans a
different tree (the adversarial self-test's mutated copy); see README.md.

Lineage: FR-FBR-30; DEC-FBR-IMPL-25; DEC-FBR-IMPL-26; DEC-FBR-02 / Q24.
Plan: docs/planning/plans/20260621T160022-fr-fbr-30-multilingual-translation.md (Stream E)
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path
from typing import Dict, Iterator, List, Optional, Tuple

SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_ROOT = SCRIPT_DIR.parents[2]

REPO_ROOT = DEFAULT_ROOT
CRATES_DIR = API_SRC = MAIN_RS = LIB_RS = MIGRATIONS_DIR = WORKER_TEST_RS = Path()


def _set_root(root: Path) -> None:
    """Point every probe at `root` (default: this repo)."""
    global REPO_ROOT, CRATES_DIR, API_SRC, MAIN_RS, LIB_RS, MIGRATIONS_DIR, WORKER_TEST_RS
    REPO_ROOT = root
    CRATES_DIR = root / "crates"
    API_SRC = CRATES_DIR / "feedbackmonk-api" / "src"
    MAIN_RS = API_SRC / "main.rs"
    LIB_RS = API_SRC / "lib.rs"
    MIGRATIONS_DIR = root / "migrations"
    WORKER_TEST_RS = CRATES_DIR / "feedbackmonk-api" / "tests" / "translation_worker.rs"


_set_root(DEFAULT_ROOT)

TRANSLATED_COL = "body_translated"
# Functions permitted to REFERENCE body_translated. Adding one is a conscious
# decision: extend this set and cite why.
ALLOWED_REFERENT_FNS = {
    "set_translation",                 # the worker's writer (Probe C pins uniqueness)
    "list_member_bodies_for_cluster",  # the sole analyst/clustering consumer (Stream D)
    "get_translation_for_admin",       # the sole admin (controller) repo reader, scoped (#3)
    "get_admin_feedback",              # the admin detail HANDLER plumbing the above into the
                                       # admin (controller) response — not public/end-user/board (#3)
}
# Structs permitted to DECLARE a `body_translated` field. A struct carrying the
# translation is a vehicle for it; a new one (a board/end-user wire struct, a
# row type a public handler reads) FAILS Probe B, and no public-router file may
# name one of these (Probe E).
ALLOWED_TRANSLATION_STRUCTS = {
    "AdminTranslationView",    # repository feedback.rs: get_translation_for_admin's return type
    "FeedbackDetailResponse",  # handlers/admin_feedback.rs: the admin (controller) detail wire (#3)
}
# Repository readers of the translation — forbidden by name in public-router files.
TRANSLATION_READER_FNS = {"get_translation_for_admin", "list_member_bodies_for_cluster"}
# Concrete egress providers: constructing one outside PROVIDER_FN bypasses default-off.
PROVIDER_TYPES = ("DeepLTranslator", "LibreTranslateTranslator")

WRITER_FN = "set_translation"
WRITER_CALLER_REL = "crates/feedbackmonk-api/src/translation/worker.rs"

PROVIDER_ENV = "FEEDBACKMONK_TRANSLATION_PROVIDER"
PROVIDER_FN = "build_translation_provider"


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p).replace("\\", "/")


_TOKEN_RE = re.compile(
    r'(?P<raw>\br(?P<h>#*)".*?"(?P=h))'       # raw string r"..", r#".."#
    r'|(?P<str>"(?:\.|[^"\])*")'           # string literal
    r"|(?P<chr>'(?:\.|[^'\\n])')"          # char literal (a lone ' is a lifetime)
    r'|(?P<line>//[^\n]*)'                   # line comment
    r'|(?P<block>/\*.*?\*/)',                # block comment
    re.DOTALL,
)
_STRIP_CACHE: Dict[Path, str] = {}


def _blank(m: "re.Match[str]") -> str:
    if m.group("line") is not None or m.group("block") is not None:
        return re.sub(r"[^\n]", " ", m.group(0))
    return m.group(0)


def _strip_comments(text: str) -> str:
    """Blank out `//` and `/* */` comments, LENGTH-PRESERVING (comment chars
    become spaces, newlines kept) so offsets line up with the raw text.
    String-aware: `//` inside `"http://.."` or a raw string is not a comment."""
    return _TOKEN_RE.sub(_blank, text)


def _balanced(text: str, open_idx: int, open_c: str, close_c: str) -> int:
    depth = 0
    for i in range(open_idx, len(text)):
        if text[i] == open_c:
            depth += 1
        elif text[i] == close_c:
            depth -= 1
            if depth == 0:
                return i
    return -1


def _fn_spans(code: str) -> Iterator[Tuple[str, int, int]]:
    """(name, start, end) for every fn WITH a body in comment-stripped `code`.
    Generic fns (`fn x<'t>(`) included; trait signatures (`fn x(..);`) skipped."""
    for m in re.finditer(r"\bfn\s+(\w+)\s*[(<]", code):
        brace = code.find("{", m.end())
        semi = code.find(";", m.end())
        if brace == -1 or (semi != -1 and semi < brace):
            continue
        end = _balanced(code, brace, "{", "}")
        if end != -1:
            yield m.group(1), m.start(), end + 1


def _struct_spans(code: str) -> Iterator[Tuple[str, int, int]]:
    for m in re.finditer(r"\bstruct\s+(\w+)[^;{]*\{", code):
        end = _balanced(code, m.end() - 1, "{", "}")
        if end != -1:
            yield m.group(1), m.start(), end + 1


def _innermost(spans, pos: int):
    hit = [s for s in spans if s[1] <= pos < s[2]]
    return min(hit, key=lambda s: s[2] - s[1]) if hit else None


def _line(code: str, pos: int) -> int:
    return code.count("\n", 0, pos) + 1


_SRC_CACHE: Dict[Path, List[Path]] = {}


def _crate_src_files() -> List[Path]:
    """Shipped crate SOURCE only: `crates/<crate>/src/**.rs`. Walks only `src/`
    dirs, never `target/` (which is what made a naive rglob take seconds)."""
    if CRATES_DIR in _SRC_CACHE:
        return _SRC_CACHE[CRATES_DIR]
    out: List[Path] = []
    if CRATES_DIR.is_dir():
        for crate in sorted(CRATES_DIR.iterdir()):
            src = crate / "src"
            if src.is_dir():
                out.extend(sorted(src.rglob("*.rs")))
    _SRC_CACHE[CRATES_DIR] = out
    return out


_RAW_CACHE: Dict[Path, str] = {}


def _raw(path: Path) -> str:
    if path not in _RAW_CACHE:
        _RAW_CACHE[path] = path.read_text(encoding="utf-8", errors="replace")
    return _RAW_CACHE[path]


def _read_code(path: Path, *needles: str) -> Optional[str]:
    """Comment-stripped source. With `needles`, a file whose RAW text contains
    none of them returns None without stripping (a token absent from the raw
    text cannot be present in code) — this is what keeps the run fast."""
    raw = _raw(path)
    if needles and not any(n in raw for n in needles):
        return None
    if path not in _STRIP_CACHE:
        _STRIP_CACHE[path] = _strip_comments(raw)
    return _STRIP_CACHE[path]


def probe_a() -> List[str]:
    offenders: List[str] = []
    if not MAIN_RS.exists():
        return [f"{rel(MAIN_RS)} does not exist — cannot verify provider posture"]
    code = _read_code(MAIN_RS) or ""
    span = next(((s, e) for n, s, e in _fn_spans(code) if n == PROVIDER_FN), None)
    if span is None:
        return [f"{rel(MAIN_RS)}: `fn {PROVIDER_FN}` missing — the env-gated provider "
                "constructor (default-off posture, DEC-FBR-IMPL-26) is gone"]
    body = code[span[0]:span[1]]
    # Default "off" AT the env read (not any later unwrap_or in the fn).
    if not re.search(rf'env::var\(\s*"{PROVIDER_ENV}"\s*\)\s*(?:\.ok\(\)\s*)?'
                     rf'\.unwrap_or(?:_else)?\(\s*(?:\|_\|\s*)?"off"', body):
        offenders.append(
            f"{rel(MAIN_RS)}::{PROVIDER_FN}: {PROVIDER_ENV} does not default to \"off\" — egress "
            "is opt-in (DEC-FBR-IMPL-26); any other default makes every self-hoster a data exporter.")
    # The arm whose pattern contains "off" must be `=> Ok(None)`. (The previous
    # check OR'ed in `'"off"' in body`, which the default string always met.)
    if not re.search(r'(?:"[^"]*"\s*\|\s*)*"off"\s*(?:\|\s*"[^"]*"\s*)*=>\s*Ok\(\s*None\s*\)', body):
        offenders.append(
            f"{rel(MAIN_RS)}::{PROVIDER_FN}: no `\"off\" => Ok(None)` arm — `off` MUST construct "
            "no provider (no worker, no egress).")
    # Catch-all arm must refuse to start.
    for m in re.finditer(r'(?m)^\s*(_|[a-z_]\w*)\s*=>\s*(\S+)', body):
        if not m.group(2).startswith(("Err", "anyhow::bail", "bail!", "return")):
            offenders.append(
                f"{rel(MAIN_RS)}::{PROVIDER_FN}: catch-all arm `{m.group(1)} => {m.group(2)}` does "
                "not error — an unknown provider value must refuse to start, never pick a provider.")
    # No concrete provider constructed anywhere else in shipped source.
    ctor = re.compile(rf"\b({'|'.join(PROVIDER_TYPES)})\s*::\s*new\s*\(")
    for path in _crate_src_files():
        pcode = _read_code(path, *PROVIDER_TYPES)
        if pcode is None or not ctor.search(pcode):
            continue
        # Only the balanced body of a `#[cfg(test)] mod x { .. }` is exempt —
        # shipped code after a test module is still scanned.
        test_spans = []
        for tm in re.finditer(r"#\[cfg\(test\)\]\s*(?:pub\s+)?mod\s+\w+\s*\{", pcode):
            end = _balanced(pcode, tm.end() - 1, "{", "}")
            test_spans.append((tm.start(), end if end != -1 else len(pcode)))
        scan = pcode
        for m in ctor.finditer(scan):
            if path == MAIN_RS and span[0] <= m.start() < span[1]:
                continue
            if any(a <= m.start() < b for a, b in test_spans):
                continue
            offenders.append(
                f"{rel(path)}:{_line(scan, m.start())}: constructs `{m.group(1)}::new(` outside "
                f"`{PROVIDER_FN}` — an unconditional egress path (DEC-FBR-IMPL-26).")
    return offenders


def probe_b() -> List[str]:
    offenders: List[str] = []
    for path in _crate_src_files():
        code = _read_code(path, TRANSLATED_COL)
        if code is None:
            continue
        fns = list(_fn_spans(code))
        structs = list(_struct_spans(code))
        for m in re.finditer(rf"\b{TRANSLATED_COL}\b", code):
            f = _innermost(fns, m.start())
            if f is not None:
                if f[0] in ALLOWED_REFERENT_FNS:
                    continue
                where = f"fn `{f[0]}`"
            else:
                st = _innermost(structs, m.start())
                if st is not None and st[0] in ALLOWED_TRANSLATION_STRUCTS:
                    continue
                where = f"struct `{st[0]}`" if st else "a module-level item (const/static/impl)"
            offenders.append(
                f"{rel(path)}:{_line(code, m.start())}: `{TRANSLATED_COL}` referenced in {where}, "
                "which is not allow-listed. Public/end-user/board surfaces MUST read the verbatim "
                "`body`, never the translation (Q24, DEC-FBR-IMPL-25). A deliberate new admin/"
                "machine consumer needs an allowlist entry with a rationale.")
    return offenders


def probe_c() -> List[str]:
    offenders: List[str] = []
    set_re = re.compile(rf"SET\s+{TRANSLATED_COL}\b", re.IGNORECASE)
    ins_re = re.compile(rf"INSERT\s+INTO\s+feedback\b[^;]*?\b{TRANSLATED_COL}\b",
                        re.IGNORECASE | re.DOTALL)
    call_re = re.compile(rf"\.{WRITER_FN}\s*\(")
    writers: List[str] = []
    for path in _crate_src_files():
        code = _read_code(path, WRITER_FN, TRANSLATED_COL)
        if code is None:
            continue
        if call_re.search(code) and rel(path) != WRITER_CALLER_REL:
            offenders.append(
                f"{rel(path)}: calls `.{WRITER_FN}(` outside the translate-after-accept worker "
                f"({WRITER_CALLER_REL}) — translation never happens on the request path.")
        if TRANSLATED_COL not in code:
            continue
        for name, s, e in _fn_spans(code):
            if set_re.search(code[s:e]):
                writers.append(name)
                if name != WRITER_FN:
                    offenders.append(
                        f"{rel(path)}::{name}: WRITES `{TRANSLATED_COL}` but only `{WRITER_FN}` "
                        "may (DEC-FBR-IMPL-25: the async worker is the sole writer).")
        if ins_re.search(code):
            offenders.append(
                f"{rel(path)}: an INSERT INTO feedback writes `{TRANSLATED_COL}` — a translation "
                "is only written by the worker's UPDATE, never at submit.")
    if WRITER_FN not in writers:
        offenders.append(f"no `{WRITER_FN}` writes `{TRANSLATED_COL}` — the pipeline's writer is missing.")
    return offenders


def probe_d() -> List[str]:
    if not MIGRATIONS_DIR.is_dir():
        return [f"{rel(MIGRATIONS_DIR)} missing — cannot verify the FTS source"]
    gen_re = re.compile(
        r"body_tsv\s+tsvector\s+GENERATED\s+ALWAYS\s+AS\s*\((?P<expr>.*?)\)\s*STORED",
        re.IGNORECASE | re.DOTALL)
    latest: Optional[Tuple[str, str]] = None
    for sql in sorted(MIGRATIONS_DIR.glob("*.sql")):
        for m in gen_re.finditer(sql.read_text(encoding="utf-8")):
            latest = (sql.name, m.group("expr"))
    if latest is None:
        return [f"{rel(MIGRATIONS_DIR)}: no `body_tsv ... GENERATED ALWAYS AS (...) STORED` found"]
    fname, expr = latest
    norm = re.sub(r"\s+", " ", expr).lower()
    if not ("coalesce(" in norm and TRANSLATED_COL in norm):
        return [f"migrations/{fname}: current `body_tsv` is `{expr.strip()}` — it MUST source from "
                f"`coalesce({TRANSLATED_COL}, body)` (DEC-FBR-IMPL-25)."]
    return []


def _router_defs() -> Dict[str, Path]:
    """Router fn name -> defining file, including `use x::y as z` aliases."""
    defs: Dict[str, Path] = {}
    for path in (p for p in _crate_src_files() if API_SRC in p.parents):
        # Only files defining a `*_router`/`*_routes` fn; routers named anything
        # else are reached through the `use .. as <alias>` pass below.
        if not re.search(r"\bpub\s+fn\s+\w+_(?:router|routes)\b", _raw(path)):
            continue
        code = _read_code(path) or ""
        for m in re.finditer(r"\bpub\s+fn\s+(\w+)\s*[(<]", code):
            defs.setdefault(m.group(1), path)

    def modfile(modpath: str) -> Optional[Path]:
        parts = [p for p in modpath.split("::") if p not in ("crate", "feedbackmonk_api", "self")]
        for cand in (API_SRC.joinpath(*parts).with_suffix(".rs"), API_SRC.joinpath(*parts, "mod.rs")):
            if cand.exists():
                return cand
        return None

    for src in (LIB_RS, MAIN_RS):
        if not src.exists():
            continue
        code = _read_code(src) or ""
        for m in re.finditer(r"\buse\s+([\w:]+?)::(\{[^}]*\}|\w+\s+as\s+\w+)\s*;", code):
            for item in m.group(2).strip("{} ").split(","):
                im = re.match(r"\s*\w+\s+as\s+(\w+)\s*$", item)
                if im:
                    f = modfile(m.group(1))
                    if f is not None:
                        defs[im.group(1)] = f
    return defs


def probe_e() -> List[str]:
    offenders: List[str] = []
    if not MAIN_RS.exists():
        return [f"{rel(MAIN_RS)} does not exist — cannot classify public routers"]
    code = _read_code(MAIN_RS) or ""
    span = next(((s, e) for n, s, e in _fn_spans(code) if n == "build_app"), None)
    if span is None:
        return [f"{rel(MAIN_RS)}: `fn build_app` not found — cannot classify public routers"]
    body = code[span[0]:span[1]]
    public: List[str] = []
    for m in re.finditer(r"\.merge\s*\(", body):
        close = _balanced(body, m.end() - 1, "(", ")")
        arg = body[m.end():close]
        if re.match(r"\s*bind_admin_routes\s*\(", arg):
            continue
        # Every free-function call in the argument (not `.method(` / `Path::fn(`)
        # other than the public wrappers names a router — whatever it is called.
        public += [c for c in re.findall(r"(?<![.:\w])(\w+)\s*\(", arg)
                   if c not in ("bind_public_routes", "apply_public_rate_limit", "Some", "Arc")]
    base = re.search(r"let\s+app\s*=\s*(\w+)\s*\(", body)
    if base:
        public.append(base.group(1))
    public = sorted(set(public))
    if not public:
        return [f"{rel(MAIN_RS)}::build_app: no public router parsed — Q24 isolation of public "
                "handlers is unverified (parser no longer matches build_app)"]
    defs = _router_defs()
    tokens = sorted(ALLOWED_TRANSLATION_STRUCTS | TRANSLATION_READER_FNS | {TRANSLATED_COL})
    tok_re = re.compile(rf"\b({'|'.join(tokens)})\b")
    seen = set()
    for r in public:
        f = defs.get(r)
        if f is None:
            offenders.append(f"public router `{r}` (merged in build_app) has no locatable "
                             "definition — cannot verify it never names the translation")
            continue
        if f in seen:
            continue
        seen.add(f)
        fcode = _read_code(f) or ""
        for m in tok_re.finditer(fcode):
            offenders.append(
                f"{rel(f)}:{_line(fcode, m.start())}: names `{m.group(1)}` in a file serving the "
                f"PUBLIC router `{r}` — the translation and every type/reader carrying it are "
                "admin/machine-only; public responses carry the verbatim `body` (Q24).")
    return offenders


def probe_full(full: bool) -> Tuple[Optional[bool], str]:
    if not full:
        return None, "skipped (pass --full to run tests/translation_worker.rs)"
    if not WORKER_TEST_RS.exists():
        return False, f"{rel(WORKER_TEST_RS)} is missing — the behavioural drift leg is gone"
    cmd = ["cargo", "test", "-p", "feedbackmonk-api", "--test", "translation_worker"]
    try:
        proc = subprocess.run(cmd, cwd=str(REPO_ROOT), capture_output=True, text=True, timeout=600)
    except FileNotFoundError:
        return None, "cargo not found — behavioural probe could not run"
    except subprocess.TimeoutExpired:
        return False, "translation_worker tests timed out"
    if proc.returncode == 0:
        return True, "translation_worker: all passed"
    tail = (proc.stdout + proc.stderr).strip().splitlines()[-8:]
    return False, "translation_worker tests failed:\n      " + "\n      ".join(tail)


def main() -> int:
    parser = argparse.ArgumentParser(description="translation-egress-q24-isolation oracle")
    parser.add_argument("--full", action="store_true", help="also run the translation integration tests")
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT,
                        help="repo root to scan (default: this repo; the self-test uses a mutated copy)")
    args = parser.parse_args()
    root = args.root.resolve()
    if not (root / "crates").is_dir():
        print(f"ERROR translation-egress-q24-isolation: {root} has no crates/ — not a repo root")
        return 2
    _set_root(root)

    results = [
        ("A (provider posture: default off, off=>None, catch-all errors, no stray ctor)", probe_a(),
         "keep build_translation_provider the only constructor, defaulting to \"off\" (DEC-FBR-IMPL-26)."),
        ("B (Q24 read isolation: body_translated only in allow-listed fns/structs)", probe_b(),
         "human-facing reads use the verbatim `body`; only admin/machine consumers see the translation."),
        ("C (writer uniqueness: only set_translation, only the worker calls it)", probe_c(),
         "only set_translation writes body_translated, and only the worker calls it."),
        ("D (FTS sources coalesce(body_translated, body))", probe_d(),
         "the latest body_tsv migration must use coalesce(body_translated, body)."),
        ("E (no public-router file names a translation column/type/reader)", probe_e(),
         "public handlers return the verbatim `body`; keep translation types/readers admin-only."),
    ]
    full_passed, full_msg = probe_full(args.full)
    if args.full and full_passed is None:
        print(f"ERROR translation-egress-q24-isolation: {full_msg}")
        return 2

    fails = sum(1 for _, offs, _ in results if offs) + (1 if full_passed is False else 0)
    if fails == 0:
        print("PASS translation-egress-q24-isolation")
        for label, _, _ in results:
            print(f"  Probe {label}: clean")
        print(f"  Probe --full (behavioral drift): {full_msg}")
        return 0

    print(f"FAIL translation-egress-q24-isolation ({fails} probe(s) failed)")
    for label, offs, fixhint in results:
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
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
