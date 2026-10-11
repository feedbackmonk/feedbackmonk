# widget-bundle-size

**Kind**: Verification Oracle (Probandurgy — Task Zero leg 2 of three-leg defense).

**Question**: Is the built feedbackmonk widget present, free of every canonical third-party
tracker hostname (DEC-FBR-02 brand promise), and is every per-locale catalog chunk
(`widget/dist/locales/*.js`) only a flat map of `widget.*` strings (Contract C42)? Has the
canonical tracker list drifted from its hashed baseline? And what does the widget weigh — the
**English page-load set** (TOP-LEVEL `widget/dist/*.{js,mjs,css}`) and each locale chunk — which
is reported on every run and judged by no threshold (DEC-FBR-IMPL-36).

## Synopsis

Verification Oracle (P2 Task Zero; amended for FR-FBR-35 localization; size cap retired by
DEC-FBR-IMPL-36) defending the widget's no-tracker brand promise and the shape of its locale
chunks as code-level invariants, with the tracker list pinned to a hashed baseline, and printing
the widget's size so growth is visible in every check. Re-run after any widget build, catalog
change or dependency change.

## What it checks

### Size — reported, never a failure (DEC-FBR-IMPL-36, 2026-10-10)

Prints the byte count of every **top-level** file in `widget/dist/` matching `*.{js,mjs,css}` —
`widget.js` + `widget.css` + `redact.js`, post-terser, pre-gzip — with their total, and the count
and largest of the locale chunks under `dist/locales/` (a visitor loads at most one). No number
fails the run. Until 2026-10-10 this was "Probe A", a hard cap: 30,720 B from P2, raised to
40,960 B by DEC-FBR-IMPL-35 that morning, and removed the same day on the owner's word because it
had entered as one unexplained cell of DEC-FBR-08's scope table and its only measured effect was to
stall a feature. The number stays in front of whoever changes the widget.

A missing `widget/dist/widget.js` is a FAIL: the widget exists, so a commit without its build is
broken (until 2026-09-30 this was a cold-start vacuous PASS).

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

### Chunk shape — each locale chunk is only strings

Each `widget/dist/locales/*.js` must be exactly the bundler's form of a catalog —
`const <id> = {...}; export { <id> as default };` — whose object parses as JSON with every key in
the `widget.*` namespace and every value a string (Contract C42). Anything else — code, a second
namespace, a non-string value — fails, naming the chunk and why; the fix is upstream in
`widget/scripts/slice-locales.mjs`. This replaced the 4 KiB per-chunk size cap (DEC-FBR-IMPL-36),
which stood in for exactly this and could not tell a small chunk of code from a long honest
translation.

No chunks in `dist/locales/` while `i18n/locales/` holds non-English locales → FAIL (formerly a
vacuous PASS).

## Why the measured set is the English page-load set (FR-FBR-35, 2026-09-06)

Before localization, `dist/` held only the English page-load set, so "sum everything under `dist/`"
and "the bytes an English visitor downloads" were the same number. With 31 locales they are not: a
visitor fetches `widget.js` + `widget.css` + (on first redact) `redact.js`, and **at most one**
`locales/<code>.js`; an English visitor fetches no locale chunk at all — `en` is inlined. So the
reported total is that set, and the chunks are reported on their own.

## Three-leg defense (per D-FBR-02 pattern)

| Leg | Mechanism | File / location |
|---|---|---|
| 1. Bundler chokepoint | `widget/vite.config.ts` — terser + CSP-safe (no `eval`, no `Function()`, no `inline-script`); no third-party SDK imports declared in `package.json` | `widget/vite.config.ts`, `widget/package.json` |
| 2. AST / artifact oracle (this file) | tracker scan + list-hash drift + chunk shape, size reported | `.claude/project-oracles/widget-bundle-size/` |
| 3. Runtime a11y harness | Playwright + `@axe-core/playwright` integration test; will surface a behavioural regression if a tracker were to load dynamically at runtime (network requests visible in Playwright) | `widget/e2e/widget-a11y.spec.ts`, `widget/e2e/widget-locale.spec.ts` (locale matrix + the `script-src 'self'` fixture, which is also where "the chunk actually loads" is proven — this oracle only sees file sizes) |

## Invocation

```bash
python .claude/project-oracles/widget-bundle-size/oracle.py
```

Exit `0` on PASS, `1` on FAIL, `2` on environment failure (Python not found).

A missing `widget/dist/widget.js`, or no `dist/locales/*.js` chunks while `i18n/locales/` has
non-English locales, is a FAIL (2026-09-30; it used to print a vacuous PASS). This oracle measures
the committed build and never builds: CI job `frontends` rebuilds the widget and fails when the
committed `dist/` differs from the rebuild, which is what proves `dist/` matches `src/`.

