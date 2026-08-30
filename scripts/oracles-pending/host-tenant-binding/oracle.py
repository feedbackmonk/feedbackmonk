#!/usr/bin/env python3
"""host-tenant-binding Verification Oracle (FR-FBR-32, DEC-FBR-IMPL-28).

ASSERTION (frozen at Task Zero, before any implementation existed):

    When a public HTTP request arrives on a host that resolves to tenant T, no
    public route may return or act on a resource belonging to any tenant other
    than T; and the admin surface is reachable on exactly one host -- the
    canonical admin host -- and on no tenant subdomain, custom domain, or admin
    alias.

WHY THIS ORACLE EXISTS. DEC-FBR-13 chose tenant subdomains over path-based
multi-tenancy for one decisive reason: origin isolation for the
user-generated-content surface. That reason is bought ONLY by *binding* -- the
restriction that a request on tenant A's host cannot reach tenant B's data.
Resolution alone buys nothing, and its absence is invisible: a missing guard
renders a perfectly correct-looking page of someone else's feedback on your
origin. Nothing errors. No test that exercises the happy path can see it.

`multi-tenant-isolation-check` does NOT cover this. It polices the REPOSITORY
scope axis (does every repo method take a `&TenantScope`). Host binding is a
second, orthogonal axis introduced by FR-FBR-32, and a router merged without the
guard passes that oracle unchanged.

The recurrent failure mode is the same one `public-route-ceiling` exists for: a
public route added later that silently fails to inherit a floor. Probe A is
therefore modelled on it directly -- adding a new public route means adding it
to PUBLIC_ROUTERS here AND wrapping it in `build_app`. That is the point.

Detection-from-CODE, never a self-reported flag:
  Probe A -- guard coverage in `build_app`.
  Probe B -- admin exclusivity in the host layer's own source.
  Probe C -- resolution purity: one host->tenant entry point, in the repository
             crate (DEC-FBR-03), with no ad-hoc Host comparisons elsewhere.
  Probe D -- (--full) the behavioural fixtures.

GRACEFUL ABSENCE. Vacuous-PASS when the host layer is not present in the tree at
all. A self-host checkout that never enables `FEEDBACKMONK_ROOT_DOMAIN` must not
go red for declining a SaaS feature -- the binding is inert by design there
(DEC-FBR-IMPL-28 part 3), and inertness is itself asserted by Probe D.

Exit 0 PASS, 1 FAIL, 2 environment error.
"""
from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
# When installed at .claude/oracles/host-tenant-binding/, the repo root is 3 up.
REPO_ROOT = SCRIPT_DIR.parents[2]

API = REPO_ROOT / "crates" / "feedbackmonk-api"
MAIN_RS = API / "src" / "main.rs"
HOSTING_RS = API / "src" / "hosting.rs"
REPO_DOMAINS_RS = REPO_ROOT / "crates" / "feedbackmonk-repository" / "src" / "domains.rs"

PUBLIC_WRAPPER = "bind_public_routes"
ADMIN_WRAPPER = "bind_admin_routes"

# Public (end-user-reachable) routers that MUST carry the host binding. Every
# one of these can be reached with a `project_id` in the path, so every one of
# them can leak across tenants if the guard is dropped.
PUBLIC_ROUTERS = [
    "submission_router",
    "attachments_router",
    "board_router",
    "roadmap_router",
    "widget_config_router",
    "me_feedback_router",
    "me_feedback_data_router",
    "solicitation_router",
    "public_site_router",
]

# Admin/operator routers that must be unreachable on a tenant-bound host, so no
# admin surface -- and no admin session cookie -- ever exists on an origin a
# tenant or a customer controls.
ADMIN_ROUTERS = [
    "worker_a_router",
    "account_recovery_router",
    "admin_feedback_routes",
    "admin_roadmap_router",
    "admin_tier_router",
    "ops_router",
    "moderation_router",
    "promote_router",
    "domains_router",
    "work_order_admin_router",
    "work_order_runner_router",
    "runner_tokens_admin_router",
    "cluster_admin_router",
    "recommendation_admin_router",
    "sweep_admin_router",
]

# Deliberately unbound, each for a stated reason. Listed so a reviewer sees the
# exemptions rather than inferring them from absence.
INTENTIONALLY_UNBOUND = {
    "health_router": (
        "orchestrator probes may arrive on any hostname the deployment answers "
        "on; health carries no tenant data, so a 404-by-hostname would take a "
        "healthy instance out of rotation for no security gain"
    ),
    "capabilities_router": (
        "deployment metadata only -- no tenant data, no project id"
    ),
}

# The only files permitted to name the host->tenant resolver. Anything else
# means a second resolution path has appeared.
RESOLVE_CALLERS_ALLOWED = {
    "crates/feedbackmonk-repository/src/domains.rs",
    "crates/feedbackmonk-repository/tests/domains_repo.rs",
    "crates/feedbackmonk-api/src/hosting.rs",
    "crates/feedbackmonk-api/src/handlers/public_site.rs",
}


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p).replace("\\", "/")


