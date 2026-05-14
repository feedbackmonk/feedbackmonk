# widget-bundle-size

**Kind**: Verification Oracle (Probandurgy — Task Zero leg 2 of three-leg defense).

**Question**: Is the built feedbackmonk widget bundle (`widget/dist/*.{js,mjs,css}`)
at most 30720 bytes (30 KiB; FR-FBR-04 cap), and does it contain zero canonical
third-party tracker hostnames (DEC-FBR-02 brand promise)? Has the canonical
tracker-list drifted from its hashed baseline?

## Probes

### Probe A — Bundle size

Walks `widget/dist/` recursively, sums the byte count of every file matching
`*.{js,mjs,css}` (post-minification, post-terser, **pre-gzip** — the cap is
on the wire-format bytes the browser must download in the worst case). Cap
is `SIZE_CAP_BYTES = 30 * 1024 = 30720`. Over → FAIL with per-file size
breakdown + overage.

Cold-start (no `widget/dist/` yet) emits **vacuous PASS**: 0 files = 0 bytes
≤ cap. This is the load-bearing property that lets the oracle ship BEFORE
the widget source, then re-evaluate on every subsequent build.

### Probe B — No canonical third-party tracker hostnames

Reads `expected-trackers.txt`, parses to a list of hostnames (stripping
`#` comments + blank lines, lowercasing). For every file in `widget/dist/`
matching the bundle extensions, scans each line for any hostname; case
folded. Any hit = FAIL with `(hostname, file:line)` offender list.

Defends DEC-FBR-02 ("no third-party trackers in the widget, ever") as a
code-level invariant. The check is on the **built artifact**, not the
source — so even if a tracker is dynamically pulled in via an indirect
import, the bundler's minification surface still contains the hostname
string and the probe catches it.

### List-hash drift detection (Q5)

The canonical form of `expected-trackers.txt` is:
1. Strip `#` comments and blank lines.
2. Lowercase + strip each remaining line.
3. Sort.
4. Join with `\n`, UTF-8 encode.

The SHA-256 of that canonical form is **printed in every oracle report**
(both PASS and FAIL). If someone silently shrinks the list, the hash
changes and the diff surfaces in every subsequent commit's oracle output.
The list itself is `git`-tracked, so the canonical record lives in
version control — the hash is the second leg.

## Three-leg defense (per D-FBR-02 pattern)

| Leg | Mechanism | File / location |
|---|---|---|
| 1. Bundler chokepoint | `widget/vite.config.ts` — terser + CSP-safe (no `eval`, no `Function()`, no `inline-script`); no third-party SDK imports declared in `package.json` | `widget/vite.config.ts`, `widget/package.json` |
| 2. AST / artifact oracle (this file) | Probe A (size) + Probe B (tracker scan) + list-hash drift | `.claude/oracles/widget-bundle-size/` |
| 3. Runtime a11y harness | Playwright + `@axe-core/playwright` integration test; will surface a behavioural regression if a tracker were to load dynamically at runtime (network requests visible in Playwright) | `widget/e2e/widget-a11y.spec.ts` |

## Invocation

```bash
# Unix / Git Bash on Windows / WSL
bash .claude/oracles/widget-bundle-size/oracle.sh

# Windows (PowerShell)
pwsh .claude/oracles/widget-bundle-size/oracle.ps1

# Or Python directly (cross-platform)
python .claude/oracles/widget-bundle-size/oracle.py
```

Exit `0` on PASS, `1` on FAIL, `2` on environment failure (Python not found).

## Output schema

```
PASS widget-bundle-size
  tracker-list hash: <sha256-hex> (<N> hostnames)
  Probe A (size <= 30720B): clean (<USED>B used, <HEADROOM>B headroom across <N> file(s))
    widget/dist/widget.js  <SIZE>B
    widget/dist/widget.css  <SIZE>B
  Probe B (no canonical tracker hostnames in widget/dist): clean
```

or

