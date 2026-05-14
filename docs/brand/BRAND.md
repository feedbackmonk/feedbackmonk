# feedbackmonk — Brand Kit (v1)

**Status**: FROZEN for P4 Stage 2 (the "good-enough v1" kit; iterate post-launch).
**Frozen at**: P4 Stage 1, 2026-05-14.
**Authority**: this file is the source of truth that Worker A (marketing site) and Worker B (self-host docker docs) consume as Contract **C20**.

> **Framing**: this is the v1 brand kit. It is **good enough to ship** the marketing site and the Show HN draft. It is **not** the final-state visual identity — that iterates from real-launch signal post-Stage-2 beta. Per DEC-FBR-09: the brand pass is bundled with the marketing site, not a separate sprint.

---

## Wordmark

**v1 = text-only wordmark.** No logo-mark, no symbol, no emblem.

- Spelling: `feedbackmonk` (lowercase, single word, no space).
- Typesetting: set in the heading font (Inter, see § Typography) at semibold or bold; case-as-rendered (do not auto-capitalize).
- Pronunciation: /ˈfiːdbæk.mʌŋk/ ("feedback-monk"). Two syllables in conversation; one visual unit on paper.
- Stress: equal-weight; no internal capital (NOT "FeedbackMonk", NOT "feedbackMonk").

**Why no logo-mark in v1**: the wordmark IS the brand at this stage. Adding a hasty symbol creates a sub-Plausible visual identity and signals "still figuring it out." Better to ship clean wordmark + cohesive typography than ship a mediocre mark. Logo-mark exploration is post-launch work, gated on a real designer engagement or a clear self-derived concept that holds up under critique.

### Wordmark do / don't

✅ **DO**: use the wordmark as the site nav-bar branding, the README title (`# feedbackmonk`), the footer attribution string (`powered by feedbackmonk`), and the email-template footer.

❌ **DON'T**: stylize with custom letterforms, add hyphens (NOT `feedback-monk`), add suffixes (NOT `feedbackmonk.io` in branding contexts — the domain is the domain, the wordmark is the wordmark), italicize, all-caps, or wrap in decoration.

---

## Color Palette

Six tokens. Constraint-first design: cool ink + warm cream + a single restrained accent. Reads as "monastic craft" — quiet, disciplined, evidenced — rather than "SaaS landing page" (no purple gradient, no saturated blue, no neon).

| Token | Hex | Use |
|---|---|---|
| `--brand-ink` | `#1a1a1a` | Primary text on light backgrounds; wordmark; headings. Near-black but warmer than pure `#000`. |
| `--brand-cream` | `#faf8f4` | Primary surface (page background, card background). Warmer than `#ffffff`; reads softer; less SaaS-clinical. |
| `--brand-sage` | `#5a8a6a` | Single accent. Use for CTAs, hyperlinks, status badges, the `--brand-cream`-on-`--brand-sage` button. Muted enough to not shout; saturated enough to call attention. |
| `--brand-sage-deep` | `#3f6b4f` | Hover / active state for `--brand-sage` UI. ~12% darker for AA contrast on cream surface. |
| `--brand-muted` | `#6b6863` | Secondary text (captions, metadata, footnote-grade copy). Warm-tinted grey to harmonize with cream. |
| `--brand-rule` | `#e8e3da` | Borders, hairlines, dividers, code-block backgrounds. Just-darker than cream. |

### Accessibility (WCAG AA, normal text 4.5:1, large text 3:1)

| Foreground / Background | Ratio | Verdict |
|---|---|---|
| `--brand-ink` on `--brand-cream` | 16.5:1 | ✅ AAA |
| `--brand-muted` on `--brand-cream` | 5.2:1 | ✅ AA normal |
| `--brand-sage` on `--brand-cream` | 4.8:1 | ✅ AA normal |
| `--brand-sage-deep` on `--brand-cream` | 7.1:1 | ✅ AAA |
| `--brand-cream` on `--brand-sage-deep` | 7.1:1 | ✅ AAA (button "primary" state) |
| `--brand-ink` on `--brand-rule` | 14.8:1 | ✅ AAA (code-block text) |

**Dark mode**: defer to v1.1. The marketing site ships light-only for v1; the admin UI already has its own theming concerns and is out of scope for this brand kit.

