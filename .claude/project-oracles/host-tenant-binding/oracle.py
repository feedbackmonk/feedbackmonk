#!/usr/bin/env python3
"""host-tenant-binding Verification Oracle (FR-FBR-32, DEC-FBR-IMPL-28, P0-2, DEC-FBR-IMPL-09).

ASSERTION (frozen at Task Zero, before any implementation existed):

    When a public HTTP request arrives on a host that resolves to tenant T, no
    public route may return or act on a resource belonging to any tenant other
    than T; and the admin surface is reachable on exactly one host -- the
    canonical admin host -- and on no tenant subdomain, custom domain, or admin
    alias.

Since 2026-09-30 this is also THE single `build_app` wiring check. It absorbed
`public-route-ceiling` (every public router carries the per-IP rate floor,
P0-2) and `cors-allowlist-enforcement` Probe A (credentialed CORS wired onto
exactly the public widget-embed routers, DEC-FBR-IMPL-09). CORS *policy* (echo
origin, never `*`) is not here: `tests/cors_preflight.rs` covers it.

WHY ONE TABLE. Both absorbed oracles checked a hand-kept list of routers that
were supposed to carry a wrapper; a router missing from the list was invisible
(four public routers sat bound-but-unlimited that way until 2026-09-30). Here
EVERY router merged in `build_app` must appear in ROUTERS below with its class,
and its merge expression must have exactly that class's shape:

  public   bind_public_routes(apply_public_rate_limit(<r>(..)[.layer(cors..)], prl..), hs..)
           with `.layer(cors` REQUIRED when cors=True and FORBIDDEN when False
  admin    bind_admin_routes(<r>(..), hs..)       -- no layers, no rate floor
  unbound  <r>(..)                                -- listed with a reason

An unclassified router FAILS, a router merged twice FAILS, a classified router
no longer merged FAILS, and any chain element in the `let app = ...;` statement
other than `.merge(` FAILS (a `.route(` or global `.layer(` there would escape
classification).

Detection-from-CODE (comments stripped first, so a commented-out wrapper does
not count), never a self-reported flag:
  Probe A -- build_app wiring (classification, binding, rate floor, CORS).
  Probe B -- admin exclusivity + public cross-tenant 404 in hosting.rs.
  Probe C -- resolution purity: one host->tenant entry point, in the repository
             crate (DEC-FBR-03), no ad-hoc Host reads elsewhere.
  Probe D -- (--full) the behavioural fixtures.

A missing target file is a FAIL: the host layer exists in this codebase, so its
absence is a regression, not a self-host opt-out (self-host inertness is a
RUNTIME property -- no root domain configured -- asserted by Probe D).

Usage: oracle.py [--full] [--root <repo root>]
Exit 0 PASS, 1 FAIL, 2 environment error.
"""
from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_ROOT = SCRIPT_DIR.parents[2]

PUBLIC, ADMIN, UNBOUND = "public", "admin", "unbound"