```
FAIL widget-bundle-size (<N> probe(s) failed)
  tracker-list hash: <sha256-hex> (<N> hostnames)

Probe A failure (bundle exceeds 30720B / 30KiB cap per FR-FBR-04):
  current_size=<X>B  cap=30720B  over_by=<Y>B
    widget/dist/widget.js  <SIZE>B
    ...
  Remediation: drop a feature or aggressive-minify before re-running. Never silently raise SIZE_CAP_BYTES.

Probe B failure (canonical third-party tracker hostname in built bundle — DEC-FBR-02 brand promise violation):
  widget/dist/widget.js:42  hostname='segment.io' (canonical-tracker; not permitted in widget bundle)
  ...
  Remediation: remove the offending import / script-src / fetch URL. feedbackmonk's widget calls home ONLY to feedbackmonk's own backend.
```

Cold-start (no `widget/dist/`):

```
PASS widget-bundle-size
  tracker-list hash: <sha256-hex> (<N> hostnames)
  Probe A (size <= 30720B): vacuous PASS — widget/dist does not exist yet (pre-build / cold-start)
  Probe B (no tracker hostnames): vacuous PASS — no built files to scan
```

## Editing the tracker list

The `expected-trackers.txt` file is **additive-only** per GUIDE.md §8
(CLAUDE-A's pre-authorized widenings). Additions are permitted; removals
require LD ratification via `channels/alerts.md`.

Workflow for adding a tracker:
1. Append the new hostname to `expected-trackers.txt` (one per line,
   sorted within a logical grouping if applicable; the canonical form
   is whole-list sorted at hash time so insertion order doesn't matter).
2. Re-run the oracle — the printed `tracker-list hash` changes. That
   change is the reviewable artifact.
3. Tag the commit (or a follow-up `channels/decisions.md` entry) with
   `self_mediated=true; ratification_pending=true; matches_spec_at=docs/planning/plans/20260514T034730-feedbackmonk-p2-customer-facing.md#oracle-pre-build-plan`.

## Why the size cap is reified here (vs. in CI alone)

Per arc-plan Testability Gate Q3=4 finding: without a deterministic
verifier reachable from the inner develop/test/fix loop, an agent will
accept "looks small enough" and ship over-budget. The oracle is the
inner-loop closer. CI is the outer-loop redundancy.

## Lineage

- **FR-FBR-04** — Embeddable widget, <30KB bundle
- **DEC-FBR-02** — Brand promise: no third-party trackers in the widget, ever
- **DEC-FBR-IMPL-03** — Python-canonical oracle implementations
- **P2 plan §Oracle Pre-Build Plan** — Probe A + Probe B + drift-detection contract
- **P2 plan §Testability Gate** — composite-11 (Q3=4 critical-path) finding that drove Task Zero scheduling
- **Three-leg defense pattern (D-FBR-02)** — type/bundler chokepoint + oracle + lint/behavioural-test

## Decision log

- **File-naming**: `oracle.{py,sh,ps1}` (not brief's `run.*`). Rationale:
  consistency with existing sibling oracles (`pii-scrub-audit/oracle.py`,
  `multi-tenant-isolation-check/oracle.py`). Brief's `run.*` was a generic
  template placeholder; the project's established convention wins.
- **Cap value `30 * 1024 = 30720`**: KiB, not metric KB. FR-FBR-04 says
  "<30KB" — the conservative interpretation is binary kibibytes (what
  bundlers report; what `ls -la` reports). 30000 would be tighter but
  outside the spec's expressed intent.
- **Bundle extensions `.js`/`.mjs`/`.css`**: covers ESM + CommonJS + style
  outputs from vite. SourceMaps (`.map`) are excluded — they're dev-only
  and not shipped to embedders.
- **`pre-gzip` size, not `post-gzip`**: the cap is the wire-format ceiling
  in the worst case (an embedder serving with `Content-Encoding: identity`).
  If gzip is on, the actual transfer is smaller. Defending pre-gzip is
  the conservative invariant.
- **Tracker list canonical form (sort + lowercase + `\n`-join)**: stable
  serialisation so cosmetic edits (reordering, casing) don't churn the
  hash. The hash is the drift defender, not a strict-format defender.
- **Hostname match is `substring + case-insensitive`**: catches both
  `https://segment.io/sdk.js` and `'//cdn.segment.io/'` patterns. False
  positives are theoretically possible (a customer slug literally named
  `segment.io`) but acceptable — the file `widget/dist/*` is bundled
  output, not customer data.
- **Cold-start vacuous PASS**: load-bearing. Lets the oracle land
  BEFORE `widget/dist/` exists, satisfying Task Zero's order-of-operations
  invariant.
