<!--
Agent Context Header (ULADP):
- Purpose: Feedback notification email module — plain-text templates (FR-FBR-09),
  brand-parameterised render chokepoint, lettre SMTP send path. Distinct from
  `mailer.rs` which handles signup-verification emails.
- Owner module: crates/feedbackmonk-api/src/email/
- Read first: this README + Contract C10 in
  docs/planning/handoffs/p1-stage1-to-stage2.md
-->

# email/ — Feedback notification chokepoint

## Synopsis

Outbound feedback-notification email module (FR-FBR-09, plain-text). Every confirmation / status-change / public-reply email funnels through one `EmailNotifier::send_email` chokepoint so brand parameterisation (Contract C10) is uniform. Holds the `Mailer` trait + plain-text template renderers + the env-selected send path (Mailpit in dev, lettre SMTP in prod). **Localized since FR-FBR-37**: every `Mailer` method and `render_*` takes a required `Locale`, and all copy comes from the `email.*` catalog namespace with per-key English fallback — recipient language resolves `feedback.submitter_locale` → `tenants.locale` → `en`. **Outbound machine translation since FR-FBR-40**: when the tenant opted in and a provider is configured (both off by default), the chokepoint translates the team's own status note / public reply into the submitter's language and the template renders it ABOVE the original — every failure mode sends the original unchanged. Two things not to break: Contract C10's subject shape `[{prefix} #{fb_id}] {short_subject}` is structural (words localize, structure does not), and English output is byte-locked by insta snapshots.

## 1. Purpose & Responsibilities

Stage 2 Worker A's deliverable for **FR-FBR-09 (status emails, plain-text)**.
Every feedback-notification email (confirmation, status-change, public-reply)
funnels through one `EmailNotifier::send_email` call so:

- brand parameterisation (Contract C10) is uniform across all three template kinds,
- the chokepoint resolves `EmailTenantBrand` per-call via `TenantRepo::get_brand`,
- `submitter_email = None` short-circuits to a noop with an `info!` log line
  (anonymous-no-email submitters; internal-visibility replies),
- the PII scrubber's chokepoint at the tracing layer (Contract C9) protects every
  log line the notifier emits — no special-case scrubbing here.

The module ships two layers:

1. **Pure templates** (`templates.rs`) — deterministic `(brand, context) -> RenderedEmail`
   functions. No I/O, no async.
2. **Send chokepoint** (`send.rs`) — `EmailNotifier` trait + `LettreEmailNotifier`
   prod impl (lettre SMTP) + `RecordingEmailNotifier` test impl.

Separate from `mailer.rs` (signup verification emails) so the two paths can
evolve independently. Production uses the same SMTP transport for both
(Mailpit dev, env-driven SMTP prod) but the feedback-notification chokepoint
adds per-tenant brand resolution that the signup path doesn't need.

## 2. File Index

| File | One-line summary |
|------|---|
| `mod.rs` | Module surface — `pub use` re-exports of every type Worker A's handlers consume. |
| `templates.rs` | Plain-text template renderers: `render_confirmation`, `render_status_change`, `render_public_reply`. Brand- AND locale-parameterised; every literal comes from `i18n/locales/<code>/email.json`. English is locked byte-for-byte by `insta` snapshots. `with_optional_translation` renders the FR-FBR-40 machine translation above the original. |
| `send.rs` | `EmailNotifier` trait + `LettreEmailNotifier` (lettre SMTP) + `RecordingEmailNotifier` (test). `is_submitter_visible_transition` filters re-open/un-merge from the email path. `resolve_recipient_locale` / `resolve_account_locale` are the two FR-FBR-37 language ladders. `outbound_translation` is the whole FR-FBR-40 decision (FR-FBR-40). |
| `mailpit.rs` | P0 signup-verification + password-reset mailer over Mailpit (dev), plus the four catalog-driven body renderers both mailers share. Not part of FR-FBR-09 — kept here for module cohesion with `env_smtp.rs`. |
| `env_smtp.rs` | P0 signup-verification mailer over env-driven SMTP (prod). |
| `README.md` | This file. |

## 3. Public API & Usage

### Render (no I/O)

```rust
use feedbackmonk_api::email::{render_status_change, StatusChangeContext, RenderedEmail};
use feedbackmonk_core::FeedbackStatus;

let rendered: RenderedEmail = render_status_change(&brand, &StatusChangeContext {
    feedback_id: &fb_id,
    from_status: FeedbackStatus::Submitted,
    to_status:   FeedbackStatus::Triaged,
    reason_note: None,
}, Locale::EN);
```