## Output schema

```
PASS widget-bundle-size
  tracker-list hash: <sha256-hex> (<N> hostnames)
  size: English page-load set <TOTAL>B pre-gzip across <N> file(s)
    widget/dist/redact.js  <SIZE>B
    widget/dist/widget.css  <SIZE>B
    widget/dist/widget.js  <SIZE>B
  size: <N> locale chunk(s), largest <FILE> at <SIZE>B (a visitor loads at most one)
  trackers: no canonical tracker hostname in widget/dist (recursive)
  chunks: every locale chunk is a flat map of widget.* strings (<N> checked)
```

or

```
FAIL widget-bundle-size (tracker hostname in the built widget; locale chunk that is not only strings)
  tracker-list hash: <sha256-hex> (<N> hostnames)
  size: … (as above)

Tracker failure (DEC-FBR-02 brand promise violation):
  widget/dist/widget.js:42  hostname='segment.io' (canonical-tracker; not permitted in widget bundle)
  Remediation: remove the offending import / script-src / fetch URL. feedbackmonk's widget calls home ONLY to feedbackmonk's own backend.

Chunk failure (Contract C42: a locale chunk is a flat map of widget.* strings):
  widget/dist/locales/de.js: key 'admin.title' is outside the widget.* namespace
  Remediation: check widget/scripts/slice-locales.mjs for leaked code, a non-widget namespace or a non-string value; never hand-edit dist/.
```

Missing build:

```
FAIL widget-bundle-size (widget/dist/widget.js is missing)
  Run `npm run build` in widget/ and commit dist/.
```

## Adversarial self-test (v2.0.0, recorded 2026-10-10)

| Injected defect | Expected | Observed |
|---|---|---|
| a tracker hostname appended to `dist/widget.css` | FAIL, tracker | FAIL naming the file and line |
| a function appended inside a translated `dist/locales/de.js` | FAIL, chunk shape | FAIL naming `de.js` |
| a `public.*` key inside a translated chunk | FAIL, chunk shape | FAIL: outside the widget.* namespace |
| the real 33 KB build, over the old 30 KiB cap | PASS, size printed | PASS with the totals |

## Adversarial self-test (v1.1.0, recorded 2026-09-06 — historical, Probes A/C retired)

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

## Lineage

- **FR-FBR-04** — Embeddable widget, lightweight; size reported, not capped (DEC-FBR-IMPL-36; <30KB until 2026-10-10)
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
- **No size cap (DEC-FBR-IMPL-36, 2026-10-10).** The cap began as `30 * 1024 = 30720` (FR-FBR-04
  "<30KB", read as KiB), was raised to 40,960 B by DEC-FBR-IMPL-35 for the form redesign (2,053 B
  over after a ~1 KB refactor), and was removed hours later on the owner's word. Size is printed,
  never judged; the person landing a change reads it.
- **Bundle extensions `.js`/`.mjs`/`.css`**: covers ESM + CommonJS + style
  outputs from vite. SourceMaps (`.map`) are excluded — they're dev-only
  and not shipped to embedders.
- **`pre-gzip` size, not `post-gzip`**: the reported figure is the worst-case
  wire size (an embedder serving with `Content-Encoding: identity`). With gzip
  on, the transfer is roughly a third of it.
- **Tracker list canonical form (sort + lowercase + `\n`-join)**: stable
  serialisation so cosmetic edits (reordering, casing) don't churn the
  hash. The hash is the drift defender, not a strict-format defender.
- **Hostname match is `substring + case-insensitive`**: catches both
  `https://segment.io/sdk.js` and `'//cdn.segment.io/'` patterns. False
  positives are theoretically possible (a customer slug literally named
  `segment.io`) but acceptable — the file `widget/dist/*` is bundled
  output, not customer data.
- **Cold-start vacuous PASS — retired 2026-09-30.** It let the oracle land
  before `widget/dist/` existed (Task Zero ordering). The widget has shipped
  since, so a missing build now fails; freshness against `src/` is CI job
  `frontends` (rebuild + `git status --porcelain -- widget/dist`).
- **The reported total is the ENGLISH PAGE-LOAD SET, not everything under `dist/`**
  (FR-FBR-35 amendment, v1.1.0): once `dist/` holds 31 mutually-exclusive locale chunks, a
  recursive sum measures a page load nobody performs.
- **Chunk shape replaced the per-chunk 4 KiB cap (v2.0.0).** The cap guarded against code or a
  second namespace leaking into a chunk; a size proxy passes a small chunk of code and would fail
  a long honest translation, so the content is parsed instead.
- **The tracker scan still recurses.** The page-load set narrowed what is summed, on purpose;
  the tracker scan must keep seeing every built file, locale chunks included,
  and there is an adversarial self-test row above whose only job is to keep that
  true.
