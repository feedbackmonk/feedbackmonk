#!/usr/bin/env python3
"""selfhost-compose-smoke Verification Oracle (canonical implementation).

Three probes per DEC-FBR-IMPL-06:

  A) yaml-lint: `docker compose config --quiet` on `deploy/docker/docker-compose.yml`
     when the `docker` CLI is available, falling back to a pure-Python `yaml.safe_load`
     (WARN, not FAIL) when not. Catches malformed YAML, undefined service refs,
     and bad interpolation syntax.

  B) env-doc-xref: extracts every `${FEEDBACKMONK_*}` / `${DATABASE_URL}` /
     `${POSTGRES_*}` reference from compose `environment:` and ${...} interpolations,
     then set-compares against the canonical env-var catalog parsed from
     `docs/operations/SELFHOST_ENV.md` (Contract C21). Drift in either direction:
       - compose ref not in C21 → FAIL  (the oracle's load-bearing invariant —
         if an operator-facing env var is referenced but undocumented, the
         self-host story is broken at the contract boundary).
       - C21 entry not in compose → WARN (operator may legitimately omit
         optional vars; we surface but don't fail).

  C) full-smoke (gated behind `--full`): cold-state `down -v` → `up -d` → poll
     `/health/ready` for up to 90s → assert HTTP 200 with `{"status":"ok",
     "db_connected": true, ...}`. Tear down `down -v` in `finally` block so
     failure paths don't leave dangling containers/volumes. Off by default
     to keep inner-loop cost <500ms (Probe A + B only).

Cold-start vacuous-PASS plan:
  - Pre-compose: Probe A FAILs ("file does not exist"); Probe B vacuous-PASS
    (no compose to scan, no refs to check). This is the expected cold-start
    state when the oracle is built BEFORE the compose stack.
  - Post-compose: Probes A + B GREEN; Probe C optional.

Output: machine-parseable PASS / FAIL. Exit 0 on PASS, 1 on FAIL, 2 on
environment failure.

Lineage:
- FR-FBR-17 (self-host `docker compose up` distribution)
- DEC-FBR-IMPL-06 (three-probe smoke oracle decision)
- Contract C21 (env-var catalog SSOT — `docs/operations/SELFHOST_ENV.md`)
- Contract C24 (three-probe schema)
- P4 plan § Oracle Pre-Build Plan + § Testability Gate (FR-FBR-17 composite
  ~14 → high-leverage scaffolding)
- Three-leg defense:
    leg 1 = type system / env-reader chokepoints in `crates/feedbackmonk-api/src/main.rs`
    leg 2 = THIS oracle (yaml-lint + env-doc-xref + clean-state smoke)
    leg 3 = operator runbook `docs/operations/SELFHOST.md` (human-readable cold-start path)
"""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Iterable, List, Optional, Set, Tuple


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parents[2]
COMPOSE_FILE = REPO_ROOT / "deploy" / "docker" / "docker-compose.yml"
SELFHOST_ENV_DOC = REPO_ROOT / "docs" / "operations" / "SELFHOST_ENV.md"

# Probe C tunables.
HEALTH_POLL_INTERVAL_S = 2.0
HEALTH_POLL_TIMEOUT_S = 90.0
PROBE_C_PORT_DEFAULT = 14304  # FEEDBACKMONK_PORT default per C21.