# THE table. Every router `build_app` merges must be here, exactly once.
#   name -> (class, cors_required, reason)
# cors_required is meaningful only for PUBLIC: True = `.layer(cors..)` must be
# on the router; False = it must NOT be (it is never optional).
ROUTERS: dict[str, tuple[str, bool, str]] = {
    # --- public: host-bound + per-IP rate floor --------------------------------
    "submission_router": (PUBLIC, True, "widget POST feedback; credentialed cross-origin embed"),
    "attachments_router": (PUBLIC, True, "widget attachment upload; credentialed cross-origin embed"),
    "board_router": (PUBLIC, True, "approved-only public board read/vote from the embed"),
    "roadmap_router": (PUBLIC, False, "public roadmap read/vote"),
    "widget_config_router": (
        PUBLIC,
        False,
        "fetched with credentials:'omit' and served `*`-public (brand metadata "
        "only, main.rs build_app comment) -- a credentialed CORS layer here "
        "would be wrong, so it is FORBIDDEN",
    ),
    "me_feedback_router": (PUBLIC, False, "JWT end-user surface driven by the consumer's own client"),
    "me_feedback_data_router": (PUBLIC, False, "JWT end-user erasure/export; consumer's own client"),
    "solicitation_router": (PUBLIC, False, "JWT end-user solicitation state; consumer's own client"),
    "public_site_router": (PUBLIC, False, "host-rooted discovery + on-demand-TLS seam"),
    # --- admin: canonical admin host only ----------------------------------------
    "worker_a_router": (ADMIN, False, "auth, projects, keys, admin console API"),
    "account_recovery_router": (ADMIN, False, "reset mail must not be triggerable from a tenant origin"),
    "admin_feedback_routes": (ADMIN, False, "admin triage"),
    "admin_roadmap_router": (ADMIN, False, "admin roadmap"),
    "admin_tier_router": (ADMIN, False, "admin tier read"),
    "ops_router": (ADMIN, False, "operator bearer-token surface"),
    "moderation_router": (ADMIN, False, "moderation queue + board settings"),
    "promote_router": (ADMIN, False, "feedback -> roadmap promotion"),
    "domains_router": (ADMIN, False, "subdomain / custom-domain claim + release"),
    "tenant_settings_router": (ADMIN, False, "tenant language settings (FR-FBR-38 / C38)"),
    "work_order_admin_router": (ADMIN, False, "work-order approval state machine"),
    "work_order_runner_router": (ADMIN, False, "runner write-token surface, server-to-server"),
    "runner_tokens_admin_router": (ADMIN, False, "runner token lifecycle"),
    "cluster_admin_router": (ADMIN, False, "cluster merge/split"),
    "recommendation_admin_router": (ADMIN, False, "recommendation ingestion + read"),
    "sweep_admin_router": (ADMIN, False, "sweep trigger + digest"),
    # --- intentionally unbound, each for a stated reason -------------------------
    "health_router": (
        UNBOUND,
        False,
        "orchestrator probes may arrive on any hostname the deployment answers "
        "on; health carries no tenant data, so a 404-by-hostname would take a "
        "healthy instance out of rotation for no security gain",
    ),
    "capabilities_router": (UNBOUND, False, "deployment metadata only -- no tenant data, no project id"),
}

WRAPPERS = ("bind_public_routes", "bind_admin_routes", "apply_public_rate_limit")

# The only files permitted to CALL the host->tenant resolver.
RESOLVE_CALLERS_ALLOWED = {
    "crates/feedbackmonk-repository/src/domains.rs",
    "crates/feedbackmonk-repository/tests/domains_repo.rs",
    "crates/feedbackmonk-api/src/hosting.rs",
    "crates/feedbackmonk-api/src/handlers/public_site.rs",
}


class Ctx:
    def __init__(self, root: Path):
        self.root = root
        self.api = root / "crates" / "feedbackmonk-api"
        self.main_rs = self.api / "src" / "main.rs"
        self.hosting_rs = self.api / "src" / "hosting.rs"
        self.repo_domains_rs = root / "crates" / "feedbackmonk-repository" / "src" / "domains.rs"

    def rel(self, p: Path) -> str:
        try:
            return str(p.relative_to(self.root)).replace("\\", "/")
        except ValueError:
            return str(p).replace("\\", "/")


# ----------------------------------------------------------------------------
# Lexing helpers
# ----------------------------------------------------------------------------

_LEX = re.compile(
    r'(?P<raw>(?<![A-Za-z0-9_])r(?P<h>#+)".*?"(?P=h))'   # raw string with hashes
    r'|(?P<str>(?:(?<![A-Za-z0-9_])r)?"(?:[\\].|[^"\\])*")'   # ordinary / raw string
    r'|(?P<lc>//[^\n]*)'
    r'|(?P<bc>/\*.*?\*/)',
    re.S,
)


def strip_comments(text: str) -> str:
    """Remove `//` and `/* */` comments; keep string literals intact.

    String-aware so a `//` inside a string is not taken as a comment. Char
    literals are not special-cased (a lifetime `'a` makes that ambiguous);
    build_app and the host layer contain no `'/'`-style char literals.
    """
    def repl(m: re.Match) -> str:
        return " " if (m.group("lc") is not None or m.group("bc") is not None) else m.group(0)

    return _LEX.sub(repl, text)