def _balanced_body(text: str, open_idx: int, opener: str = "{", closer: str = "}"):
    """Return the substring from `open_idx` through its matching closer."""
    depth = 0
    for i in range(open_idx, len(text)):
        if text[i] == opener:
            depth += 1
        elif text[i] == closer:
            depth -= 1
            if depth == 0:
                return text[open_idx : i + 1]
    return None


def build_app_body(text: str):
    m = re.search(r"fn\s+build_app\s*\(", text)
    if not m:
        return None
    brace = text.find("{", m.end())
    if brace == -1:
        return None
    return _balanced_body(text, brace)


def merge_arguments(body: str):
    """Yield the full text of each `.merge( ... )` argument in `body`.

    Parenthesis-balanced rather than regex-split, because the arguments nest
    (`bind_public_routes(apply_public_rate_limit(board_router(state), prl), hs)`)
    and a naive split on `.merge(` would truncate at the first `)`.
    """
    out = []
    for m in re.finditer(r"\.merge\s*\(", body):
        arg = _balanced_body(body, m.end() - 1, "(", ")")
        if arg is not None:
            out.append(arg[1:-1].strip())
    return out


def wrapped_by(arg_text: str, wrapper: str) -> bool:
    """True if `arg_text` is (immediately) an application of `wrapper`."""
    return arg_text.startswith(wrapper + "(")


def probe_a(offenders: list[str]) -> str:
    """Guard coverage in `build_app`."""
    if not MAIN_RS.exists():
        return "SKIP (main.rs absent)"
    text = MAIN_RS.read_text(encoding="utf-8", errors="replace")
    body = build_app_body(text)
    if body is None:
        offenders.append(f"{rel(MAIN_RS)}  build_app not found -- cannot verify guard coverage")
        return "FAIL"

    args = merge_arguments(body)
    # The first router in the chain is not inside a `.merge(`; capture it too.
    head = re.search(r"let\s+app\s*=\s*(.+?)\n", body, re.S)
    if head:
        args.append(head.group(1).strip())

    def carries(router: str, wrapper: str) -> bool:
        for a in args:
            if re.search(rf"\b{re.escape(router)}\s*\(", a):
                if wrapped_by(a, wrapper):
                    return True
        return False

    def mentioned(router: str) -> bool:
        return any(re.search(rf"\b{re.escape(router)}\s*\(", a) for a in args)

    ok = True
    for router in PUBLIC_ROUTERS:
        if not mentioned(router):
            offenders.append(
                f"{rel(MAIN_RS)}  build_app no longer merges public router `{router}` "
                f"-- if it was renamed or removed, update PUBLIC_ROUTERS in this oracle"
            )
            ok = False
        elif not carries(router, PUBLIC_WRAPPER):
            offenders.append(
                f"{rel(MAIN_RS)}  public router `{router}` is merged WITHOUT "
                f"`{PUBLIC_WRAPPER}(...)` -- a request on tenant A's host could reach "
                f"another tenant's project through it (FR-FBR-32 / DEC-FBR-IMPL-28)"
            )
            ok = False

    for router in ADMIN_ROUTERS:
        if not mentioned(router):
            offenders.append(
                f"{rel(MAIN_RS)}  build_app no longer merges admin router `{router}` "
                f"-- if it was renamed or removed, update ADMIN_ROUTERS in this oracle"
            )
            ok = False
        elif not carries(router, ADMIN_WRAPPER):
            offenders.append(
                f"{rel(MAIN_RS)}  admin router `{router}` is merged WITHOUT "
                f"`{ADMIN_WRAPPER}(...)` -- the admin surface would be reachable on a "
                f"tenant subdomain or a customer's own domain (DEC-FBR-13: admin lives "
                f"on exactly one host)"
            )
            ok = False

    return "PASS" if ok else "FAIL"


