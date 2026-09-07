# widget-bundle-size

**Kind**: Verification Oracle (Probandurgy — Task Zero leg 2 of three-leg defense).

**Question**: Is the **English page-load set** of the built feedbackmonk widget
(TOP-LEVEL `widget/dist/*.{js,mjs,css}`) at most 30720 bytes (30 KiB; FR-FBR-04
cap), is every per-locale catalog chunk (`widget/dist/locales/*.js`) at most
4096 bytes, and does the built tree contain zero canonical third-party tracker
hostnames anywhere (DEC-FBR-02 brand promise)? Has the canonical tracker-list
drifted from its hashed baseline?

## Synopsis

Verification Oracle (P2 Task Zero; amended for FR-FBR-35 localization) defending two widget brand promises as code-level invariants: the English page-load set (top-level `widget/dist/*.{js,mjs,css}`) is ≤30720 bytes (30 KiB, FR-FBR-04), each lazily-loaded locale chunk under `dist/locales/` is ≤4096 bytes on its own (Contract C42), and the whole built tree contains zero canonical third-party tracker hostnames (DEC-FBR-02), with the tracker list pinned to a hashed baseline. Re-run after any widget build, catalog change or dependency change.

## Probes

### Probe A — English page-load set size

Sums the byte count of every **top-level** file in `widget/dist/` matching
`*.{js,mjs,css}` — `widget.js` + `widget.css` + `redact.js` — post-minification,
post-terser, **pre-gzip** (the cap is on the wire-format bytes the browser must
download in the worst case). Cap is `SIZE_CAP_BYTES = 30 * 1024 = 30720`.
Over → FAIL with per-file size breakdown + overage.

**Subdirectories are excluded deliberately** (`glob`, not `rglob`) — see
*Why the measured set was redefined* below.

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

### Probe C — Per-locale chunk size

Each `widget/dist/locales/*.js` is measured **individually** against
`LOCALE_CHUNK_CAP_BYTES = 4 * 1024 = 4096`. One chunk holds ~59 short strings
for one locale (Contract C42); 4 KiB is roughly twice the largest plausible
translated widget catalog, so a chunk that trips this is carrying something
that is not strings — leaked code, a non-`widget` namespace, duplicated keys —
and the fix is upstream in `widget/scripts/slice-locales.mjs`. Never silently
raise the constant.

No `dist/locales/` directory (or no chunks in it) → **vacuous PASS**, same
cold-start reasoning as Probe A.

## Why the measured set was redefined (FR-FBR-35, 2026-09-06)

Before localization, `dist/` held only the English page-load set, so "sum
everything under `dist/`" and "the bytes an English visitor downloads" were the
same number. With 31 locales they are not:

- a visitor fetches `widget.js` + `widget.css` + (on first redact) `redact.js`,
  and **at most one** `locales/<code>.js`;
- an English visitor fetches **no** locale chunk at all — `en` is inlined.

Summing all 31 into one 30 KiB total would therefore measure a page load nobody
performs, and — worse — would let a single oversized chunk hide inside the
total while the aggregate still passed.

**The cap itself was not touched.** `SIZE_CAP_BYTES` is the same 30720 it has
always been; what changed is the SET it is applied to, and Probe C adds a
second ceiling that did not exist before. This is a tightening: a 20 KB locale
chunk passed the old aggregate check whenever the rest of the bundle was small
enough, and fails now.

Both constants carry a "never silently raise" comment in `oracle.py` for the
same reason: a cap that moves when it is inconvenient is not an invariant.

## Three-leg defense (per D-FBR-02 pattern)

