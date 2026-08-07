#!/usr/bin/env python3
"""probandurgy-footprint Verification Oracle (shared implementation).

Answers: "For each declared Probandurgy mechanism in this project's manifest,
is the declared compile-time-presence footprint enforced by an active gate in
the build configuration?"

Phase 1: declared-vs-implemented check only. Phase 2 (artifact inspection) is
explicitly deferred per FOOTPRINT-04.

Cross-platform parity contract: run.sh and run.ps1 both invoke this script so
they produce structurally identical JSON output for the same fixture.
"""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path
from typing import Any

SCHEMA_VERSION = "1"
DEFAULT_MANIFEST_PATH = ".claude/manifests/probandurgy-footprint.json"
ENV_OVERRIDE = "PROBANDURGY_FOOTPRINT_MANIFEST"


def emit(obj: dict[str, Any]) -> None:
    json.dump(obj, sys.stdout, separators=(",", ":"), ensure_ascii=False)
    sys.stdout.write("\n")


def graceful_absent() -> dict[str, Any]:
    return {
        "schemaVersion": SCHEMA_VERSION,
        "manifest_present": False,
        "mechanisms_declared": [],
        "violations": [],
        "advisory": True,
    }


def violation(mechanism: str | None, kind: str, evidence: str, remediation: str) -> dict[str, Any]:
    return {
        "mechanism": mechanism,
        "kind": kind,
        "evidence": evidence,
        "remediation": remediation,
    }


# ---- Per-extension parsers -------------------------------------------------
#
# Each parser receives the file text and the named anchor (e.g. "features.aria-dev",
# "define.__ARIA_ENABLED__") and returns True iff the gate is present. Parsers
# never raise on malformed input — they return False and the caller emits a
# declared-but-no-gate violation pointing at the file:anchor.


def parse_cargo_features(text: str, anchor: str) -> bool:
    if not anchor.startswith("features."):
        return False
    feature_name = anchor[len("features."):]
    in_features = False
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith("[") and line.endswith("]"):
            in_features = line == "[features]"
            continue
        if not in_features or not line or line.startswith("#"):
            continue
        # Match `name = ...` or `"name" = ...`
        m = re.match(r'(?:"([^"]+)"|([A-Za-z0-9_\-]+))\s*=', line)
        if m:
            key = m.group(1) or m.group(2)
            if key == feature_name:
                return True
    return False


def _js_has_define_key(text: str, container: str, key: str) -> bool:
    """Match `<container>: { ... <key>: ... }` or `<container>({ ... <key>: ... })`.

    Heuristic: locate the container introducer, then a `{`, then scan a
    balanced-brace block looking for the key as an identifier or quoted string.
    Sufficient for Vite/Webpack/Rollup; AST is overkill for Phase 1.
    """
    # Look for `<container>:` OR `<container>(` patterns (DefinePlugin etc.)
    for intro_pat in (
        re.escape(container) + r"\s*:\s*\{",
        re.escape(container) + r"\s*\(\s*\{",
    ):
        m = re.search(intro_pat, text)
        if not m:
            continue
        # Scan from the `{` for the matching `}` while respecting nesting.
        i = m.end() - 1  # position of `{`
        assert text[i] == "{"
        depth = 0
        j = i
        while j < len(text):
            c = text[j]
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        block = text[i + 1 : j]
        # Look for key as `<key>:` or `"<key>":` or `'<key>':`
        key_re = re.compile(
            r'(?:^|[\s,{])(?:"' + re.escape(key) + r'"|\'' + re.escape(key) + r"'|"
            + re.escape(key) + r")\s*:"
        )
        if key_re.search(block):
            return True
    return False


def parse_vite_define(text: str, anchor: str) -> bool:
    if not anchor.startswith("define."):
        return False
    return _js_has_define_key(text, "define", anchor[len("define."):])


def parse_webpack_defineplugin(text: str, anchor: str) -> bool:
    if not anchor.startswith("DefinePlugin."):
        return False
    return _js_has_define_key(text, "DefinePlugin", anchor[len("DefinePlugin."):])


def parse_rollup_replace(text: str, anchor: str) -> bool:
    # Accept either #replace.<NAME> or #define.<NAME>
    if anchor.startswith("replace."):
        return _js_has_define_key(text, "replace", anchor[len("replace."):]) \
            or _js_has_define_key(text, "values", anchor[len("replace."):])
    if anchor.startswith("define."):
        return _js_has_define_key(text, "define", anchor[len("define."):])
    return False


def parse_dockerfile_arg(text: str, anchor: str) -> bool:
    if not anchor.startswith("ARG."):
        return False
    name = anchor[len("ARG."):]
    arg_re = re.compile(r"^\s*ARG\s+([A-Za-z_][A-Za-z0-9_]*)", re.MULTILINE)
    return any(m.group(1) == name for m in arg_re.finditer(text))


# Map filename (basename, lowercased) → list of (extension_predicate, parser).
def select_parser(filename: str):
    base = os.path.basename(filename).lower()
    if base == "cargo.toml":
        return parse_cargo_features
    if base.startswith("vite.config."):
        return parse_vite_define
    if base.startswith("webpack.config."):
        return parse_webpack_defineplugin
    if base.startswith("rollup.config."):
        return parse_rollup_replace
    if base == "dockerfile":
        return parse_dockerfile_arg
    return None


# ---- Main ------------------------------------------------------------------


