# i18n-literal-ratchet — Verification Oracle

## Assertion

> No user-visible string can reach a widget, public-page or admin screen
> without passing through the catalog — so a new feature cannot quietly ship
> English-only in a product that promises 31 locales.

Frozen in `manifest.json` at Task Zero (2026-09-06).

## What it scans

| Surface | Patterns |
|---|---|
| `widget/src/**/*.ts` | `.textContent =`, `.innerText =`, `createTextNode(...)`, `setAttribute("aria-label"\|"title"\|"placeholder", ...)`, `notify(...)` |
| `admin-ui/src/**/*.tsx` | JSX text nodes (`>text<` containing a letter), `aria-label=`/`title=`/`placeholder=` string attributes, `notify("...")` |

Every match is `(file, normalized text)`. The current set must be a
**subset** of `i18n/literal-baseline.json`; the baseline only shrinks, via
`--freeze` (refuses to write if the set grew).

## Measured false-positive rate (2026-09-06, against this tree)

Raw scan before tuning: **211 literals across 36 files**. Three systematic
false-positive classes, all fixed in the scanner itself (not papered over
with baseline noise):

| Class | Example | Fix |
|---|---|---|
| Translation key mistaken for display text | `el.setAttribute("aria-label", t("widget.attach.listAria"))` grabs `"widget.attach.listAria"` | `TRANSLATION_KEY_SHAPE_RE` excludes any literal shaped like a dotted lower-camelCase identifier — real English prose never looks like that |
| `notify()`'s severity enum mistaken for the message | `notify(\`Token "${x}" revoked.\`, "success")` grabs `"success"` (the template-literal message is skipped, so the severity argument is the first plain-quoted string in the statement) | `_literal_after` now checks which quote character opens *first*; a backtick opening before `'`/`"` means the real argument is a template and the whole match is skipped, not leapt over. `"info"`/`"success"`/`"error"` (the closed `ToastKind` union, `admin-ui/src/components/Toast.tsx`) are also allow-listed directly |
| Plain TS comparison mistaken for JSX text | `if (rung >= 0 && rung <= 3)` reads as JSX text between the `>` of `>=` and the `<` of `<=` | `JSX_TEXT_NODE_RE` excludes `=`, `&`, `|` from the captured span — real JSX prose essentially never needs those characters literally (an ampersand in real copy is written `&amp;`) |

After tuning: **192 literals across 31 files**, all in `admin-ui/src`
— **expected**, not a defect: the admin console's English label maps stay
hardcoded through Stage 1 by design (GUIDE §6.2 — Stage 2/W-D does the admin
extraction). These will be absorbed into the frozen baseline whole at Stage-1
converge; a shrinking baseline is exactly what makes W-D's later extraction
work (cheap-tier, per the oracle's own `rationale`) mechanically verifiable.

**`widget/src` produced exactly one finding after tuning**, and it is a real
bug, not a false positive: `widget/src/attachments.ts:241` —
`it.redactBtn.textContent = prev || "Redact";` — a hardcoded English
fallback that never goes through `t()`. Flagged to the file's owner
(CLAUDE-A) rather than fixed here (GUIDE §5.7).

## Self-test

`oracle.py --self-test` (or either shim with the same flag) exercises the
scanner directly against small in-memory fixtures — no filesystem, no
`i18n/literal-baseline.json` — proving all three fixes above in one
invertible pass: a real literal is still caught, and each false-positive
shape is now excluded.

## Graceful absence

Vacuous-PASS while `i18n/literal-baseline.json` does not exist yet — the LD
establishes it with `--freeze` at Stage-1 converge.

## Known gaps

See `manifest.json` `assertion.known_gaps`: a literal assembled at runtime
or held in a `const` before being passed to a DOM call is not matched
(the a11y locale-matrix specs are the second, behavioral leg); non-user-facing
literals occasionally match the JSX-text pattern and are absorbed into the
baseline at freeze time rather than chased individually; the allow-list is
itself a stated exemption surface, reviewed above.
