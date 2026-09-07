# `widget/` — the byte cap that governs every widget change

<!-- #!binds widget/src/** widget/dist/** -->
The widget's page-load set (top-level `widget/dist/*`) is capped at **30,720 B** (FR-FBR-04) and
each lazily loaded `widget/dist/locales/<code>.js` at **4,096 B**. The `widget-bundle-size` oracle
measures both; it is the authority on the current figure, so run it rather than trusting a number
written down anywhere.

Headroom is thin — the last measurement left well under 1 KB. When a change does not fit, the
documented lever is the `widget.`-prefix rename (~735 B, see `widget/README.md`). Raising the cap
is not the lever: it is FR-FBR-04.

DEC-FBR-02 forbids any third-party tracker in this tree — no Segment, Mixpanel, GA, Intercom. The
same oracle's Probe B scans `widget/dist/` recursively for them, and it is a brand promise, not a
preference.

`widget/dist/` is vendored downstream by GitCellar. A change here means GitCellar must re-vendor
the **whole tree**, not just `widget.js`: `dist/locales/` is new and the entry point has moved
twice. That work is filed in the GitCellar repo.