def balanced(text: str, open_idx: int, opener: str = "{", closer: str = "}"):
    """Substring from `open_idx` through its matching closer, or None."""
    depth = 0
    for i in range(open_idx, len(text)):
        if text[i] == opener:
            depth += 1
        elif text[i] == closer:
            depth -= 1
            if depth == 0:
                return text[open_idx : i + 1]
    return None


def fn_body(text: str, name_re: str):
    m = re.search(name_re, text)
    if not m:
        return None
    brace = text.find("{", m.end())
    return None if brace == -1 else balanced(text, brace)


def split_top(args: str) -> list[str]:
    """Split on top-level commas (depth over (), [], {})."""
    parts, depth, cur = [], 0, []
    for ch in args:
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        if ch == "," and depth == 0:
            parts.append("".join(cur).strip())
            cur = []
        else:
            cur.append(ch)
    tail = "".join(cur).strip()
    if tail:
        parts.append(tail)
    return parts


def parse_call(expr: str):
    """Parse `name(args)` followed by zero or more `.method(args)` postfixes.

    Returns (name, [args], [(method, argtext)]) or None if `expr` is not that
    shape (anything unparseable is reported, never silently accepted).
    """
    expr = expr.strip()
    m = re.match(r"([A-Za-z_][A-Za-z0-9_:]*)\s*\(", expr)
    if not m:
        return None
    inner = balanced(expr, m.end() - 1, "(", ")")
    if inner is None:
        return None
    pos = m.end() - 1 + len(inner)
    postfix = []
    while pos < len(expr):
        rest = expr[pos:]
        if not rest.strip():
            break
        pm = re.match(r"\s*\.\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(", rest)
        if not pm:
            return None
        arg = balanced(expr, pos + pm.end() - 1, "(", ")")
        if arg is None:
            return None
        postfix.append((pm.group(1), arg[1:-1].strip()))
        pos = pos + pm.end() - 1 + len(arg)
    return m.group(1), split_top(inner[1:-1]), postfix


def let_app_chain(body: str):
    """Return (head_expr, [(method, argtext)]) for the `let app = ...;` statement."""
    m = re.search(r"\blet\s+app\s*=", body)
    if not m:
        return None
    # Find the terminating `;` at depth 0.
    depth, end = 0, None
    for i in range(m.end(), len(body)):
        ch = body[i]
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        elif ch == ";" and depth == 0:
            end = i
            break
    if end is None:
        return None
    stmt = body[m.end() : end]
    # Head = text up to the first depth-0 `.`.
    depth, cut = 0, len(stmt)
    for i, ch in enumerate(stmt):
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        elif ch == "." and depth == 0:
            cut = i
            break
    head = stmt[:cut].strip()
    chain = []
    pos = cut
    while pos < len(stmt):
        rest = stmt[pos:]
        if not rest.strip():
            break
        pm = re.match(r"\s*\.\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(", rest)
        if not pm:
            chain.append(("<unparseable>", rest.strip()[:80]))
            break
        arg = balanced(stmt, pos + pm.end() - 1, "(", ")")
        if arg is None:
            chain.append(("<unbalanced>", rest.strip()[:80]))
            break
        chain.append((pm.group(1), arg[1:-1].strip()))
        pos = pos + pm.end() - 1 + len(arg)
    return head, chain


def is_ref(arg: str, var: str) -> bool:
    return re.fullmatch(rf"&?\s*{var}(\s*\.\s*clone\s*\(\s*\))?", arg.strip()) is not None


# ----------------------------------------------------------------------------
# Probe A -- build_app wiring
# ----------------------------------------------------------------------------

