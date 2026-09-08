#!/usr/bin/env python3
"""i18n-catalog-integrity Verification Oracle -- v1.0.0.

Assertion: see manifest.json `assertion` (frozen before any probe existed).

Five probes:
  A. generated locale tables equal i18n/locales.json (scripts/i18n/gen-locales.py --check)
  B. every catalog file parses and starts with a well-formed _meta
  C. every non-en key is a subset of en's + {{placeholder}} sets match per key
  D. CLDR-required plural categories are present for every translated plural base
  E. no mojibake byte sequences, no leaked HTML entities

C/D/E delegate to scripts/i18n/validate.py's own defect classes (extra_key +
placeholder_mismatch; plural_category_gap; mojibake + leaked_entity respectively)
so the catalog-shape contract has exactly one implementation -- this oracle
does not re-derive it.

Exit 0 PASS, 1 FAIL, 2 environment failure. Python 3.8+, stdlib only.
"""
from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
CATALOGS = ROOT / "i18n" / "locales"
GEN = ROOT / "scripts" / "i18n" / "gen-locales.py"
SCRIPTS_I18N = ROOT / "scripts" / "i18n"
STATUSES = {
    "source",
    "untranslated",
    "machine-translated — community review welcome",
    "english-fallback — provider unsupported",
}


def _load_validate(path: Path = None, module_name: str = "_oracle_validate"):
    spec = importlib.util.spec_from_file_location(module_name, path or (SCRIPTS_I18N / "validate.py"))
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


def probe_a() -> list[str]:
    if not GEN.exists():
        return [f"generator missing: {GEN.relative_to(ROOT).as_posix()}"]
    r = subprocess.run([sys.executable, str(GEN), "--check"], capture_output=True, text=True, cwd=ROOT)
    if r.returncode != 0:
        return [line for line in (r.stdout + r.stderr).splitlines() if line.strip()]
    return []


def probe_b() -> list[str]:
    fails: list[str] = []
    for path in sorted(CATALOGS.glob("*/*.json")):
        rel = path.relative_to(ROOT).as_posix()
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as exc:
            fails.append(f"{rel}: does not parse ({exc})")
            continue
        if not isinstance(data, dict) or not data:
            fails.append(f"{rel}: not a non-empty JSON object")
            continue
        first = next(iter(data))
        if first != "_meta":
            fails.append(f"{rel}: first key must be _meta (found {first!r})")
            continue
        meta = data["_meta"]
        if not isinstance(meta, dict) or not meta.get("language") or meta.get("status") not in STATUSES:
            fails.append(f"{rel}: _meta needs language + a known status (got {meta!r})")
    return fails


def probe_cde() -> tuple[list[str], list[str], list[str]]:
    """Returns (probe_c_fails, probe_d_fails, probe_e_fails) via validate.py."""
    validate = _load_validate()
    findings = validate.run(None, None)
    c = [f"{f['locale']}/{f['namespace']}.json {f['key']}: {f['detail']}"
         for f in findings if f["class"] in ("extra_key", "placeholder_mismatch")]
    d = [f"{f['locale']}/{f['namespace']}.json {f['key']}: {f['detail']}"
         for f in findings if f["class"] == "plural_category_gap"]
    e = [f"{f['locale']}/{f['namespace']}.json {f['key']}: {f['detail']}"
         for f in findings if f["class"] in ("mojibake", "leaked_entity")]
    return c, d, e


