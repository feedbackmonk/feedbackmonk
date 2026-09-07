---
schema: test-modification-justification/v1
vouched_by: judge
judge_model_id: "claude-opus-5[1m]"
judge_verdict: VOUCH
judge_ruled_at: "2026-09-07T10:24:03Z"
judged_test_diff_sha256: "a7653d562dab00f51302faabfd266b558405717e70d41379931eae09c0750130"
judged_code_diff_sha256: "b5782d445f86e1f482b077f93aaada89afd8e4bddfd4157cf2e3ac7a90b73a32"
working_session_id: "CLAUDE-D"
commit: ""
session_id: "CLAUDE-D"
authored_at: "2026-09-07T10:24:03Z"
authored_by: judge
tests_modified:
  - path: "admin-ui/src/pages/autopilot/__tests__/Board.test.tsx"
    change_type: rewrite
    lines_changed: 12
    description: "Routing-tag assertion relocated from the visible glyph (getByText(/→ ci-runner/)) to the stable title attribute with toHaveTextContent; claimed-runner assertion given the same element-scoped form. No test added or removed (10/10 before and after)."
code_modified:
  - path: "admin-ui/src/pages/autopilot/BoardCard.tsx"
    change_type: new-behavior
    lines_changed: 36
    description: "R-A11Y A-5 accessibility fix: the routing '→' glyph is now aria-hidden and preceded by a visually-hidden 'Routed to:' label, splitting what was one text node into sibling spans; tag titles and labels moved onto i18n keys (admin.boardCard.*)."
rationale_summary: "Test tracks a real DOM split forced by the a11y fix; same two values asserted, scoped tighter, strings sourced from the en catalog."
hypothesis_ledger_ref: ""
spec_change_ref: "R-A11Y finding A-5 (accessibility review, UI-localization Stage 2 wave)"
---

# Test Modification Justification — Independent Judge Vouch (ARHG-06)

> **What this artifact is**: an independent, fresh-context judge — which did **not** write the code under
> examination and has no stake in the test passing — examined the test diff, the code diff, and the working
> agent's rationale, and ruled this **legitimate test evolution consistent with the development change**.
> It is the Phase 0.5 justification artifact for that change, and the working agent proceeded on this ruling
> instead of interrupting the user (DEC-190, amending DEC-111).
>
> **Authored by the judge, never by the working agent.** The Flag Integrity Rule is amended, not repealed:
> the judge is vouching source 3, alongside plan/spec pre-grant (ARHG-05) and the user's explicit word.
> The agent being graded still never vouches for itself and never writes this file.
>
> **This file is the whole artifact (ARHG-14, DEC-430).** The framework's vouch artifact set is closed: one
> record per VOUCH under `docs/test-modifications/`, and no ledger, index or JSONL beside it. If your project
> keeps an additional local audit surface, **that surface is your project's to enforce** — the ARHG-01
> pre-commit gate checks this record's presence and nothing else, so a row omitted from a local ledger is
> omitted silently. Enforce it in the same commit that stages this record, not with an oracle run afterwards.
>
> **Honesty caveat (DEC-131 tier)**: authenticity here is enforced by role and schema, not cryptographically.
> The authority chain is auditable — the record names the judge, its model, the verdict, and hashes of the
> exact diffs judged — but it is not tamper-proof. What makes a bad VOUCH recoverable is that nothing
> downstream was removed: the ARHG-01 pre-commit gate, Phase 0.5 detection, the DEC-84 subordinate deletion
> deferral, and the PODS critic all still run.

## Judge verdict

**VERDICT: VOUCH** — by `claude-opus-5[1m]` at `2026-09-07T10:24:03Z`.

## Judge's reason

The single changed assertion moved from `getByText(/→ ci-runner/)` to
`getByTitle("Routed to runner")).toHaveTextContent("ci-runner")`, and the code diff independently forces
exactly that: `BoardCard.tsx` now renders the routing tag as three children —
`<span class="visually-hidden">{routedToLabel} </span><span aria-hidden="true">→</span> {routing_label}` —
so the arrow and the runner value no longer share a text node and Testing Library's single-element
`getByText` can no longer match across the sibling split. Nothing was weakened: the same two values
(`routing_label` = `ci-runner`, `claimed_by_runner` = `claimed · ci-runner`) are still asserted, the test
count is unchanged at 10, and the assertion is in fact *scoped tighter* than before — it now requires the
value to live inside the specific element carrying `title="Routed to runner"` / `"Claimed by runner"`,
whereas the old `getByText` would have matched the string anywhere in the document. The expected strings
are sourced, not invented: `i18n/locales/en/admin.json` `admin.boardCard.routedTitle` = `"Routed to runner"`,
`claimedTitle` = `"Claimed by runner"`, `claimedBy` = `"claimed · {{runner}}"`, with the middle dot at
U+00B7 in both the catalog and the test literal (verified by codepoint comparison, not by eye). The only
thing no longer asserted is the presence of the decorative `→` glyph in the visible text, which the code
diff deliberately made `aria-hidden` — so this edit would have been the correct edit even if the test had
somehow kept passing, which is the discriminator.