def probe_b(offenders: list[str]) -> str:
    """Admin exclusivity, proven from the host layer's own source."""
    if not HOSTING_RS.exists():
        return "SKIP (hosting.rs absent)"
    text = HOSTING_RS.read_text(encoding="utf-8", errors="replace")

    m = re.search(r"async\s+fn\s+admin_host_binding\s*\(", text)
    if not m:
        offenders.append(f"{rel(HOSTING_RS)}  `admin_host_binding` not found")
        return "FAIL"
    brace = text.find("{", m.end())
    body = _balanced_body(text, brace) or ""

    ok = True
    # A tenant-bound host must be refused outright.
    if not re.search(r"HostScope::Tenant\s*\{[^}]*\}\s*=>\s*not_found\s*\(", body):
        offenders.append(
            f"{rel(HOSTING_RS)}  `admin_host_binding` does not map `HostScope::Tenant` "
            f"straight to `not_found()` -- admin must be unreachable on a tenant host "
            f"BEFORE any handler runs, so no admin session cookie is ever issued there"
        )
        ok = False
    # An admin alias must redirect, never serve (DEC-FBR-IMPL-27).
    if not re.search(r"HostScope::AdminAlias\s*=>\s*redirect_to_admin\s*\(", body):
        offenders.append(
            f"{rel(HOSTING_RS)}  `admin_host_binding` does not redirect "
            f"`HostScope::AdminAlias` to the canonical admin host (DEC-FBR-IMPL-27)"
        )
        ok = False

    # The cross-tenant refusal must be 404, not 403: a 403 is an existence
    # oracle for another tenant's project ids.
    if "StatusCode::FORBIDDEN" in text:
        offenders.append(
            f"{rel(HOSTING_RS)}  host layer returns FORBIDDEN somewhere -- a "
            f"cross-tenant probe must be indistinguishable from a missing resource "
            f"(404), matching public-board-moderation-gate's posture"
        )
        ok = False

    # X-Forwarded-Host must be gated on the declared trusted proxy, or the whole
    # binding is bypassable with one header.
    m = re.search(r"pub\s+fn\s+effective_host\s*\(", text)
    if m:
        eh_body = _balanced_body(text, text.find("{", m.end())) or ""
        if "x-forwarded-host" in eh_body and "trust_forwarded_host" not in eh_body:
            offenders.append(
                f"{rel(HOSTING_RS)}  `effective_host` reads `x-forwarded-host` without "
                f"checking `trust_forwarded_host` -- an attacker-settable header would "
                f"then choose the tenant binding"
            )
            ok = False

    return "PASS" if ok else "FAIL"


def probe_c(offenders: list[str]) -> str:
    """Resolution purity: one entry point, inside the repository crate."""
    if not REPO_DOMAINS_RS.exists():
        return "SKIP (repository domains.rs absent)"

    ok = True
    if "sqlx::query!" not in REPO_DOMAINS_RS.read_text(encoding="utf-8", errors="replace"):
        offenders.append(
            f"{rel(REPO_DOMAINS_RS)}  no query found -- host resolution must live in "
            f"the repository crate (DEC-FBR-03: sole query path)"
        )
        ok = False

    for path in REPO_ROOT.glob("crates/**/*.rs"):
        r = rel(path)
        if r in RESOLVE_CALLERS_ALLOWED:
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        if re.search(r"\bresolve_host\s*\(", text):
            offenders.append(
                f"{r}  names `resolve_host` outside the allowed set -- there must be "
                f"exactly ONE host->tenant resolution path"
            )
            ok = False
        # An ad-hoc Host comparison outside the host layer is a second,
        # unguarded resolution in disguise.
        if re.search(r"headers\(\)\s*\.\s*get\s*\(\s*(header::HOST|\"host\")", text):
            offenders.append(
                f"{r}  reads the Host header outside `hosting.rs` -- host handling "
                f"must go through `effective_host` so normalisation and the "
                f"trusted-proxy rule cannot be bypassed"
            )
            ok = False

    return "PASS" if ok else "FAIL"


def probe_d(offenders: list[str]) -> str:
    """Behavioural leg (--full)."""
    cmds = [
        ["cargo", "test", "-p", "feedbackmonk-api", "--test", "host_tenant_binding"],
        ["cargo", "test", "-p", "feedbackmonk-repository", "--test", "domains_repo"],
    ]
    for cmd in cmds:
        try:
            res = subprocess.run(cmd, cwd=REPO_ROOT, capture_output=True, text=True, timeout=900)
        except (OSError, subprocess.TimeoutExpired) as e:
            offenders.append(f"probe D could not run `{' '.join(cmd)}`: {e}")
            return "FAIL"
        if res.returncode != 0:
            tail = (res.stdout + res.stderr).strip().splitlines()[-25:]
            offenders.append(f"`{' '.join(cmd)}` FAILED:\n    " + "\n    ".join(tail))
            return "FAIL"
    return "PASS"


def main() -> int:
    full = "--full" in sys.argv

    if not HOSTING_RS.exists() and not REPO_DOMAINS_RS.exists():
        print("PASS host-tenant-binding (vacuous: host layer not present in this tree)")
        return 0

    offenders: list[str] = []
    results = {
        "A (guard coverage in build_app)": probe_a(offenders),
        "B (admin exclusivity)": probe_b(offenders),
        "C (resolution purity)": probe_c(offenders),
    }
    results["D (behavioural)"] = probe_d(offenders) if full else "SKIP (pass --full)"

    failed = [k for k, v in results.items() if v == "FAIL"]
    if failed:
        print(f"FAIL host-tenant-binding ({len(offenders)} offender(s))")
        for k, v in results.items():
            print(f"  Probe {k}: {v}")
        print()
        for o in offenders:
            print(f"  {o}")
        return 1

    print("PASS host-tenant-binding")
    for k, v in results.items():
        print(f"  Probe {k}: {v}")
    if not full:
        print("  (run with --full for the behavioural leg)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