def resolve_manifest_path() -> Path:
    override = os.environ.get(ENV_OVERRIDE)
    if override:
        return Path(override)
    return Path(DEFAULT_MANIFEST_PATH)


def check_gate_location(loc: str, mechanism: str) -> tuple[bool, dict[str, Any] | None]:
    """Return (gate_present, violation_or_none)."""
    if "#" not in loc:
        return False, violation(
            mechanism,
            "gate-location-not-found",
            loc,
            "gate_locations[] entries must be of the form '<file>#<anchor>' (e.g. 'Cargo.toml#features.aria-dev'). Fix the manifest entry.",
        )
    file_part, anchor = loc.split("#", 1)
    p = Path(file_part)
    if not p.exists():
        return False, violation(
            mechanism,
            "gate-location-not-found",
            loc,
            f"File '{file_part}' does not exist. Either create the file with the gate stanza or remove the gate_locations[] entry.",
        )
    parser = select_parser(file_part)
    if parser is None:
        return False, violation(
            mechanism,
            "gate-location-not-found",
            loc,
            f"No Phase 1 parser for '{file_part}'. Supported: Cargo.toml, vite.config.*, webpack.config.*, rollup.config.*, Dockerfile. Add a parser to run.py or remove the entry.",
        )
    try:
        text = p.read_text(encoding="utf-8-sig")
    except (OSError, UnicodeDecodeError) as e:
        return False, violation(
            mechanism,
            "gate-location-not-found",
            loc,
            f"Could not read '{file_part}': {e.__class__.__name__}. Verify file encoding (UTF-8) and permissions.",
        )
    found = parser(text, anchor)
    if not found:
        return False, violation(
            mechanism,
            "declared-but-no-gate",
            loc,
            f"Add the gate stanza named in '{loc}' to '{file_part}', or remove the gate_locations[] entry from the manifest if the gate is no longer required.",
        )
    return True, None


def main() -> int:
    manifest_path = resolve_manifest_path()

    if not manifest_path.exists():
        emit(graceful_absent())
        return 0

    try:
        raw = manifest_path.read_text(encoding="utf-8-sig")
    except (OSError, UnicodeDecodeError) as e:
        emit({
            "schemaVersion": SCHEMA_VERSION,
            "manifest_present": True,
            "mechanisms_declared": [],
            "violations": [violation(
                None,
                "manifest-malformed",
                str(manifest_path).replace("\\", "/"),
                f"Could not read manifest: {e.__class__.__name__}. Verify file encoding (UTF-8) and permissions.",
            )],
            "advisory": True,
        })
        return 0

    try:
        manifest = json.loads(raw)
    except json.JSONDecodeError:
        emit({
            "schemaVersion": SCHEMA_VERSION,
            "manifest_present": True,
            "mechanisms_declared": [],
            "violations": [violation(
                None,
                "manifest-malformed",
                str(manifest_path).replace("\\", "/"),
                "Fix JSON syntax in the manifest (validate with `jq . " + str(manifest_path).replace("\\", "/") + "`). Phase 1 cannot proceed until the manifest parses cleanly.",
            )],
            "advisory": True,
        })
        return 0

    mechanisms_in = manifest.get("mechanisms") or []
    if not isinstance(mechanisms_in, list):
        mechanisms_in = []

    mechanisms_out: list[dict[str, Any]] = []
    violations: list[dict[str, Any]] = []

    for m in mechanisms_in:
        if not isinstance(m, dict):
            continue
        name = m.get("name") or ""
        axis_a = m.get("axis_a") or "not-applicable"
        axis_b = m.get("axis_b") or "not-applicable"
        gate_locations = m.get("gate_locations") or []
        if not isinstance(gate_locations, list):
            gate_locations = []
        verifier_script = m.get("verifier_script")
        waivers_in = m.get("waivers") or []
        if not isinstance(waivers_in, list):
            waivers_in = []

        gate_implementation_found = True if gate_locations else False
        for loc in gate_locations:
            if not isinstance(loc, str):
                gate_implementation_found = False
                continue
            ok, v = check_gate_location(loc, name)
            if not ok:
                gate_implementation_found = False
                if v is not None:
                    violations.append(v)

        self_test_verifier_present = False
        if isinstance(verifier_script, str) and verifier_script:
            self_test_verifier_present = Path(verifier_script).exists()

        for w in waivers_in:
            if not isinstance(w, dict):
                continue
            rationale = w.get("rationale")
            if isinstance(rationale, str) and rationale and not Path(rationale).exists():
                violations.append(violation(
                    name,
                    "waiver-without-rationale-file",
                    rationale,
                    f"Create the rationale doc at {rationale} describing why extracting the {w.get('surface', '<surface>')} surface from the Probandurgy module is impractical, or remove the waiver entry.",
                ))

        mechanisms_out.append({
            "mechanism": name,
            "declared_axis_a": axis_a,
            "declared_axis_b": axis_b,
            "gate_implementation_found": gate_implementation_found,
            "gate_locations": [g for g in gate_locations if isinstance(g, str)],
            "self_test_verifier_present": self_test_verifier_present,
            "waivers": [w for w in waivers_in if isinstance(w, dict)],
        })

    emit({
        "schemaVersion": SCHEMA_VERSION,
        "manifest_present": True,
        "mechanisms_declared": mechanisms_out,
        "violations": violations,
        "advisory": True,
    })
    return 0


if __name__ == "__main__":
    sys.exit(main())
