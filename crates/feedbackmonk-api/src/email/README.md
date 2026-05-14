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
| `templates.rs` | Plain-text template renderers: `render_confirmation`, `render_status_change`, `render_public_reply`. Brand-parameterised; locked by `insta` snapshots. |
| `send.rs` | `EmailNotifier` trait + `LettreEmailNotifier` (lettre SMTP) + `RecordingEmailNotifier` (test). `is_submitter_visible_transition` filters re-open/un-merge from the email path. |
| `mailpit.rs` | P0 signup-verification mailer over Mailpit (dev). Not part of FR-FBR-09 — kept here for module cohesion with `env_smtp.rs`. |
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
});
```

### Send chokepoint (handlers)

```rust
use feedbackmonk_api::email::{EmailKind, EmailContext, EmailNotifier};

state.email_notifier
    .send_email(scope.tenant(), EmailKind::StatusChange { from, to, reason_note },
                EmailContext { feedback_id, submitter_email: feedback.end_user_email, .. })
    .await?;
```

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
  - (P0 carry-state path) `handlers/signup.rs` uses the separate
    `Mailer::send_verify_email` path, NOT this chokepoint.

## 6. Decision Log

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