def self_test() -> int:
    """Invertible, against throwaway scratch mirrors -- never the live tree
    (the live tree may have in-flight edits from live sibling workers).
    Both legs recorded in README.md "Adversarial self-test"; this is the
    mechanized form of that manual demonstration."""
    import shutil
    import tempfile

    ok = True

    def check(name: str, condition: bool) -> None:
        nonlocal ok
        print(f"  self-test [{name}]: {'PASS' if condition else 'FAIL'}")
        if not condition:
            ok = False

    # --- Probe C/D/E leg: a scratch i18n/ + scripts/i18n mirror ---
    tmp = Path(tempfile.mkdtemp(prefix="i18n-catalog-integrity-selftest-cde-"))
    try:
        shutil.copytree(SCRIPTS_I18N, tmp / "scripts" / "i18n")
        (tmp / "i18n" / "locales" / "en").mkdir(parents=True)
        (tmp / "i18n" / "locales" / "de").mkdir(parents=True)
        (tmp / "i18n" / "locales.json").write_text(
            '{"default":"en","overrides":{},"bare_defaults":{},"locales":['
            '{"code":"en","name":"English","dir":"ltr","deepl":"EN-US","base":null},'
            '{"code":"de","name":"Deutsch","dir":"ltr","deepl":"DE","base":null}]}',
            encoding="utf-8")
        (tmp / "i18n" / "source-hashes.json").write_text(
            '{"widget":{},"public":{},"admin":{},"email":{},"status":{}}', encoding="utf-8")
        (tmp / "i18n" / "locales" / "en" / "widget.json").write_text(
            '{"_meta":{"language":"English","status":"source"},"widget":{"a":"Hello {{name}}"}}',
            encoding="utf-8")
        (tmp / "i18n" / "locales" / "de" / "widget.json").write_text(
            '{"_meta":{"language":"Deutsch","status":"machine-translated — community review welcome"},'
            '"widget":{"a":"Hallo"}}', encoding="utf-8")  # {{name}} dropped -- placeholder mismatch

        validate = _load_validate(tmp / "scripts" / "i18n" / "validate.py", "_selftest_validate")
        cat = validate.cat
        saved = {n: getattr(cat, n) for n in ("ROOT", "I18N_DIR", "LOCALES_JSON", "CATALOG_DIR", "SOURCE_HASHES_JSON")}
        cat.ROOT = tmp
        cat.I18N_DIR = tmp / "i18n"
        cat.LOCALES_JSON = tmp / "i18n" / "locales.json"
        cat.CATALOG_DIR = tmp / "i18n" / "locales"
        cat.SOURCE_HASHES_JSON = tmp / "i18n" / "source-hashes.json"
        try:
            findings = validate.run(None, None)
            classes = {f["class"] for f in findings}
            check("Probe C fires on a placeholder-mismatch in a temp copy of de/widget.json",
                  "placeholder_mismatch" in classes)
        finally:
            for n, v in saved.items():
                setattr(cat, n, v)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    # --- Probe A leg: a hand edit to a generated locale table ---
    tmp2 = Path(tempfile.mkdtemp(prefix="i18n-catalog-integrity-selftest-a-"))
    try:
        shutil.copytree(ROOT / "i18n", tmp2 / "i18n")
        shutil.copytree(SCRIPTS_I18N, tmp2 / "scripts" / "i18n")
        for rel in ("admin-ui/src/i18n/locales.gen.ts", "widget/src/locales.gen.ts",
                    "crates/feedbackmonk-i18n/src/locales.gen.rs"):
            src, dst = ROOT / rel, tmp2 / rel
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(src, dst)

        real_root, real_gen = ROOT, GEN
        globals()["ROOT"], globals()["GEN"] = tmp2, tmp2 / "scripts" / "i18n" / "gen-locales.py"
        try:
            baseline_fails = probe_a()
            check("scratch mirror starts clean (Probe A baseline)", not baseline_fails)
            hand_edited = tmp2 / "admin-ui" / "src" / "i18n" / "locales.gen.ts"
            hand_edited.write_text(hand_edited.read_text(encoding="utf-8") + "\n// hand edit\n", encoding="utf-8")
            after_fails = probe_a()
            check("Probe A fires on a hand edit to a generated locale table", bool(after_fails))
        finally:
            globals()["ROOT"], globals()["GEN"] = real_root, real_gen
    finally:
        shutil.rmtree(tmp2, ignore_errors=True)

    print("self-test:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def main(argv: "list[str]" = ()) -> int:
    if "--self-test" in argv:
        return self_test()
    if not CATALOGS.exists():
        print("PASS i18n-catalog-integrity (vacuous -- i18n/locales/ absent)")
        return 0
    a = probe_a()
    b = probe_b()
    c, d, e = probe_cde()
    n_files = len(list(CATALOGS.glob("*/*.json")))
    total = len(a) + len(b) + len(c) + len(d) + len(e)
    verdict = "PASS" if total == 0 else f"FAIL {total}"
    print(f"{verdict} i18n-catalog-integrity")
    print(f"  Probe A (generated tables == i18n/locales.json): {'PASS' if not a else 'FAIL'}")
    for line in a:
        print(f"    {line}")
    print(f"  Probe B ({n_files} catalog files parse + _meta): {'PASS' if not b else 'FAIL'}")
    for line in b:
        print(f"    {line}")
    print(f"  Probe C (keys ⊆ en + placeholder equality): {'PASS' if not c else 'FAIL'}")
    for line in c:
        print(f"    {line}")
    print(f"  Probe D (CLDR plural categories): {'PASS' if not d else 'FAIL'}")
    for line in d:
        print(f"    {line}")
    print(f"  Probe E (mojibake / entities): {'PASS' if not e else 'FAIL'}")
    for line in e:
        print(f"    {line}")
    return 0 if total == 0 else 1


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Exception as exc:  # noqa: BLE001 -- environment failure is exit 2 by contract
        print(f"FAIL i18n-catalog-integrity (environment: {exc})")
        sys.exit(2)
