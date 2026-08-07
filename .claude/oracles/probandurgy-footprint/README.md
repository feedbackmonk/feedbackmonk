# probandurgy-footprint Oracle

## Synopsis

Verification Oracle (`kind: "verification"` per Oraculurgy Part 11) for the **Probandurgy Footprint Discipline** (cross-cutting invariant; SPECIFICATION.md § FOOTPRINT-01..08; DEC-62/63; DISC-FOOTPRINT-01). Reads a per-project manifest of declared Probandurgy mechanisms (ARIA, Agent UI Fixtures, etc.) and verifies that each declared compile-time-presence gate is wired in the project's build config (Cargo feature, Vite/Webpack/Rollup define, Dockerfile ARG). Graceful-absent when no manifest exists — silent on projects that haven't adopted the discipline. Phase 1 is **declared-vs-implemented** only; it does NOT inspect built artifacts.

> **Category**: probandurgy | **Kind**: verification | **Strategy**: trigger-invalidate

The oracle answers: *"For each declared Probandurgy mechanism in this project's manifest, is the declared compile-time-presence footprint enforced by an active gate in the build configuration?"*

## Why this oracle exists

Probandurgy mechanisms with binary-bleed risk (ARIA's Rust crate + Vite-bundled JS surfaces; mounted Agent UI Fixture routes) are scaffolding for agentic development — they ship OUT of release builds by default. SessionHelm's ARIA-08 implementation (Cargo `aria-dev` feature + Vite `__ARIA_ENABLED__` define + 14-signature `verify:aria-excluded` script with `--self-test` invert mode) is the reference pattern. WinLocksmith hardening on 2026-05-17 surfaced the same gap from scratch (DISC-FOOTPRINT-01), triggering promotion to a framework-level discipline.

This oracle is the **cheap declared-vs-implemented audit** that catches the gap at the manifest level before it becomes a binary leak. It does NOT replace the project-side `verify-<name>-excluded` script — that script is the artifact-level verifier; this oracle confirms the declaration matches the gate, and that the verifier itself exists.

## Phase 1 vs Phase 2 boundary (READ THIS)

**Phase 1 (this implementation)** verifies that gates EXIST in build config files. It parses the manifest's `gate_locations[]` entries (e.g. `Cargo.toml#features.aria-dev`) and confirms each named directive is present.

**Phase 1 does NOT** crack open compiled artifacts (no `strings`, no `nm`, no AST inspection, no Docker layer scan). A Phase 1 PASS does NOT guarantee artifact-level absence of mechanism strings — only that the project has declared and wired the gate. The project-side `verify-<name>-excluded` script (referenced by `verifier_script` in the manifest) is responsible for artifact-level verification.

**Phase 2** (deferred per FOOTPRINT-04) layers in per-language artifact inspection as patterns emerge. The frozen output schema (below) is additive-only: Phase 2 will add fields but never rename or remove.

## Manifest schema (IC-1)

Per-project manifest lives at `.claude/manifests/probandurgy-footprint.json` (default). Override via the `PROBANDURGY_FOOTPRINT_MANIFEST` environment variable.

```json
{
  "schemaVersion": "1",
  "mechanisms": [
    {
      "name": "<mechanism slug, e.g., aria | agent-ui-fixture>",
      "axis_a": "<in-binary | feature-gated-out-by-default | not-applicable>",
      "axis_b": "<on-by-default | runtime-gated | not-applicable>",
      "gate_locations": [
        "<file>#<anchor>"
      ],
      "verifier_script": "<path or null>",
      "self_test_supported": true,
      "waivers": [
        {
          "surface": "<name>",
          "rationale": "<path-to-rationale-doc>",
          "review_due": "<YYYY-MM-DD>"
        }
      ]
    }
  ]
}
```

### Field semantics

| Field | Meaning |
|-------|---------|
| `schemaVersion` | String `"1"` at Phase 1. Additive-only changes bump to `"2"`. |
| `mechanisms[]` | Array of declared mechanism instances. May be empty. |
| `name` | Slug identifying the mechanism instance (e.g. `aria`). Free-form; correlates with PROBANDURGY_MECHANISMS.md catalog entries by convention. |
| `axis_a` | Compile-time-presence axis (FOOTPRINT-01). Mirrors the IC-3 catalog row `axis-a` field. Binary-bleed-risk mechanisms default to `feature-gated-out-by-default`. |
| `axis_b` | Runtime-activation axis. Mirrors IC-3 `axis-b`. Orthogonal to `axis_a`. |
| `gate_locations[]` | `<file>#<anchor>` pointers. See parser support matrix below. |
| `verifier_script` | Path to a project-side `verify-<name>-excluded` script. Optional (`null`). Oracle records path-exists as `self_test_verifier_present`; does NOT invoke. |
| `self_test_supported` | Boolean. `true` if `verifier_script` accepts a `--self-test` invert flag (deliberately leaks the surface to confirm the verifier fires). |
| `waivers[]` | Per-surface exemptions per FOOTPRINT-06. Each requires `surface`, `rationale` (path), `review_due` (ISO date). |

### Per-extension parser support matrix

Phase 1 supports these `<file>#<anchor>` shapes. Unsupported anchors emit `gate-location-not-found` violations rather than crashing.

| File pattern | Anchor pattern | Detects |
|--------------|----------------|---------|
| `Cargo.toml` | `features.<NAME>` | `[features]` table key |
| `vite.config.{ts,js,mjs,cjs}` | `define.<NAME>` | `define: { __NAME__: ... }` key |
| `webpack.config.{ts,js}` | `DefinePlugin.<NAME>` | `new DefinePlugin({ __NAME__: ... })` key |
| `rollup.config.{ts,js,mjs}` | `replace.<NAME>` or `define.<NAME>` | `@rollup/plugin-replace` or define key |
| `Dockerfile` | `ARG.<NAME>` | `ARG <NAME>` directive |

JS parsing is a regex-with-balanced-braces heuristic, not a full AST. False negatives on unusual JS forms (computed property names, spread expressions, ternary-wrapped defines) are acceptable Phase 1 limits — they emit `declared-but-no-gate` violations the user can investigate. Adding a new parser: edit `run.py` and update this matrix.

## Output schema (FOOTPRINT-04 frozen)

```json
{
  "schemaVersion": "1",
  "manifest_present": true,
  "mechanisms_declared": [
    {
      "mechanism": "aria",
      "declared_axis_a": "feature-gated-out-by-default",
      "declared_axis_b": "runtime-gated",
      "gate_implementation_found": true,
      "gate_locations": ["Cargo.toml#features.aria-dev", "vite.config.ts#define.__ARIA_ENABLED__"],
      "self_test_verifier_present": true,
      "waivers": []
    }
  ],
  "violations": [
    {
      "mechanism": "aria",
      "kind": "declared-but-no-gate",
      "evidence": "Cargo.toml#features.aria-dev",
      "remediation": "Add the gate stanza named in 'Cargo.toml#features.aria-dev' to 'Cargo.toml', or remove the gate_locations[] entry from the manifest if the gate is no longer required."
    }
  ],
  "advisory": true
}
```

Graceful-absent (no manifest present):

```json
{"schemaVersion":"1","manifest_present":false,"mechanisms_declared":[],"violations":[],"advisory":true}
```

### Violation kinds

| `kind` | Meaning |
|--------|---------|
| `declared-but-no-gate` | Manifest declares a `gate_locations[]` entry whose anchor was not found in the cited file. |
| `gate-location-not-found` | Cited file does not exist, anchor format is invalid, or extension is unsupported. |
| `waiver-without-rationale-file` | A `waivers[]` entry's `rationale` path does not resolve. Per FOOTPRINT-06, waivers without rationale are not real waivers. |
| `manifest-malformed` | Manifest file exists but JSON does not parse. Oracle does not crash; surfaces the manifest path as evidence. |
| `unsupported-anchor` | Reserved for Phase 2; not currently emitted. |

### Schema stability contract

The output schema is **frozen** per FOOTPRINT-04. Phase 2+ additions are **additive only**: new fields may appear, but no existing field is renamed, removed, or has its semantics changed. Programmatic consumers (e.g. `/0-uldf-finalize` Phase 11 oracle audit) depend on this stability.

## Freshness contract

`trigger-invalidate` — re-run when the manifest or any cited build-config file mutates. Trigger list in `oracle.json#freshness.triggers`. Compute cost: ~300ms on a typical project.

## Wiring posture

`/0-uldf-finalize` Phase 11 auto-detects oracles in `.claude/oracles/`; no explicit registration is required. Per FOOTPRINT-04:

- **autopilot**: advisory-only (summary appears in Phase 11 oracle output; does not halt).
- **controlled or below**: halts when `violations[]` is non-empty.

This aligns with DEC-12 Halt Principle conservatism for first-ship Verification Oracles. The posture is declared in `oracle.json#wiring.finalize_phase_11_posture`.

## Invocation

```bash
# Unix / Git Bash
bash .claude/oracles/probandurgy-footprint/run.sh

# Windows PowerShell
powershell -NoProfile -File .claude/oracles/probandurgy-footprint/run.ps1
```

Both wrappers exec the shared `run.py` implementation — output is byte-identical for the same project state (modulo line-ending normalization on Windows).

### Self-test

```bash
bash .claude/oracles/probandurgy-footprint/validate.sh
# OR
powershell -NoProfile -File .claude/oracles/probandurgy-footprint/validate.ps1
```

Both iterate `test-fixtures/<name>/`, run the oracle in each, and diff against `expected-output.json`. Exit 0 iff all fixtures pass.

## Test fixtures

| Fixture | Purpose | Expected verdict |
|---------|---------|------------------|
| `no-manifest-graceful-absent/` | Empty fixture (no manifest) | Graceful-absent shape; exit 0 |
| `aria-declared-and-gated/` | SessionHelm-modeled clean case (Cargo feature + Vite define both present) | Clean: `gate_implementation_found: true`, no violations |
| `aria-declared-but-no-gate/` | Cargo feature deliberately missing | One `declared-but-no-gate` violation |
| `waiver-without-rationale/` | Waiver with non-existent rationale path | One `waiver-without-rationale-file` violation |
| `malformed-manifest/` | JSON syntax error in manifest | One `manifest-malformed` violation; oracle does not crash |

## Adding new manifest entries (for adopters)

1. Add `.claude/manifests/probandurgy-footprint.json` to your project (or extend an existing one).
2. Declare a `mechanisms[]` entry per the IC-1 schema above. Use `axis_a: "feature-gated-out-by-default"` for any mechanism that compiles into shipped artifacts.
3. For each gate, add a `<file>#<anchor>` pointer to `gate_locations[]`. Confirm the parser support matrix covers your build system (or add a new parser to `run.py`).
4. Author the project-side `verify-<name>-excluded` script (separate work — see SessionHelm ARIA-08 for reference; W5's `/0-uldf-setup-project` template emits a starting point).
5. Run `bash .claude/oracles/probandurgy-footprint/run.sh` to confirm the declaration matches the gate. Iterate until `violations[]` is empty.
6. For any side surface that legitimately needs to ship (per FOOTPRINT-06: prefer Extract; Waive only when impractical), add a `waivers[]` entry with a rationale doc and a `review_due` date.

## Fallback (graceful absence)

If this oracle is missing or broken, an adopter can manually grep their build config for the gates declared in `.claude/manifests/probandurgy-footprint.json`:

```bash
# Cargo feature presence
grep -A 20 '^\[features\]' Cargo.toml | grep 'aria-dev'

# Vite define presence
grep '__ARIA_ENABLED__' vite.config.ts

# Verifier script existence
test -f scripts/verify-aria-excluded.sh
```

Slow and error-prone vs. the oracle, but the discipline is enforceable by hand.

## Cross-references

- **Spec**: SPECIFICATION.md § FOOTPRINT-04 (frozen output schema, wiring posture); § FOOTPRINT-01 (two-axis declaration); § FOOTPRINT-06 (waiver rules)
- **Decisions**: DECISIONS.md DEC-62 (placement: cross-cutting invariant, not ninth mechanism), DEC-63 (scope: Probandurgy-narrow)
- **Discovery**: DISCOVERIES.md DISC-FOOTPRINT-01 (SessionHelm + WinLocksmith trigger incidents)
- **Cross-cutting invariant prose** (W2-owned): `FOUNDATIONS/PROBANDURGY_MECHANISMS.md#cross-cutting-invariant-probandurgy-footprint-discipline`
- **Catalog row format** (IC-3, W2-owned): same anchor
- **Oraculurgy Part 11**: `FOUNDATIONS/ORACULURGY_DESIGN.md` § Verification Oracles
- **Reference implementation**: SessionHelm ARIA-08 (Cargo `aria-dev` + Vite `__ARIA_ENABLED__` + 14-signature `verify:aria-excluded` with `--self-test`)
- **Scaffolding template** (W5-owned, FOOTPRINT-05): `~/.claude/skills/0-uldf-setup-project/` emits manifest skeleton per IC-1