`Locale` is required, not optional (FR-FBR-37). An `Option` defaulting to English would make
"forgot to pass the language" produce output indistinguishable from correct output — the one
failure mode a localized email system cannot afford.

### Send chokepoint (handlers)

```rust
use feedbackmonk_api::email::{EmailKind, EmailContext, EmailNotifier};

state.email_notifier
    .send_email(scope.tenant(), EmailKind::StatusChange { from, to, reason_note },
                EmailContext { feedback_id, submitter_email: feedback.end_user_email,
                   submitter_locale: feedback.submitter_locale, .. })
    .await?;
```

The chokepoint resolves the language itself — hand it `submitter_locale` straight off the
`Feedback` row and it applies the ladder below.

### Submitter-visibility filter (handlers)

```rust
use feedbackmonk_api::email::is_submitter_visible_transition;

if is_submitter_visible_transition(to_status) {
    // ... build EmailKind::StatusChange and send.
}
```

## 4. Constraints & Business Rules

- **Plain-text only.** FR-FBR-09 mandates plain-text. The template tests assert
  no HTML markers (`<html`, `</…>`, `<br`) in any rendered body. Adding HTML is
  a spec change, not a refactor — bring DEC-FBR-?? to LD first.
- **One chokepoint.** Every send path goes through `EmailNotifier::send_email`.
  Bypassing the chokepoint (calling `lettre::AsyncSmtpTransport::send` directly
  from a handler) loses brand parameterisation and tracing — do not do this.
- **`submitter_email = None` is not an error.** Submitters may be anonymous
  without an email on file, or a reply may be `visibility=internal`. The
  chokepoint returns `Ok(SendOutcome::Skipped)` and emits a single `info!`
  log line. Handlers translate `Skipped → email_queued: false` in the JSON
  response (Contract C7).
- **Mail failure does NOT roll back DB writes.** The handler commits the
  status transition / reply insert FIRST, then sends the email post-commit.
  A failed send emits `tracing::warn!` and proceeds — the DB state is the
  source of truth, the email is best-effort notification.
- **Subject format is Contract-locked.** `[{email_subject_prefix} #{FB-id}] {short_subject}`
  is byte-for-byte from Contract C10. The insta snapshots lock this.
- **The recipient's language is resolved at the chokepoint, not at the call site.** Two ladders,
  because the two kinds of mail have two different recipients:
  - **Notification mail** (confirmation / status-change / public-reply) goes to the SUBMITTER:
    `feedback.submitter_locale` → `tenants.locale` → English (`resolve_recipient_locale`). The
    submitter's own captured language wins over the admin's setting; the tenant value is a fallback
    for rows that carry none, not a default that overrides one.
  - **Account mail** (verify / password reset) goes to the ADMIN: `tenants.locale` →
    `Accept-Language` of the request → English (`resolve_account_locale`). At signup there is no
    stored setting yet, so the signing-up browser is the only signal there is.
  A tenant-locale read failure degrades to English with a `warn!` — it must never swallow the email.
- **English output is byte-locked.** Moving every literal into `i18n/locales/en/email.json` must not
  change a single byte any recipient sees. Ten `insta` snapshots enforce it: six on the three
  feedback templates × two brand fixtures, four on the account emails' text and HTML parts (which
  had no test at all before FR-FBR-37 — the gap most likely to have hidden a silent drift).
- **A missing translation renders English, never a raw key.** Fallback is per key (C35 rule 6), and
  `every_shipped_locale_renders_both_account_emails_without_raw_keys` asserts it across all 31
  locales for every catalog state, including today's untranslated skeletons.
