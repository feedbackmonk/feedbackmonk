#!/usr/bin/env python3
"""Self-test for the `project-type` oracle -- a C6 cell module."""

from __future__ import annotations

import importlib.util
import json
import os
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))
if not (pathlib.Path(sys.path[0]) / "uldf").is_dir():
    sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[3] / "scripts" / "lib"))

from uldf import cells, oracle, paths  # noqa: E402
from uldf.cells import T, cell  # noqa: E402

from pathlib import Path  # noqa: E402

HERE = Path(__file__).resolve().parent


def _load_run():
    spec = importlib.util.spec_from_file_location("project_type_run", HERE / "run.py")
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


RUN = _load_run()


def measure(root: Path) -> dict:
    ctx = oracle.Context(project_root=root, anchor_root=root, home=paths.home())
    return RUN.run(ctx)


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


@cell(red="pre-implementation: run() did not exist")
def cell_empty_dir_is_pass_with_no_languages(t: T) -> None:
    root = t.tmpdir() / "empty"
    root.mkdir()
    r = measure(root)
    t.eq(r["verdict"], "pass")
    t.eq(r["data"]["languages"], [])


@cell(red="pre-implementation: package.json detection did not exist")
def cell_package_json_reports_npm_commands_and_typescript(t: T) -> None:
    root = t.tmpdir() / "js"
    write(root / "package.json", json.dumps({
        "dependencies": {"react": "^18.0.0", "typescript": "^5.0.0"},
        "scripts": {"test": "jest", "dev": "vite"},
    }))
    d = measure(root)["data"]
    t.eq(d["languages"], ["javascript", "typescript"])
    t.ok("react" in d["frameworks"], d["frameworks"])
    t.eq(d["test_command"], "npm test")
    t.eq(d["dev_command"], "npm run dev")
    t.eq(d["package_managers"], ["npm"])


@cell(red="pre-implementation: lockfile-based package manager detection did not exist")
def cell_pnpm_lockfile_overrides_default_npm(t: T) -> None:
    root = t.tmpdir() / "js-pnpm"
    write(root / "package.json", json.dumps({"scripts": {}}))
    write(root / "pnpm-lock.yaml", "lockfileVersion: 6\n")
    d = measure(root)["data"]
    t.eq(d["package_managers"], ["pnpm"])


@cell(red="pre-implementation: pyproject.toml textual scan did not exist")
def cell_pyproject_poetry_and_pytest(t: T) -> None:
    root = t.tmpdir() / "py"
    write(root / "pyproject.toml",
          "[tool.poetry]\nname = \"x\"\n\n[tool.pytest.ini_options]\naddopts = \"-q\"\n")
    d = measure(root)["data"]
    t.eq(d["languages"], ["python"])
    t.ok("poetry" in d["package_managers"], d["package_managers"])
    t.eq(d["test_command"], "pytest")


@cell(red="pre-implementation: requirements.txt-only python detection did not exist")
def cell_requirements_txt_alone_is_python(t: T) -> None:
    root = t.tmpdir() / "py-reqs"
    write(root / "requirements.txt", "flask\n")
    d = measure(root)["data"]
    t.eq(d["languages"], ["python"])


@cell(red="pre-implementation: Cargo.toml / go.mod / csproj detection did not exist")
def cell_cargo_go_dotnet_markers(t: T) -> None:
    cargo_root = t.tmpdir() / "rs"
    write(cargo_root / "Cargo.toml", "[package]\nname = \"x\"\n")
    d = measure(cargo_root)["data"]
    t.eq(d["languages"], ["rust"])
    t.eq(d["test_command"], "cargo test")
    t.eq(d["dev_command"], "cargo run")

    go_root = t.tmpdir() / "go"
    write(go_root / "go.mod", "module example.com/x\n")
    d = measure(go_root)["data"]
    t.eq(d["languages"], ["go"])
    t.eq(d["test_command"], "go test ./...")

    dotnet_root = t.tmpdir() / "dn"
    write(dotnet_root / "app.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>\n")
    d = measure(dotnet_root)["data"]
    t.eq(d["languages"], ["csharp"])
    t.eq(d["test_command"], "dotnet test")


@cell(red="observed red 2026-09-07: an RN + Next.js monorepo fixture answered frameworks=[] (brief 20260906-2005)")
def cell_a_monorepo_reports_the_frameworks_of_its_workspace_packages(t: T) -> None:
    """The workspace ROOT manifest of a monorepo names no framework -- the frameworks live in the
    package manifests -- so a root-only read answers a confident `frameworks: []` on a repo with a
    ten-screen mobile app and an eighteen-route web app. This fixture is a monorepo on purpose: a
    single-package fixture passes while the real case fails (the brief's own caution)."""
    root = t.tmpdir() / "mono"
    write(root / "package.json", json.dumps({
        "name": "mono", "private": True, "workspaces": ["apps/*", "packages/*"],
        "devDependencies": {"turbo": "^2.0.0", "typescript": "^5.0.0"},
        "scripts": {"test": "turbo run test", "dev": "turbo run dev"}}))
    write(root / "pnpm-lock.yaml", "lockfileVersion: 9\n")
    write(root / "apps" / "web" / "package.json", json.dumps({
        "name": "web", "dependencies": {"next": "^15.0.0", "react": "^19.0.0"}}))
    write(root / "apps" / "mobile" / "package.json", json.dumps({
        "name": "mobile", "dependencies": {"expo": "~52.0.0", "react-native": "0.76.0",
                                           "react": "^19.0.0"}}))
    write(root / "packages" / "core" / "package.json", json.dumps({"name": "@mono/core"}))
    write(root / "node_modules" / "left-pad" / "package.json", json.dumps({
        "name": "left-pad", "dependencies": {"vue": "^3.0.0"}}))     # a dependency is NOT a workspace
    d = measure(root)["data"]
    t.eq(d["languages"], ["javascript", "typescript"])
    for framework in ("next", "react", "react-native", "expo"):
        t.ok(framework in d["frameworks"],
             f"{framework} lives in a workspace package and was not reported: {d['frameworks']}")
    t.ok("vue" not in d["frameworks"], "node_modules was read as a workspace")
    t.eq(d["package_managers"], ["pnpm"])
    t.ok("apps/web" in d.get("workspaces", []) and "apps/mobile" in d.get("workspaces", []),
         f"the answer does not name the workspace packages it read: {d.get('workspaces')}")


@cell(red="observed red 2026-09-07: pnpm-workspace.yaml globs were never read, so a pnpm monorepo answered like a single package")
def cell_pnpm_workspace_yaml_names_the_packages_when_package_json_does_not(t: T) -> None:
    root = t.tmpdir() / "pnpm-mono"
    write(root / "package.json", json.dumps({"name": "m", "private": True}))
    write(root / "pnpm-workspace.yaml", "packages:\n  - 'apps/*'\n  - \"tools/**\"\n")
    write(root / "apps" / "web" / "package.json", json.dumps({"dependencies": {"svelte": "^5"}}))
    write(root / "tools" / "a" / "b" / "package.json", json.dumps({"dependencies": {"express": "^5"}}))
    d = measure(root)["data"]
    t.ok("svelte" in d["frameworks"] and "express" in d["frameworks"], d["frameworks"])


if __name__ == "__main__":
    sys.exit(cells.main())
