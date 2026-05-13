# `ui-surface-detector` Oracle

## Synopsis

Project-state oracle that classifies the project's runtime surface (`tauri-desktop`, `electron-desktop`, `web-spa`, `react-native`, `flutter`, `mobile-native`, `backend-service`, `cli-tool`, or `none`) by reading manifests and marker paths — never executing project scripts. Consumed by `aria-status` (Leg A), `/0-uldf-ldis-intake` Phase 1 risk profiling (Leg B), and `/0-uldf-ldis-plan` Phase 4 testability gate Q4 to decide whether an ARIA Stage 0 instrumentation task should be auto-proposed. Don't come here for live ARIA endpoint health — that's the `aria-status` sibling — or for project type / language / build system classification, which is the `project-type` starter oracle.

## Purpose

Answers *"Does this project have a UI / runtime surface that ARIA could instrument, and what kind?"* by reading the project's manifests and a small set of marker paths.

This oracle is the **surface-detection helper** consumed by:

- `aria-status` oracle (ARIA-01) — uses `surface_kind` to decide whether to probe ARIA endpoints
- `/0-uldf-ldis-intake` Phase 1 risk profiling (Leg B of ARIA's four-leg detection machinery) — broadens "is this UI work?" to "does this need runtime perception?"
- `/0-uldf-ldis-plan` Phase 4 testability gate Q4 — auto-proposes a Stage 0 ARIA instrumentation task when a non-`none` surface is detected on a project without ARIA foundation

Spec: ARIA-02 in `docs/specs/SPECIFICATION.md`.

## Output

```json
{
  "surface_kind": "tauri-desktop | electron-desktop | web-spa | react-native | flutter | mobile-native | backend-service | cli-tool | none",
  "confidence": "high | medium | low",
  "evidence": ["package.json declares 'react' dependency", "index.html present"]
}
```

## Detection Rules (per ARIA-02 acceptance #1)

| Markers | Surface | Confidence rule |
|---------|---------|-----------------|
| `src-tauri/` directory + `Cargo.toml` | `tauri-desktop` | high (≥2 evidence) |
| `package.json` declares `electron` | `electron-desktop` | high if 2+ evidence; else medium |
| `package.json` declares `react-native` or `expo` | `react-native` | high if both; else medium |
| `pubspec.yaml` declares Flutter SDK | `flutter` | medium (single marker) |
| `package.json` UI framework dep (react/vue/svelte/angular/preact/solid-js/lit) **AND** `index.html` | `web-spa` | high (≥2 evidence) |
| `package.json` backend dep (express/fastify/hono/koa/nest/restify) **AND** no UI dep | `backend-service` | medium |
| `bin/` directory or `package.json` `bin` field or Cargo `[[bin]]` (no UI/desktop/mobile/backend match) | `cli-tool` | medium |
| None of the above | `none` | high (definitively no surface) |

When **multiple** kinds match (rare, e.g., Tauri + Electron both declared), `confidence: "low"` with all candidates listed in `evidence`.

## Invocation

```bash
# Unix
bash .claude/oracles/ui-surface-detector/run.sh

# Windows
powershell -NoProfile -File .claude/oracles/ui-surface-detector/run.ps1
```

## Caching

Output is cached at `.claude/oracle-cache/ui-surface-detector.json`. Cache is invalidated when any of `package.json`, `Cargo.toml`, or `pubspec.yaml` is modified after the cache file (mtime comparison). Detection is filesystem-stat + small-file reads only — never executes project scripts. Bound: ≤200ms.

## Behavior on Edge Cases

- **No manifest files**: returns `surface_kind: "none"`, `confidence: "high"`, `evidence: ["no UI/runtime surface markers detected"]`.
- **Malformed JSON in `package.json`**: dependency probes return false; `surface_kind` falls back to `none` or to `cli-tool` if `bin/` is present.
- **Cache corruption**: oracle silently recomputes; never throws.

## Validation

```bash
bash .claude/oracles/ui-surface-detector/validate.sh
powershell -NoProfile -File .claude/oracles/ui-surface-detector/validate.ps1
```

Validators check JSON well-formedness, required fields, and that `surface_kind` and `confidence` are within the enumerated value sets.

## Integration with ARIA's Four-Leg Detection

This oracle is part of **Leg B (planning analysis)** alongside the `aria-status` oracle (ARIA-01) and the `/0-uldf-ldis-intake`/`/0-uldf-ldis-plan` widening (ARIA-04). The four legs are:

| Leg | When it fires | Mechanism |
|-----|---------------|-----------|
| **A** — Predictive | Session start | `[aria-status]` ORACLE BRIEFING line |
| **B** — Planning | `/0-uldf-ldis-intake` Phase 1, `/0-uldf-ldis-plan` Phase 4 | This oracle + `aria-status` |
| **C** — Inner-loop reflex | Before agent asks human a runtime question | CLAUDE.md standing instruction (Phase 3) |
| **D** — Audit | `/0-uldf-finalize` Phase 11.5 | `runtime-perception-questions` oracle (Phase 2) |

## Related

- Spec: ARIA-02 in `docs/specs/SPECIFICATION.md`
- Design: `FOUNDATIONS/ARIA_INTEGRATION_DESIGN.md` § 3 (architecture)
- Sibling oracle: `aria-status` (ARIA-01) — consumes this oracle's output
- Probandurgy: this oracle helps detect the absent-ARIA failure mode (PROBANDURGY_MECHANISMS.md, runtime-perception leg)
