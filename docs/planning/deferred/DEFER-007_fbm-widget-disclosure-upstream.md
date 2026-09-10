---
id: DEFER-007
title: "Port GitCellar's widget privacy-disclosure patch into the widget source before the next dist re-sync"
status: PROPOSED
origin: inject
source-project: GitCellar
source-session-id: session-20260831-100904-403
injected-at: 2026-08-31T17:05:00Z
autonomy-hint: collaborative
suggested-entry-point: implement
scope-estimate: single-session
content-hash: gc-fbm-widget-disclosure-upstream
---

# DEFER-007: Port GitCellar's widget privacy-disclosure patch into the widget source

## Idea

GitCellar's claims audit (2026-08-31, owner-approved) patched its two VENDORED copies of the
FeedbackMonk widget (`apps/gitcellar-landing/public/feedback/widget.{js,css}` and the Forge
`custom/public/assets/feedback/` twins in the GitCellar repo). Those copies are re-synced from THIS
project's `widget/dist/`, so the next re-sync silently reverts the patch unless the same changes
land in the widget SOURCE here (`widget/src/` — verify layout) and a fresh dist is built.

## Originating Context

Audit finding (GitCellar, owner-approved D18): the widget's only statement of where submissions go
— "Tell us what's on your mind. Submissions are sent to <display_name>." — was styled
`fbm-sr-only` (screen-reader-only), so a sighted visitor saw a form collecting an optional email
with no destination/privacy disclosure at all. The patch, live in GitCellar's vendored copies
(diff them against this repo's `widget/dist/` for the exact changes):

1. The destination sentence renders VISIBLY: a small muted paragraph under the form title (new
   `fbm-desc` class in `widget.css`; the `fbm-sr-only` class removed from that node in `widget.js`).
2. In the widget's ANONYMOUS mode branch (`"anonymous" === s` in the minified dist), the visible
   line appends: "Sent anonymously — add your email if you'd like a reply." (GitCellar's Cloud
   Forge embeds the widget without a JWT even for signed-in users, so submissions there are
   unattributed; the audit ruled users must be told.)

Both GitCellar copies are byte-identical to each other (md5-verified at patch time) and pass
`node --check`. GitCellar's registry note: `docs/forge-customizations/customizations/03-templates.md`
§3.32 (in the GitCellar repo) records the patch and the re-sync hazard.

Snapshot claims above are from the GitCellar session's diffs — re-measure against the live trees
before acting (INJECT-17).

## Success Criterion

The widget source in this repo produces a dist whose behavior includes both changes (visible
destination line; anonymous-mode reply note), and a rebuilt `widget/dist/` diff-matches (or
functionally matches, if the build isn't byte-reproducible) GitCellar's vendored copies — so the
next re-sync into GitCellar is a no-op rather than a revert.

## Dependencies

None. GitCellar's vendored copies are already correct; this only has to land before the next
widget re-sync into GitCellar.

## Related Artifacts

- This repo: `widget/` (source + dist — verify actual layout)
- GitCellar repo: `apps/gitcellar-landing/public/feedback/widget.{js,css}` (the patched reference),
  `docs/forge-customizations/customizations/03-templates.md` §3.32
