"""project-type (C2, RB-21/22): languages, build systems, and how the project is run/tested.

Root-level marker files, plus the WORKSPACE package manifests a JavaScript monorepo declares
(brief 20260906-2005: a React Native + Next.js monorepo answered `frameworks: []` with confidence,
because every framework lived in `apps/*/package.json` and only the root was read -- a confidently
wrong oracle is worse than none). Workspaces come from `package.json` `workspaces` and from
`pnpm-workspace.yaml`, expanded with `node_modules` and `.git` pruned, and the packages read are
NAMED in `data.workspaces` so a reader can see what the answer rests on. `finalize`'s language
detection (RB-36 (1)) reads `data` to pick lint/test commands, so commands are named, not just a
label. No TOML or YAML parser: `PYTHON_MIN` is 3.10 and `tomllib` needs 3.11, so `pyproject.toml`
and `pnpm-workspace.yaml` are read textually for the handful of markers this needs.
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(
    os.environ.get("ULDF_HOME", pathlib.Path.home() / ".claude")) / "scripts" / "lib"))

from uldf import oracle  # noqa: E402

_JS_FRAMEWORKS = ("react", "react-native", "expo", "next", "vue", "svelte", "express", "nestjs")
#: Never descended into when a workspace glob is expanded: a dependency is not a workspace.
_PRUNED = {"node_modules", ".git"}
#: A ceiling on manifests read per batch, so a pathological glob cannot blow the runtime budget.
_MAX_WORKSPACES = 200
_PY_FRAMEWORKS = ("django", "flask", "fastapi")


def _read(root, name) -> str | None:
    p = root / name
    if not p.is_file():
        return None
    try:
        return p.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None


def _pnpm_workspace_globs(text: str) -> list[str]:
    """The `packages:` list of `pnpm-workspace.yaml`, read textually: `- 'x'`, `- "x"`, `- x`."""
    globs: list[str] = []
    in_packages = False
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].rstrip()
        if not line.strip():
            continue
        if not line.startswith((" ", "\t", "-")):
            in_packages = line.strip().startswith("packages:")
            continue
        if in_packages and line.strip().startswith("- "):
            value = line.strip()[2:].strip().strip("'\"")
            if value and not value.startswith("!"):
                globs.append(value)
    return globs


def workspace_globs(root, pkg: dict) -> list[str]:
    """Every workspace glob the root declares, from `package.json` and `pnpm-workspace.yaml`."""
    declared = pkg.get("workspaces") if isinstance(pkg, dict) else None
    if isinstance(declared, dict):
        declared = declared.get("packages")
    globs = [g for g in (declared or []) if isinstance(g, str) and not g.startswith("!")]
    pnpm = _read(root, "pnpm-workspace.yaml")
    if pnpm:
        globs += _pnpm_workspace_globs(pnpm)
    return globs


def _glob_match(rel: str, pattern: str) -> bool:
    """`fnmatch` with `**` meaning any number of path segments (fnmatch's `*` crosses `/` too,
    which is what makes the simple translation right here)."""
    import fnmatch
    return fnmatch.fnmatchcase(rel, pattern.replace("**", "*"))


def expand_workspaces(root, globs: list[str]) -> list[pathlib.Path]:
    """Directories matching the globs that hold a `package.json`, `node_modules`/`.git` pruned.

    `**` is walked with `os.walk` and pruned rather than handed to `Path.glob`, which would
    descend into every `node_modules` under the prefix; a single-level glob goes to `Path.glob`.
    """
    found: list[pathlib.Path] = []
    for pattern in globs:
        pattern = pattern.replace("\\", "/").strip("/")
        if "**" in pattern:
            prefix = pattern.split("**", 1)[0].rstrip("/")
            base = root / prefix if prefix else root
            if not base.is_dir():
                continue
            for dirpath, dirnames, filenames in os.walk(base):
                dirnames[:] = [d for d in dirnames if d not in _PRUNED]
                rel = pathlib.Path(dirpath).relative_to(root).as_posix()
                if "package.json" in filenames and _glob_match(rel, pattern):
                    found.append(pathlib.Path(dirpath))
                if len(found) >= _MAX_WORKSPACES:
                    break
        else:
            for candidate in sorted(root.glob(pattern)):
                if not candidate.is_dir() or not (candidate / "package.json").is_file():
                    continue
                if _PRUNED & set(candidate.relative_to(root).parts):
                    continue
                found.append(candidate)
    out: list[pathlib.Path] = []
    seen: set = set()
    for path in found:
        key = path.resolve()
        if key not in seen and key != root.resolve():
            seen.add(key)
            out.append(path)
    return out[:_MAX_WORKSPACES]


def run(ctx: oracle.Context) -> dict:
    root = ctx.project_root
    languages: list[str] = []
    frameworks: list[str] = []
    build_systems: list[str] = []
    package_managers: list[str] = []
    workspaces: list[str] = []
    test_command = None
    dev_command = None

    pkg_text = _read(root, "package.json")
    if pkg_text is not None:
        languages.append("javascript")
        try:
            pkg = json.loads(pkg_text)
        except ValueError:
            pkg = {}
        deps: dict = {}
        deps.update(pkg.get("dependencies") or {})
        deps.update(pkg.get("devDependencies") or {})
        packages = expand_workspaces(root, workspace_globs(root, pkg))
        workspaces = [p.relative_to(root).as_posix() for p in packages]
        typescript = "typescript" in deps or (root / "tsconfig.json").is_file()
        for package in packages:
            text = _read(package, "package.json")
            try:
                manifest = json.loads(text) if text else {}
            except ValueError:
                continue
            if not isinstance(manifest, dict):
                continue
            deps.update(manifest.get("dependencies") or {})
            deps.update(manifest.get("devDependencies") or {})
            typescript = typescript or (package / "tsconfig.json").is_file()
        if typescript or "typescript" in deps:
            languages.append("typescript")
        frameworks += [name for name in _JS_FRAMEWORKS if name in deps]
        if (root / "pnpm-lock.yaml").is_file():
            package_managers.append("pnpm")
        elif (root / "yarn.lock").is_file():
            package_managers.append("yarn")
        else:
            package_managers.append("npm")
        build_systems.append("npm")
        scripts = pkg.get("scripts") or {}
        if isinstance(scripts, dict):
            if "test" in scripts:
                test_command = "npm test"
            if "dev" in scripts:
                dev_command = "npm run dev"
            elif "start" in scripts:
                dev_command = "npm start"

    pyproject_text = _read(root, "pyproject.toml")
    has_python_markers = (pyproject_text is not None or _read(root, "setup.py") is not None
                           or _read(root, "requirements.txt") is not None)
    if has_python_markers:
        languages.append("python")
        build_systems.append("pip")
        if pyproject_text:
            if "[tool.poetry]" in pyproject_text:
                package_managers.append("poetry")
                build_systems.append("poetry")
            if re.search(r"\[tool\.pytest", pyproject_text):
                test_command = "pytest"
            lowered = pyproject_text.lower()
            frameworks += [name for name in _PY_FRAMEWORKS if name in lowered]
        if test_command is None and ((root / "pytest.ini").is_file()
                                      or (root / "tests").is_dir()):
            test_command = "pytest"

    if _read(root, "Cargo.toml") is not None:
        languages.append("rust")
        build_systems.append("cargo")
        package_managers.append("cargo")
        test_command = test_command or "cargo test"
        dev_command = dev_command or "cargo run"

    if _read(root, "go.mod") is not None:
        languages.append("go")
        build_systems.append("go")
        test_command = test_command or "go test ./..."
        dev_command = dev_command or "go run ."

    if list(root.glob("*.csproj")) or list(root.glob("*.sln")):
        languages.append("csharp")
        build_systems.append("dotnet")
        test_command = test_command or "dotnet test"
        dev_command = dev_command or "dotnet run"

    data = {"languages": languages, "frameworks": frameworks, "build_systems": build_systems,
            "test_command": test_command, "dev_command": dev_command,
            "package_managers": package_managers, "workspaces": workspaces}

    if not languages:
        return oracle.result("pass", "no recognized project markers at the root", data=data)
    return oracle.result("pass", f"languages={','.join(languages)}"[:200], data=data)


if __name__ == "__main__":
    sys.exit(oracle.main(run))