def classify_merge(expr: str):
    """Unwrap the wrappers of one merged expression.

    Returns (router_name, wrapper_stack, problems). wrapper_stack lists the
    wrappers outermost-first; problems are shape violations found on the way.
    """
    problems: list[str] = []
    stack: list[str] = []
    cur = expr
    while True:
        parsed = parse_call(cur)
        if parsed is None:
            return None, stack, [f"unparseable merge expression `{cur[:100]}`"]
        name, args, postfix = parsed
        if name in WRAPPERS:
            if postfix:
                problems.append(f"`{name}(...)` carries postfix `.{postfix[0][0]}(` -- not a recognised shape")
            if len(args) != 2:
                problems.append(f"`{name}` called with {len(args)} args, expected 2")
                return None, stack, problems
            second = args[1]
            want = "prl" if name == "apply_public_rate_limit" else "hs"
            if not is_ref(second, want):
                problems.append(f"`{name}(.., {second})` -- second arg must be `{want}`")
            stack.append(name)
            cur = args[0]
            continue
        return (name, stack, problems, postfix)


def probe_a(ctx: Ctx, offenders: list[str]) -> str:
    if not ctx.main_rs.exists():
        offenders.append(f"{ctx.rel(ctx.main_rs)} does not exist -- cannot verify build_app wiring")
        return "FAIL"
    raw = ctx.main_rs.read_text(encoding="utf-8", errors="replace")
    text = strip_comments(raw)
    where = f"{ctx.rel(ctx.main_rs)}::build_app"
    body = fn_body(text, r"fn\s+build_app\s*\(")
    if body is None:
        offenders.append(f"{where}  not found -- cannot verify wiring")
        return "FAIL"
    ok = True

    # CORS + rate-floor sources (absorbed from cors-allowlist-enforcement A).
    if not re.search(r"\blet\s+cors\s*=\s*public_cors_layer\s*\(", body):
        offenders.append(f"{where}  does not build `let cors = public_cors_layer(..)` -- CORS layer is gone")
        ok = False
    if "FEEDBACKMONK_CORS_ORIGINS" not in text:
        offenders.append(f"{ctx.rel(ctx.main_rs)}  never reads FEEDBACKMONK_CORS_ORIGINS -- the allowlist source is gone")
        ok = False
    if not re.search(r"\blet\s+prl\s*=\s*PublicRateLimit\s*::\s*new\s*\(", body):
        offenders.append(f"{where}  does not build `let prl = PublicRateLimit::new(..)` -- the P0-2 rate floor is gone")
        ok = False
    if len(re.findall(r"\blet\s+app\s*=", body)) != 1 or re.search(r"\blet\s+mut\s+app\b", body):
        offenders.append(f"{where}  must bind `let app = ...;` exactly once (no rebinding / mut app) -- merges elsewhere escape classification")
        ok = False
    for bad in (r"\.\s*route\s*\(", r"\.\s*nest\s*\(", r"\.\s*nest_service\s*\(", r"\.\s*fallback\s*\(", r"\.\s*route_service\s*\("):
        if re.search(bad, body):
            offenders.append(f"{where}  defines routes inline (`{bad}`) -- routes belong in a classified router fn")
            ok = False

    parsed = let_app_chain(body)
    if parsed is None:
        offenders.append(f"{where}  `let app = ...;` not found/parseable")
        return "FAIL"
    head, chain = parsed
    exprs = [head]
    for method, arg in chain:
        if method == "merge":
            exprs.append(arg)
        else:
            offenders.append(
                f"{where}  `let app` chain carries `.{method}(` -- only `.merge(` is allowed there "
                f"(a layer or route on the whole chain escapes per-router classification)"
            )
            ok = False
    total_merges = len(re.findall(r"\.\s*merge\s*\(", body))
    if total_merges != len(exprs) - 1:
        offenders.append(
            f"{where}  has {total_merges} `.merge(` calls but only {len(exprs) - 1} are in the `let app` chain "
            f"-- a merge outside the chain escapes classification"
        )
        ok = False
    # cors must not be applied anywhere except on a router inside a merge.
    cors_uses = len(re.findall(r"\blayer\s*\(\s*cors\b", body))

    seen: dict[str, int] = {}
    cors_in_merges = 0
    for expr in exprs:
        res = classify_merge(expr)
        if res[0] is None:
            offenders.append(f"{where}  " + "; ".join(res[2]))
            ok = False
            continue
        router, stack, problems, postfix = res
        seen[router] = seen.get(router, 0) + 1
        if router not in ROUTERS:
            offenders.append(
                f"{where}  merges UNCLASSIFIED router `{router}` (wrappers: {stack or 'none'}) -- add it to "
                f"ROUTERS in this oracle as public/admin/unbound with its wrappers; an unclassified "
                f"router is exactly the silent-omission this oracle exists to catch"
            )
            ok = False
            continue
        cls, cors_req, _reason = ROUTERS[router]
        for p in problems:
            offenders.append(f"{where}  `{router}`: {p}")
            ok = False
        cors_layers = [a for m, a in postfix if m == "layer" and re.match(r"&?\s*cors\b", a)]
        other_post = [(m, a) for m, a in postfix if not (m == "layer" and re.match(r"&?\s*cors\b", a))]
        cors_in_merges += len(cors_layers)
        for m, a in other_post:
            offenders.append(f"{where}  `{router}` carries unrecognised `.{m}({a[:40]})` -- not part of any classified shape")
            ok = False

        if cls == PUBLIC:
            if stack != ["bind_public_routes", "apply_public_rate_limit"]:
                if "bind_public_routes" not in stack:
                    offenders.append(
                        f"{where}  public router `{router}` is merged WITHOUT `bind_public_routes(...)` "
                        f"-- a request on tenant A's host could reach another tenant's project (FR-FBR-32)"
                    )
                if "apply_public_rate_limit" not in stack:
                    offenders.append(
                        f"{where}  public router `{router}` is merged WITHOUT `apply_public_rate_limit(...)` "
                        f"-- the class-level per-IP rate ceiling (P0-2) is missing from a public surface"
                    )
                if "bind_admin_routes" in stack:
                    offenders.append(f"{where}  public router `{router}` is wrapped in `bind_admin_routes`")
                if set(stack) >= {"bind_public_routes", "apply_public_rate_limit"}:
                    offenders.append(
                        f"{where}  public router `{router}` wrapper order is {stack}; expected "
                        f"bind_public_routes(apply_public_rate_limit(<router>, prl), hs)"
                    )
                ok = False
            if cors_req and len(cors_layers) != 1:
                offenders.append(
                    f"{where}  public router `{router}` must carry exactly one `.layer(cors..)` "
                    f"(has {len(cors_layers)}) -- browser preflight 405-regresses for the embed (DEC-FBR-IMPL-09)"
                )
                ok = False
            if not cors_req and cors_layers:
                offenders.append(
                    f"{where}  public router `{router}` carries `.layer(cors..)` but is classified cors=NO "
                    f"-- {ROUTERS[router][2]}"
                )
                ok = False
        elif cls == ADMIN:
            if stack != ["bind_admin_routes"]:
                offenders.append(
                    f"{where}  admin router `{router}` wrappers are {stack or 'none'}; expected exactly "
                    f"bind_admin_routes(<router>, hs) -- the admin surface would be reachable on a tenant "
                    f"host (DEC-FBR-13) or carry a public wrapper"
                )
                ok = False
            if cors_layers:
                offenders.append(f"{where}  admin router `{router}` carries `.layer(cors..)` -- admin must never be CORS-exposed")
                ok = False
        else:  # UNBOUND
            if stack:
                offenders.append(f"{where}  `{router}` is classified intentionally-unbound but is wrapped in {stack} -- reclassify it")
                ok = False
            if cors_layers:
                offenders.append(f"{where}  unbound router `{router}` carries `.layer(cors..)`")
                ok = False

    if cors_uses != cors_in_merges:
        offenders.append(
            f"{where}  `.layer(cors` appears {cors_uses}x in build_app but only {cors_in_merges}x on classified "
            f"routers -- CORS applied outside the per-router shape leaks onto admin routes"
        )
        ok = False
    for router, count in seen.items():
        if count > 1:
            offenders.append(f"{where}  router `{router}` is merged {count} times -- axum panics on overlap or one copy is unguarded")
            ok = False
    for router, (cls, _c, _r) in ROUTERS.items():
        if router not in seen:
            offenders.append(
                f"{where}  classified {cls} router `{router}` is no longer merged -- if it was renamed or "
                f"removed, update ROUTERS in this oracle"
            )
            ok = False
    return "PASS" if ok else "FAIL"