| Leg | Mechanism | File / location |
|---|---|---|
| 1. Bundler chokepoint | `widget/vite.config.ts` — terser + CSP-safe (no `eval`, no `Function()`, no `inline-script`); no third-party SDK imports declared in `package.json` | `widget/vite.config.ts`, `widget/package.json` |
| 2. AST / artifact oracle (this file) | Probe A (size) + Probe B (tracker scan) + list-hash drift | `.claude/oracles/widget-bundle-size/` |
| 3. Runtime a11y harness | Playwright + `@axe-core/playwright` integration test; will surface a behavioural regression if a tracker were to load dynamically at runtime (network requests visible in Playwright) | `widget/e2e/widget-a11y.spec.ts`, `widget/e2e/widget-locale.spec.ts` (locale matrix + the `script-src 'self'` fixture, which is also where "the chunk actually loads" is proven — this oracle only sees file sizes) |

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
  Probe A (English page-load set, top-level widget/dist/*.{js,mjs,css} <= 30720B): clean (<USED>B used, <HEADROOM>B headroom across <N> file(s))
    widget/dist/redact.js  <SIZE>B
    widget/dist/widget.css  <SIZE>B
    widget/dist/widget.js  <SIZE>B
  Probe B (no canonical tracker hostnames in widget/dist, recursive): clean
  Probe C (each locale chunk <= 4096B): clean (<N> chunk(s), largest <FILE> at <SIZE>B, <HEADROOM>B headroom)
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

Probe C failure (per-locale catalog chunk exceeds 4096B cap per Contract C42):
  widget/dist/locales/de.js  5023B  over_by=927B (cap=4096B)
  Remediation: a locale chunk is a flat map of ~40 short strings and nothing else. Check widget/scripts/slice-locales.mjs for leaked code, a non-widget namespace or duplicated keys. Never silently raise LOCALE_CHUNK_CAP_BYTES.
```

Cold-start (no `widget/dist/`):

```
PASS widget-bundle-size
  tracker-list hash: <sha256-hex> (<N> hostnames)
  Probe A (English page-load set <= 30720B): vacuous PASS — widget/dist does not exist yet (pre-build / cold-start)
  Probe B (no tracker hostnames): vacuous PASS — no built files to scan
  Probe C (each locale chunk <= 4096B): vacuous PASS — no built files to scan
```

## Adversarial self-test (v1.1.0, recorded 2026-09-06)

A passing oracle proves nothing until it has been made to fail on purpose.
What was run, against a real build (`cd widget && npm run build`):

| Injected defect | Expected | Observed |
|---|---|---|
| 5,023 B `widget/dist/locales/de.js` written by hand | Probe C RED | `FAIL … over_by=927B (cap=4096B)`, exit 1 |
| that file removed | back to PASS | `PASS`, exit 0 |
| a 66 B `de.js` chunk | PASS, chunk reported | `Probe C … 1 chunk(s), largest widget/dist/locales/de.js at 66B` |
| `mixpanel.com` planted inside `dist/locales/de.js` | Probe B RED — the Probe A split must NOT have made Probe B stop recursing | `FAIL … widget/dist/locales/de.js:1 hostname='mixpanel.com'`, exit 1 |
| the same 5 KB chunk present while Probe A measured | Probe A unchanged at 23,250 B | unchanged — the chunk is correctly outside the page-load set |

The fourth row is the one worth keeping: splitting Probe A onto a non-recursive
glob is exactly the kind of change that silently narrows a *neighbouring* probe,
and nothing else in the suite would have caught it.

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
- **Probe A measures the ENGLISH PAGE-LOAD SET, not everything under `dist/`**
  (FR-FBR-35 amendment, v1.1.0). Rationale in full above. The short version:
  once `dist/` contains 31 mutually-exclusive locale chunks, a recursive sum
  measures a page load nobody performs. `SIZE_CAP_BYTES` was NOT changed.
- **Locale chunks get their own per-file cap (Probe C) rather than a share of
  the aggregate.** An aggregate would couple 31 independent artifacts: adding a
  32nd language would eat the widget's code budget, and one oversized chunk
  could hide inside a small total. A per-chunk ceiling says the thing that is
  actually true — *whatever locale you read this in, you download at most 30 KiB
  plus at most 4 KiB.*
- **4096 B for a chunk**: a translated widget catalog is ~59 short strings,
  measured at ~2 KB for a verbose language. 4 KiB is roughly 2x that — loose
  enough never to fire on honest translation, tight enough that code or a
  second namespace leaking into a chunk fires it immediately.
- **Probe B still recurses.** The Probe A split narrowed ONE probe on purpose;
  the tracker scan must keep seeing every built file, locale chunks included,
  and there is an adversarial self-test row above whose only job is to keep that
  true.