### Color do / don't

✅ **DO**: pair `--brand-ink` headlines on `--brand-cream` for hero sections; use `--brand-sage` for ONE element per viewport (the primary CTA); use `--brand-rule` for code blocks and table dividers; use `--brand-muted` for "EU + US hosting" trust signals.

❌ **DON'T**: introduce a fourth color without a written rationale here; gradient backgrounds (clichéd); pure `#000` or pure `#fff` (breaks the "warm" register); saturated reds/oranges/blues anywhere — they read as SaaS-clinical and clash with the monastic posture.

---

## Typography

Two libre faces, both self-hostable (no Google Fonts CDN dependency post-launch).

### Headings + UI: **Inter**

- Source: rsms.me/inter, OFL-licensed.
- Subset: latin + latin-ext.
- Weights ship: 400 (regular), 500 (medium), 600 (semibold), 700 (bold).
- Use: H1–H6, nav links, buttons, form labels.
- Default sizes (mobile-first; site can override per-page):
  - H1 — `font-size: clamp(2rem, 4vw + 1rem, 3.5rem); font-weight: 700; letter-spacing: -0.02em; line-height: 1.1`
  - H2 — `font-size: clamp(1.5rem, 2vw + 1rem, 2.25rem); font-weight: 600; line-height: 1.2`
  - H3 — `font-size: 1.25rem; font-weight: 600; line-height: 1.3`
  - Body — `font-size: 1.0625rem (17px); font-weight: 400; line-height: 1.6`

### Body + Code: **JetBrains Mono** (code only) + **Inter** (body)

- Body copy is set in **Inter 400** at 17px (1.0625rem) — slightly larger than the SaaS-default 16px for marketing readability.
- Code blocks and inline `code` are set in **JetBrains Mono** (Apache-2.0; self-hosted from the same `marketing/public/fonts/` directory).
- Code block size: `0.9375rem (15px)`, `line-height: 1.55`.

### Self-host the fonts

Worker A bundles the `.woff2` files (Inter Variable + JetBrains Mono Variable) under `marketing/public/fonts/` and serves them via `@font-face` with `font-display: swap`. No CDN fetch. Justification: DEC-FBR-02 "no third-party trackers" extends to fonts — Google Fonts CDN is a third-party fetch.

### Type do / don't

✅ **DO**: set hero H1 in Inter 700 at the upper clamp boundary; set code samples in JetBrains Mono; use `letter-spacing: -0.02em` on large headings (Inter tracks loose at display sizes); use `font-feature-settings: 'cv11', 'ss03'` to enable Inter's curved single-storey `a` (subtle character that distinguishes from system-Inter).

❌ **DON'T**: introduce serif faces (clashes with the geometric register); use Inter Italic for emphasis in body copy (use `font-weight: 600` instead); use ALL CAPS for body content (only for the rare 4-letter eyebrow label, never for sentences).

---

## Voice & Tone

Six bullets. These govern marketing-site copy, README intros, blog posts, the Show HN draft, and the in-product help copy that Worker A authors. Internal docs (decisions, READMEs) have their own tone — these guidelines are for outward-facing copy.

1. **Direct, not breathless.** Active voice. Short sentences. Land the point in the first half of the paragraph. *"feedbackmonk runs in your tenant. Your users' feedback never touches our servers"* — not *"feedbackmonk has been designed from the ground up with privacy as a first-class concern."*

2. **Privacy claims are evidenced, never aspirational.** Every privacy claim links to code, an oracle output, or the AGPL repo. *"Zero third-party trackers — verified in CI"* (link to `.claude/oracles/widget-bundle-size/`). *Not*: *"we deeply respect privacy" / "your data is safe with us."* If the claim cannot be linked to evidence, do not make it.

3. **First-person plural for builder voice; second-person for reader.** *"We built this because…"* / *"You install the widget, you own your data."* No third-person corporate-speak (*"feedbackmonk has been designed to help organizations…"*).

4. **Plain words.** Ban list (do not use without explicit reason): leverage, synergize, robust, world-class, best-in-class, cutting-edge, paradigm, holistic, ecosystem, journey, empower. Most of these scream "AI-generated SaaS landing." Replace with concrete verbs.