# ----------------------------------------------------------------------------
# Probe B -- host layer semantics
# ----------------------------------------------------------------------------

def probe_b(ctx: Ctx, offenders: list[str]) -> str:
    if not ctx.hosting_rs.exists():
        offenders.append(f"{ctx.rel(ctx.hosting_rs)} does not exist -- the host layer is gone")
        return "FAIL"
    text = strip_comments(ctx.hosting_rs.read_text(encoding="utf-8", errors="replace"))
    r = ctx.rel(ctx.hosting_rs)
    ok = True

    # The two wrappers must install the right middleware (a swap is silent).
    for wrapper, mw in (("bind_public_routes", "public_host_binding"), ("bind_admin_routes", "admin_host_binding")):
        b = fn_body(text, rf"pub\s+fn\s+{wrapper}\s*\(")
        if b is None or not re.search(rf"from_fn_with_state\s*\(\s*\w+\s*,\s*{mw}\s*\)", b):
            offenders.append(f"{r}  `{wrapper}` does not layer `from_fn_with_state(state, {mw})`")
            ok = False

    body = fn_body(text, r"async\s+fn\s+admin_host_binding\s*\(")
    if body is None:
        offenders.append(f"{r}  `admin_host_binding` not found")
        return "FAIL"
    if not re.search(r"HostScope::Tenant\s*\{[^}]*\}\s*=>\s*not_found\s*\(", body):
        offenders.append(
            f"{r}  `admin_host_binding` does not map `HostScope::Tenant` straight to `not_found()` "
            f"-- admin must be unreachable on a tenant host BEFORE any handler runs"
        )
        ok = False
    if not re.search(r"HostScope::AdminAlias\s*=>\s*redirect_to_admin\s*\(", body):
        offenders.append(f"{r}  `admin_host_binding` does not redirect `HostScope::AdminAlias` (DEC-FBR-IMPL-27)")
        ok = False

    pub = fn_body(text, r"async\s+fn\s+public_host_binding\s*\(")
    if pub is None:
        offenders.append(f"{r}  `public_host_binding` not found")
        ok = False
    else:
        if not re.search(r"project_id_in_path\s*\(", pub) or not re.search(r"\btenant_id\s*\(\s*\)\s*==\s*tenant_id\b", pub):
            offenders.append(
                f"{r}  `public_host_binding` no longer compares the path project's tenant to the host's "
                f"tenant -- the cross-tenant refusal is gone (FR-FBR-32)"
            )
            ok = False
        if len(re.findall(r"return\s+not_found\s*\(", pub)) < 2:
            offenders.append(
                f"{r}  `public_host_binding` must 404 both a cross-tenant and an unknown project "
                f"(indistinguishable from outside)"
            )
            ok = False

    if "StatusCode::FORBIDDEN" in text:
        offenders.append(
            f"{r}  host layer returns FORBIDDEN somewhere -- a cross-tenant probe must be "
            f"indistinguishable from a missing resource (404)"
        )
        ok = False

    eh = fn_body(text, r"pub\s+fn\s+effective_host\s*\(")
    if eh is None:
        offenders.append(f"{r}  `effective_host` not found")
        ok = False
    elif "x-forwarded-host" in eh.lower() and "trust_forwarded_host" not in eh:
        offenders.append(
            f"{r}  `effective_host` reads `x-forwarded-host` without checking `trust_forwarded_host` "
            f"-- an attacker-settable header would choose the tenant binding"
        )
        ok = False
    return "PASS" if ok else "FAIL"