def rel(p: Path) -> str:
    try:
        return str(p.relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(p).replace("\\", "/")


# ---------------------------------------------------------------------------
# Probe A — yaml-lint
# ---------------------------------------------------------------------------

def _find_docker_cli() -> Optional[str]:
    return shutil.which("docker")


def probe_a() -> Tuple[Optional[bool], str, List[str]]:
    """Return (passed, message, offenders).

    passed = None  → SKIP (compose file absent — cold-start)
    passed = True  → PASS
    passed = False → FAIL (with offender detail)
    """
    if not COMPOSE_FILE.exists():
        return None, f"compose file absent at {rel(COMPOSE_FILE)} — cold-start state, run after Phase 1 authors it", []

    docker = _find_docker_cli()
    if docker is not None:
        # Pre-populate required vars with placeholder values so `${VAR:?error}`
        # interpolation checks don't trip Probe A. Probe A's job is structural
        # YAML / service-ref validation, not operator-config completeness
        # (which is a runtime concern surfaced by compose up itself).
        env = os.environ.copy()
        for k in (
            "POSTGRES_PASSWORD",
            "DATABASE_URL",
            "FEEDBACKMONK_PUBLIC_URL",
            "FEEDBACKMONK_SESSION_SECRET",
        ):
            env.setdefault(k, "__probe_a_placeholder__")
        try:
            proc = subprocess.run(
                [docker, "compose", "-f", str(COMPOSE_FILE), "config", "--quiet"],
                capture_output=True,
                text=True,
                timeout=30,
                cwd=str(REPO_ROOT),
                env=env,
            )
        except subprocess.TimeoutExpired:
            return False, "`docker compose config` exceeded 30s timeout", [
                "remediation: check for circular service references or massive env interpolation lists"
            ]
        if proc.returncode == 0:
            return True, f"`docker compose config --quiet` on {rel(COMPOSE_FILE)}: clean", []
        err = (proc.stderr or "").strip() or (proc.stdout or "").strip() or "(no stderr)"
        offenders = []
        for line in err.splitlines():
            line = line.strip()
            if line:
                offenders.append(line)
        return False, "`docker compose config --quiet` reported errors", offenders

    # docker CLI absent — fall back to pure-Python YAML parse (WARN not FAIL).
    try:
        import yaml  # type: ignore  # pyyaml; optional dependency
    except ImportError:
        return True, (
            f"docker CLI not on PATH and pyyaml unavailable — Probe A SKIP with WARN "
            f"(install Docker Desktop or `pip install pyyaml` to enforce yaml-lint locally)"
        ), []
    try:
        data = yaml.safe_load(COMPOSE_FILE.read_text(encoding="utf-8"))
    except yaml.YAMLError as exc:
        return False, "pyyaml parse error (docker CLI not available — using fallback parser)", [str(exc)]
    if not isinstance(data, dict):
        return False, "compose file is not a YAML mapping at the top level", []
    if "services" not in data or not isinstance(data["services"], dict):
        return False, "compose file has no `services:` mapping", []
    return True, (
        f"docker CLI not on PATH — pyyaml fallback parse on {rel(COMPOSE_FILE)}: structurally valid "
        f"(install Docker Desktop for full `docker compose config` validation)"
    ), []


# ---------------------------------------------------------------------------
# Probe B — env-doc-xref against C21
# ---------------------------------------------------------------------------

# Catalog-name pattern: every project env var begins with FEEDBACKMONK_; we
# also include the two canonical non-prefixed vars documented in Contract
# C21: `DATABASE_URL` (Database section) and `RUST_LOG` (Logging /
# Observability section — tracing-subscriber EnvFilter is a standard Rust
# convention named outside the project's prefix). POSTGRES_* are
# postgres-internal vars (used by the official postgres image's
# `POSTGRES_USER` / `POSTGRES_PASSWORD` / `POSTGRES_DB` initialization
# protocol); they are container-level configuration, NOT part of the
# C21 application-side catalog, so Probe B excludes them from the
# cross-reference comparison entirely.
APP_ENV_PATTERN = re.compile(r"\b(FEEDBACKMONK_[A-Z0-9_]+|DATABASE_URL|RUST_LOG)\b")
POSTGRES_ENV_PATTERN = re.compile(r"\bPOSTGRES_[A-Z0-9_]+\b")


def _strip_comments_yaml(text: str) -> str:
    """Strip `#`-prefixed line tails. We do NOT try to be clever about
    `#` inside double-quoted strings — compose files in this project
    don't carry such constructs."""
    out_lines = []
    for line in text.splitlines():
        # Walk char-by-char; skip from first `#` not preceded by `\`.
        idx = line.find("#")
        if idx == -1:
            out_lines.append(line)
        else:
            out_lines.append(line[:idx])
    return "\n".join(out_lines)


def parse_compose_env_refs(compose_text: str) -> Set[str]:
    """Return the set of application env-var names referenced in compose.

    Strategy: walk the textual compose file, strip comments, then find every
    `${NAME}` / `${NAME:-default}` / `${NAME:?error}` interpolation AND every
    bareword `FEEDBACKMONK_*` / `DATABASE_URL` reference inside
    `environment:` blocks. Both surfaces are operator-facing (the compose
    interpolation requires the operator to set the var; the bareword inside
    `environment: { NAME: ${NAME} }` is the conventional "pass through from
    host" pattern).
    """
    cleaned = _strip_comments_yaml(compose_text)
    refs: Set[str] = set()
    # ${NAME} interpolations (with optional :-default or :?error).
    for m in re.finditer(r"\$\{([A-Z_][A-Z0-9_]*)(?::[-?][^}]*)?\}", cleaned):
        name = m.group(1)
        if APP_ENV_PATTERN.fullmatch(name):
            refs.add(name)
    # Bareword refs inside the file (covers `environment: - DATABASE_URL` shorthand).
    for m in APP_ENV_PATTERN.finditer(cleaned):
        refs.add(m.group(1))
    return refs


def parse_c21_catalog(selfhost_env_text: str) -> Set[str]:
    """Parse the canonical env-var catalog out of SELFHOST_ENV.md.

    Markdown tables of the form:
      | Name | Required | Default | 🔒 | Semantics |
      |---|---|---|---|---|
      | `DATABASE_URL` | **REQ** | — | 🔒 | ... |
      | `FEEDBACKMONK_PORT` | optional | `14304` | | ... |

    We pull names from the first column where the cell content is a backtick
    name (`...`) matching APP_ENV_PATTERN. The "Out of Scope (deferred)" section
    explicitly enumerates vars that don't exist yet (`FEEDBACKMONK_POLAR_*`,
    `FEEDBACKMONK_S3_*`, `FEEDBACKMONK_REDIS_URL`); we EXCLUDE those by
    detecting the trailing "## Out of Scope" header and stopping the parse
    there.
    """
    # Truncate at the "Out of Scope" header so deferred vars don't pollute.
    cutoff = re.search(r"(?im)^##\s+Out of Scope", selfhost_env_text)
    body = selfhost_env_text[: cutoff.start()] if cutoff else selfhost_env_text
    catalog: Set[str] = set()
    # Each catalog row: a markdown table row whose first cell is `NAME`.
    for line in body.splitlines():
        stripped = line.strip()
        if not stripped.startswith("|"):
            continue
        # The first cell is between the leading `|` and the next `|`.
        cells = [c.strip() for c in stripped.split("|")]
        # Skip header / separator rows.
        if not cells or len(cells) < 2:
            continue
        first = cells[1] if len(cells) > 1 else ""
        # Backticked name match.
        m = re.match(r"^`([A-Z_][A-Z0-9_]*)`$", first)
        if not m:
            continue
        name = m.group(1)
        if APP_ENV_PATTERN.fullmatch(name):
            catalog.add(name)
    return catalog


def probe_b() -> Tuple[Optional[bool], str, List[str], List[str]]:
    """Return (passed, message, fail_offenders, warn_offenders)."""
    if not COMPOSE_FILE.exists():
        return None, (
            f"compose file absent at {rel(COMPOSE_FILE)} — cold-start state, "
            "Probe B vacuous (no compose to scan)"
        ), [], []
    if not SELFHOST_ENV_DOC.exists():
        return False, (
            f"C21 catalog absent at {rel(SELFHOST_ENV_DOC)} — cannot cross-reference"
        ), ["restore docs/operations/SELFHOST_ENV.md (Contract C21 — frozen Stage 1)"], []

    compose_text = COMPOSE_FILE.read_text(encoding="utf-8")
    env_text = SELFHOST_ENV_DOC.read_text(encoding="utf-8")
    compose_refs = parse_compose_env_refs(compose_text)
    catalog = parse_c21_catalog(env_text)

    if not compose_refs:
        return False, (
            f"no application env refs found in {rel(COMPOSE_FILE)} — expected at least "
            "DATABASE_URL + FEEDBACKMONK_SESSION_SECRET + FEEDBACKMONK_PUBLIC_URL"
        ), ["compose appears to be an empty skeleton; author Phase 1 contents"], []

    undocumented = sorted(compose_refs - catalog)
    unreferenced = sorted(catalog - compose_refs)

    fail_offenders: List[str] = []
    warn_offenders: List[str] = []

    for name in undocumented:
        fail_offenders.append(
            f"{name}  referenced in compose but missing from C21 catalog "
            f"({rel(SELFHOST_ENV_DOC)}). Remediation: APPEND a catalog row "
            f"(pre-authorized self-mediated widening per GUIDE §8) — do NOT silence the oracle."
        )
    for name in unreferenced:
        warn_offenders.append(
            f"{name}  in C21 catalog but not referenced in compose (acceptable for "
            f"optional vars; flagged for visibility)"
        )

    if fail_offenders:
        return False, (
            f"compose env-refs ({len(compose_refs)}) ⟂ C21 catalog ({len(catalog)}) — "
            f"{len(undocumented)} undocumented, {len(unreferenced)} unreferenced"
        ), fail_offenders, warn_offenders

    return True, (
        f"compose env-refs ({len(compose_refs)}) ⊆ C21 catalog ({len(catalog)}); "
        f"{len(unreferenced)} catalog entries unreferenced (acceptable)"
    ), [], warn_offenders


# ---------------------------------------------------------------------------
# Probe C — full clean-state smoke
# ---------------------------------------------------------------------------

def _poll_health_ready(port: int, timeout_s: float) -> Tuple[bool, str]:
    """Poll http://localhost:{port}/health/ready until it returns 200 or the
    timeout elapses. Returns (success, last_body_or_error)."""
    deadline = time.time() + timeout_s
    last_err = "no attempts made"
    url = f"http://localhost:{port}/health/ready"
    while time.time() < deadline:
        try:
            req = urllib.request.Request(url, method="GET")
            with urllib.request.urlopen(req, timeout=3) as resp:  # noqa: S310 (localhost)
                code = resp.getcode()
                body = resp.read().decode("utf-8", errors="replace")
                if code == 200:
                    return True, body
                last_err = f"HTTP {code}: {body[:300]}"
        except urllib.error.HTTPError as exc:
            try:
                body = exc.read().decode("utf-8", errors="replace")
            except Exception:
                body = ""
            last_err = f"HTTP {exc.code}: {body[:300]}"
        except (urllib.error.URLError, ConnectionError, TimeoutError) as exc:
            last_err = f"{type(exc).__name__}: {exc}"
        time.sleep(HEALTH_POLL_INTERVAL_S)
    return False, last_err


def probe_c(full: bool) -> Tuple[Optional[bool], str, List[str]]:
    """Probe C — clean-state docker-compose smoke. Gated behind `--full`.

    Returns (passed, message, offenders). passed = None means skipped.
    """
    if not full:
        return None, "skipped (pass --full to run clean-state smoke)", []
    if not COMPOSE_FILE.exists():
        return None, (
            f"compose file absent at {rel(COMPOSE_FILE)} — Probe C skipped at cold-start"
        ), []
    docker = _find_docker_cli()
    if docker is None:
        return False, (
            "docker CLI not found on PATH — Probe C requires Docker Desktop / docker engine. "
            "Install per https://docs.docker.com/get-docker/ then re-run with --full."
        ), []

    port = int(os.environ.get("FEEDBACKMONK_PORT", PROBE_C_PORT_DEFAULT))
    compose_args = [docker, "compose", "-f", str(COMPOSE_FILE)]

    def run(*extra: str, timeout: int = 600) -> subprocess.CompletedProcess:
        return subprocess.run(
            list(compose_args) + list(extra),
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=str(REPO_ROOT),
        )

    offenders: List[str] = []
    teardown_ran = False
    try:
        # Pre-clean (defensive — a previous failed run may have left state).
        try:
            run("down", "-v", timeout=120)
        except subprocess.TimeoutExpired:
            offenders.append("pre-clean `down -v` exceeded 120s — system may be unhealthy")
            return False, "pre-clean failed", offenders

        # Cold up. Generous timeout because a fresh build cooks the entire
        # Rust dependency graph (cargo-chef) + bundles the admin-ui (npm run
        # build). Subsequent warm-cache runs complete in <60s; first run on
        # a fresh machine can be 15-25 min. Bumped from 600s after
        # observing DeadlineExceeded on cold P4 Stage 2 builds.
        up = run("up", "-d", "--build", "--wait", timeout=1800)
        if up.returncode != 0:
            err = (up.stderr or up.stdout or "(no output)").strip()
            offenders.append(f"`docker compose up -d --build --wait` exit {up.returncode}")
            for line in err.splitlines()[-30:]:
                offenders.append(f"  | {line}")
            return False, "compose up failed", offenders

        # Poll /health/ready.
        ok, last = _poll_health_ready(port, HEALTH_POLL_TIMEOUT_S)
        if not ok:
            offenders.append(
                f"GET http://localhost:{port}/health/ready did not return 200 within "
                f"{HEALTH_POLL_TIMEOUT_S:.0f}s"
            )
            offenders.append(f"  last response: {last[:500]}")
            # Capture api logs for diagnosis.
            logs = run("logs", "--no-color", "--tail", "60", "api", timeout=30)
            if logs.returncode == 0 and (logs.stdout or "").strip():
                offenders.append("  api recent logs (tail 60):")
                for line in (logs.stdout or "").splitlines()[-60:]:
                    offenders.append(f"    | {line}")
            return False, "health check failed", offenders

        # Parse the body and assert shape.
        try:
            body = json.loads(last)
        except json.JSONDecodeError:
            offenders.append(f"/health/ready returned non-JSON: {last[:300]}")
            return False, "health body is not valid JSON", offenders
        status = body.get("status")
        db_connected = body.get("db_connected")
        if status != "ok" or db_connected is not True:
            offenders.append(
                f"/health/ready body unexpected: status={status!r}, db_connected={db_connected!r} "
                f"(expected status='ok', db_connected=true)"
            )
            offenders.append(f"  full body: {last[:500]}")
            return False, "health body shape mismatch", offenders

        msg = (
            f"clean-state up → /health/ready 200 in <{HEALTH_POLL_TIMEOUT_S:.0f}s "
            f"(status={status!r}, db_connected={db_connected!r}, version={body.get('version')!r})"
        )
        return True, msg, []
    finally:
        # Always tear down to leave a clean state, even on failure.
        try:
            run("down", "-v", timeout=120)
            teardown_ran = True
        except Exception as exc:  # noqa: BLE001 — best-effort cleanup
            if not teardown_ran:
                sys.stderr.write(
                    f"\n[selfhost-compose-smoke] WARN: teardown `down -v` failed: {exc}\n"
                    f"  Manual cleanup may be required: `docker compose -f {rel(COMPOSE_FILE)} down -v`\n"
                )


# ---------------------------------------------------------------------------
# Driver
# ---------------------------------------------------------------------------

def main() -> int:
    parser = argparse.ArgumentParser(description="selfhost-compose-smoke Verification Oracle")
    parser.add_argument(
        "--full",
        action="store_true",
        help="Run Probe C clean-state smoke (docker compose down -v && up -d && curl /health/ready). "
        "Off by default for inner-loop cost; CI / /0-uldf-finalize Phase 11 run with --full.",
    )
    args = parser.parse_args()

    a_passed, a_message, a_offenders = probe_a()
    b_passed, b_message, b_fail_offenders, b_warn_offenders = probe_b()
    c_passed, c_message, c_offenders = probe_c(args.full)

    fails = (
        (1 if a_passed is False else 0)
        + (1 if b_passed is False else 0)
        + (1 if c_passed is False else 0)
    )

    if fails == 0:
        print("PASS selfhost-compose-smoke")
        prefix_a = "vacuous PASS — " if a_passed is None else ""
        print(f"  Probe A (yaml-lint): {prefix_a}{a_message}")
        prefix_b = "vacuous PASS — " if b_passed is None else ""
        print(f"  Probe B (env-doc-xref): {prefix_b}{b_message}")
        if c_passed is True:
            print(f"  Probe C (full-smoke): {c_message}")
        elif c_passed is None:
            print(f"  Probe C (full-smoke): {c_message}")
        for w in b_warn_offenders:
            print(f"    WARN: {w}")
        return 0

    print(f"FAIL selfhost-compose-smoke ({fails} probe(s) failed)")
    if a_passed is False:
        print()
        print(f"Probe A failure (yaml-lint): {a_message}")
        for o in a_offenders:
            print(f"  {o}")
        print(
            "  Remediation: fix the YAML / interpolation / service-ref errors above; "
            f"validate locally with `docker compose -f {rel(COMPOSE_FILE)} config`."
        )
    if b_passed is False:
        print()
        print(f"Probe B failure (env-doc-xref): {b_message}")
        for o in b_fail_offenders:
            print(f"  {o}")
        if b_warn_offenders:
            print("  WARN entries (not failing the probe):")
            for w in b_warn_offenders:
                print(f"    {w}")
    if c_passed is False:
        print()
        print(f"Probe C failure (full-smoke): {c_message}")
        for o in c_offenders:
            print(f"  {o}")
        print(
            "  Remediation: triage with `docker compose -f deploy/docker/docker-compose.yml logs api`; "
            "common causes: DATABASE_URL typo, FEEDBACKMONK_SESSION_SECRET wrong length, "
            "port binding conflict (deconflict via FEEDBACKMONK_PORT)."
        )
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(2)