- **Tenant-authored text is never translated.** `footer_signature` is the customer's own sign-off;
  brand names (`feedbackmonk`, the tenant's `brand_name`) are names. Both pass through verbatim.
- **Outbound machine translation renders ABOVE the original, and the original is always present (FR-FBR-40).** When the tenant has `translate_outbound` on AND a provider is configured AND the submitter's own captured locale is known and not English AND that locale has a provider target code, the team's `reason_note` / `reply_body` is translated and the body reads: translation, blank line, the localized `email.machineTranslated` line, then the original. A machine translation is evidence about the original, not a replacement for it — a mistranslated status decision must stay checkable. Exactly two strings are eligible; the confirmation `body_excerpt` is the SUBMITTER's own words (the string Q24 is about) and is never translated, and the chrome never needs to be (it comes from the catalog in all 31 languages).
- **A translation problem can never delay or drop an email.** Provider absent, tenant opted out, tenant-flag read failure, no submitter locale, English recipient, no provider target code (`ga fa ml is si`), provider error, empty or echoed response — all of them send today's email unchanged. `outbound_translation` returns `Option`, not `Result`.
- **No new state.** The outbound path writes nothing and reads no feedback column — it is not the FR-FBR-30 pipeline and must never become a reader of the stored translation (Q24; `translation-egress-q24-isolation` Probe B).
- **Re-open transitions are silent.** `Submitted → Submitted` is not a real
  transition (rejected by the state machine). `WontFix/Duplicate → Submitted`
  is a re-open / un-merge — Contract C6 admin-internal correction. The
  submitter does not get an email for these per
  `is_submitter_visible_transition`.

## 5. Relationships & Dependencies

- **Reads** `EmailTenantBrand` from `feedbackmonk_repository::TenantRepo::get_brand`
  (Contract C10; brand columns added by migration 00005).
- **Reads** `FeedbackStatus` + `FeedbackId` from `feedbackmonk_core`.
- **Inherits** PII scrubbing from `feedbackmonk_tracing::install_global_subscriber`
  — every emitted log line is scrubbed automatically.
- **Consumed by**:
  - `handlers/admin_feedback.rs::transition_status` (StatusChange emails)
  - `handlers/admin_feedback.rs::reply` (PublicReply emails)
  - (P0 carry-state path) `handlers/signup.rs` and `handlers/account_recovery.rs` use the separate
    `Mailer::{send_verify_email, send_password_reset_email}` path, NOT this chokepoint. Both pass a
    `Locale` from `resolve_account_locale`.
- **Reads** the catalogs and `Locale` from `feedbackmonk_i18n` (Contract C40). The catalogs are
  compiled in, so rendering does no file I/O and cannot fail on a missing bundle.
- **Reads** `tenants.locale` via `TenantRepo::get_locale` (migration 00032), written by the C38
  settings endpoint in `handlers/tenant_settings.rs`.

## 6. Decision Log

- **The outbound translation provider is a builder (`with_translator`), not a constructor argument.** Translation is optional in the strongest sense — absent by default, absent in every test that does not ask for it, absent in every deployment that has not opted in — so threading it through both constructors would make every call site restate `None`, including the Mailpit integration test that has nothing to do with it.
- **The FR-FBR-40 decision takes the tenant opt-in as a `bool`, not a repo handle.** `outbound_translation(provider, tenant_opted_in, kind, ctx)` lifts the one database read out to its caller, which makes every branch — opted out, no provider, English recipient, no provider code, provider error, nothing team-authored — testable with a fake provider and no database, no SMTP server and no network. The notifier method is then a five-line delegation around one repo read.
- **Only the submitter's OWN captured locale triggers an outbound translation, not the recipient-locale ladder.** The ladder's second rung is the tenant setting, which is the ADMIN's language — i.e. most likely the language the note was written in. Translating into it would pay a provider to translate text into its own source language. A `None` submitter locale is not a request for a translation.

- **Plain-text only, not multipart.** FR-FBR-09 deferred-decisions resolution.
  Plain text renders identically across every email client, dodges the entire
  CSS-rendering-quirk surface, and keeps the unsubscribe footer auditable.
  Markdown-to-HTML is a P3+ consideration.
- **Send chokepoint distinct from `Mailer`.** Signup-verification (`Mailer`)
  and feedback notifications (`EmailNotifier`) share a SMTP transport in
  production but differ in template + brand resolution. Two traits keep the
  responsibility separation clean; the cost is one extra Arc-dyn field in
  `AppState`.
- **Brand resolution per-call, not per-state.** `EmailTenantBrand` could be
  cached in `AppState` keyed by `tenant_id`, but the table is tiny and the
  read is sub-millisecond. Cache invalidation on `update_brand` would add
  more complexity than the saved DB round-trips.
- **Submitter-visibility filter centralised in `send.rs`.** Each handler
  could implement its own "should I email?" check, but centralising
  `is_submitter_visible_transition` avoids drift between the
  `transition_status` and a future `bulk_transition` (P3+) endpoint.
- **Inline `insta` snapshots, not file-on-disk.** The brief specified
  "6 snapshot files minimum"; we use `insta::assert_snapshot!(value, @"...")`
  inline form. Functionally equivalent — insta treats inline + file
  snapshots identically — and avoids the `snapshots/` directory churn
  during PR review (the snapshot lives directly next to the test).