# ----------------------------------------------------------------------------
# Probe C -- resolution purity
# ----------------------------------------------------------------------------

def probe_c(ctx: Ctx, offenders: list[str]) -> str:
    if not ctx.repo_domains_rs.exists():
        offenders.append(f"{ctx.rel(ctx.repo_domains_rs)} does not exist -- host resolution left the repository crate")
        return "FAIL"
    ok = True
    if "sqlx::query!" not in ctx.repo_domains_rs.read_text(encoding="utf-8", errors="replace"):
        offenders.append(
            f"{ctx.rel(ctx.repo_domains_rs)}  no query found -- host resolution must live in the "
            f"repository crate (DEC-FBR-03: sole query path)"
        )
        ok = False

    for path in (ctx.root / "crates").rglob("*.rs"):
        r = ctx.rel(path)
        if "/target/" in f"/{r}":
            continue
        raw = path.read_text(encoding="utf-8", errors="replace")
        if "resolve_host" not in raw and not re.search(r"(?i)header::HOST|\"host\"", raw):
            continue  # cheap prefilter; the full check below runs on comment-stripped text
        text = strip_comments(raw)
        if r not in RESOLVE_CALLERS_ALLOWED and re.search(r"\bresolve_host\s*\(", text):
            offenders.append(f"{r}  calls `resolve_host` outside the allowed set -- there must be exactly ONE host->tenant resolution path")
            ok = False
        if r != "crates/feedbackmonk-api/src/hosting.rs" and re.search(
            r"headers\s*\(\s*\)\s*\.\s*get\s*\(\s*(header::HOST|\"host\")", text, re.I
        ):
            offenders.append(
                f"{r}  reads the Host header outside `hosting.rs` -- host handling must go through "
                f"`effective_host` so normalisation and the trusted-proxy rule cannot be bypassed"
            )
            ok = False
    return "PASS" if ok else "FAIL"