On the second assertion, which the working agent flagged as possibly untouched: it *was* changed, not merely
reformatted — `getByText(/claimed · ci-runner/)` became
`getByTitle("Claimed by runner")).toHaveTextContent("claimed · ci-runner")`. The claimed-runner span's text
is still a single node (`t("admin.boardCard.claimedBy", { runner })`), so `getByText` would still have
worked there; the change is a locator-consistency improvement and is itself a slight strengthening
(document-wide match → element-scoped match) with a byte-identical expected string. Judged separately, it
is also legitimate.

## What the judge examined

- **Test diff** (`sha256: a7653d562dab00f51302faabfd266b558405717e70d41379931eae09c0750130`): one file,
  `admin-ui/src/pages/autopilot/__tests__/Board.test.tsx`, one test
  (`"renders routing_label and claimed_by_runner on a card"`), two assertions relocated from text-content
  matching to `getByTitle(...)` + `toHaveTextContent(...)`, plus a four-line explanatory comment. No test
  added, removed, skipped or `.only`'d.
- **Code diff** (`sha256: b5782d445f86e1f482b077f93aaada89afd8e4bddfd4157cf2e3ac7a90b73a32`):
  `admin-ui/src/pages/autopilot/BoardCard.tsx` — R-A11Y A-5 accessibility fix making the `→` routing glyph
  `aria-hidden` behind a visually-hidden "Routed to:" label (the glyph does not mirror in RTL locales such
  as `fa` and was announced as a bare arrow), with the tag's `title`/label strings moved onto the
  `admin.boardCard.*` i18n keys and `formatRelative` taking the active locale.
- **Working agent's stated rationale**: that `getByText` stopped matching because the arrow and the runner
  label now live in sibling spans rather than one text node, so the value is asserted via the element's
  stable `title` instead — a claim the judge treated as under examination and verified independently by
  reading `BoardCard.tsx`, comparing the asserted strings against `i18n/locales/en/admin.json` by codepoint,
  and running the suite (`npx vitest run src/pages/autopilot/__tests__/Board.test.tsx` → 10/10 passed, the
  same count the pre-change file contained).

## Working agent's rationale (as submitted)

> `admin-ui/src/pages/autopilot/BoardCard.tsx` previously rendered a routing-label tag as
> `<span className="ap-board-card-routing" title="Routed to runner">→ {order.routing_label}</span>` — a bare
> `→` glyph as part of the element's own text content, not `aria-hidden`. An accessibility reviewer (R-A11Y)
> found this glyph does not mirror in a right-to-left locale (`fa`) and is read aloud by screen readers as a
> literal arrow character with no relationship label. The fix restructured it so the arrow is now
> `aria-hidden`, and a new visually-hidden (CSS-clipped, still in the DOM/accessibility tree) text label
> "Routed to:" precedes it, so a screen reader announces "Routed to: ci-runner" instead of a bare arrow
> glyph, while sighted users still see "→ ci-runner" visually.
>
> The test `Board.test.tsx`'s test `"renders routing_label and claimed_by_runner on a card"` originally
> asserted `expect(screen.getByText(/→ ci-runner/)).toBeInTheDocument()`. Because the arrow and the
> runner-label text are now split across sibling `<span>` elements rather than living in one text node,
> Testing Library's `getByText` (which normally requires the match text to belong to one element's own text
> content, not be assembled across sibling elements) stopped finding it and the test failed with a timeout.
> My fix changed the assertion to locate the element via its stable `title` attribute instead of the visible
> glyph, and assert on `toHaveTextContent` (a substring check over the full text content of that element,
> robust to the internal child-element split). The second assertion's element and text were already present
> before my change and I did not need to touch that line's semantics — verify whether I actually changed it
> or only reformatted it as part of the same edit; if changed, judge that piece too.

---

**Requirement**: ARHG-06. **Decision**: DEC-190. **Judge agent**: `~/.claude/agents/test-mod-judge.md`.
**Consumer**: `~/.claude/segments/-finalize/phase0.5-test-mod-gate.md` § 0.5.16.