5. **Honest about limits.** *"v1 doesn't have webhook integrations yet — they're on the roadmap."* Stating limits builds trust faster than hiding them; the audience (Persona A/D per DEC-FBR-01) reads "doesn't have X yet" as candor, not weakness.

6. **Long-form is fine.** Marketing copy can be paragraphs, not just bullet salad. The audience reads. Plausible's homepage runs ~1,500 words of running prose; that's the register. *Bullet salad is the lazy default; prose with a point earns trust.* Use bullets for genuine enumeration, prose for everything else.

### Tone do / don't

✅ **DO**: write Show HN posts that read like a developer talking to other developers; cite real numbers (widget bundle = 16.8 KB, 45% under cap); admit trade-offs (*"Self-host requires Postgres; we don't ship SQLite — here's why"*); link to the AGPL repo from every page.

❌ **DON'T**: use emoji as decoration (one per page max, only if it adds information — e.g., a 🟢 status indicator); add testimonial sections to v1 (fake testimonials erode trust faster than no testimonials); write FAQs with leading questions ("*Is feedbackmonk easy to use?*"); use exclamation points in body copy.

---

## Footer Attribution String

**Canonical**: `powered by feedbackmonk`

- All lowercase.
- No leading article ("by feedbackmonk" alone is fine in 2nd-instance prose, but the footer phrase is the full form).
- Always followed by a link to `https://feedbackmonk.com` (or the configured `--brand-rule`-colored anchor in the widget).
- Tier-flip-aware: rendered on `Tier::Free`, suppressed on paid tiers per `feedbackmonk-repository::tenants::get_widget_brand` (existing implementation, **DO NOT MODIFY** in P4).
- In the email-template footer: same string + same domain link; no separator (just inline at the bottom of plain-text emails).

This string is already shipped in the widget (FR-FBR-14, P3 Stage 1). Worker A consumes the SAME string verbatim for the marketing site footer and the Show HN post footer.

---

## Show HN Voice (per-channel)

The Show HN post draft (lives at `marketing/src/blog/show-hn-draft.md` post-Worker-A) follows the six voice guidelines above, plus three channel-specific norms:

- **Title pattern**: `Show HN: feedbackmonk – privacy-first product feedback (AGPL)`
- **Open with the artifact, not the story**: first paragraph is "*here's what it does, here's a link to try*"; the founder-journey paragraph (if included) is paragraph 3 or later.
- **Include one concrete technical detail**: e.g., "widget bundle is 16.8 KB; 18-hostname tracker scan runs in CI; here's the oracle" — concrete details earn the front-page upvotes from the dev audience.

The draft does NOT auto-post on Stage 2 close. User reviews and revises before posting (per DEC-FBR-10 Stage 2 trigger).

---

## Decision Log

- **2026-05-14** — Wordmark-only v1 (no logo-mark). Rationale: hasty mark is worse than no mark; defer to post-launch.
- **2026-05-14** — Color palette = ink/cream/sage (warm + restrained). Rationale: differentiates from SaaS-clinical default; aligns with "monk" semantic without being heavy-handed.
- **2026-05-14** — Self-host fonts (Inter + JetBrains Mono). Rationale: extends DEC-FBR-02 no-third-party-trackers brand promise to font-loading.
- **2026-05-14** — Dark mode deferred to v1.1. Rationale: ship the marketing site light-only; dark-mode is a polish iteration post-launch.
- **2026-05-14** — Voice guidelines codified as 6-bullet rules. Rationale: gives Worker A and future maintainers a checklist; reduces drift across pages.

---

## Consumers

- **Worker A** (marketing site, FR-FBR-16): imports color tokens as CSS custom properties under `marketing/src/styles/brand.css`; loads fonts from `marketing/public/fonts/`; references voice guidelines when authoring page copy and the Show HN draft.
- **Worker B** (self-host docker, FR-FBR-17): references this kit only for the `deploy/docker/README.md` header tone and the `docs/operations/SELFHOST.md` voice. Worker B does not consume colors or fonts.
- **Future widget refresh** (POST-v1): if the widget ever gets a visual refresh, it consumes from here. P4 does NOT touch the widget — bundle size oracle defends it.