def probe_d(ctx: Ctx, offenders: list[str]) -> str:
    cmds = [
        ["cargo", "test", "-p", "feedbackmonk-api", "--test", "host_tenant_binding"],
        ["cargo", "test", "-p", "feedbackmonk-repository", "--test", "domains_repo"],
    ]
    for cmd in cmds:
        try:
            res = subprocess.run(cmd, cwd=ctx.root, capture_output=True, text=True, timeout=900)
        except (OSError, subprocess.TimeoutExpired) as e:
            offenders.append(f"probe D could not run `{' '.join(cmd)}`: {e}")
            return "FAIL"
        if res.returncode != 0:
            tail = (res.stdout + res.stderr).strip().splitlines()[-25:]
            offenders.append(f"`{' '.join(cmd)}` FAILED:\n    " + "\n    ".join(tail))
            return "FAIL"
    return "PASS"


def main() -> int:
    ap = argparse.ArgumentParser(description="host-tenant-binding oracle")
    ap.add_argument("--full", action="store_true", help="also run the behavioural cargo tests (Probe D)")
    ap.add_argument("--root", type=Path, default=DEFAULT_ROOT, help="repository root (default: this checkout)")
    args = ap.parse_args()
    root = args.root.resolve()
    if not (root / "crates").is_dir():
        print(f"ERROR host-tenant-binding: {root} has no crates/ directory")
        return 2
    ctx = Ctx(root)

    offenders: list[str] = []
    results = {
        "A (build_app wiring: classify + bind + rate floor + CORS)": probe_a(ctx, offenders),
        "B (host layer: admin exclusivity + cross-tenant 404)": probe_b(ctx, offenders),
        "C (resolution purity)": probe_c(ctx, offenders),
    }
    results["D (behavioural)"] = probe_d(ctx, offenders) if args.full else "SKIP (pass --full)"

    failed = [k for k, v in results.items() if v == "FAIL"]
    head = f"FAIL host-tenant-binding ({len(offenders)} offender(s))" if failed else "PASS host-tenant-binding"
    print(head)
    for k, v in results.items():
        print(f"  Probe {k}: {v}")
    if failed:
        print()
        for o in offenders:
            print(f"  {o}")
        return 1
    counts = {c: sum(1 for v in ROUTERS.values() if v[0] == c) for c in (PUBLIC, ADMIN, UNBOUND)}
    print(f"  {counts[PUBLIC]} public / {counts[ADMIN]} admin / {counts[UNBOUND]} unbound routers classified")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
