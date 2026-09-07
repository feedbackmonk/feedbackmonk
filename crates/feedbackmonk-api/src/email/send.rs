//! Email send chokepoint for the feedback notification paths
//! (FR-FBR-09). Every transition + reply path goes through `send_email`;
//! the function resolves `EmailTenantBrand` via `TenantRepo::get_brand`,
//! renders the appropriate template, and dispatches via the configured
//! `Mailer` implementation.
//!
//! Plain-text only per FR-FBR-09; HTML is NOT a Stage-2 deliverable. The
//! chokepoint design ensures every notification path inherits the same
//! footer, brand parameterisation, and (eventually) unsubscribe handling.
//!
//! Idempotency note: when `submitter_email` is `None` (anonymous-no-email
//! submitter, or a `visibility=internal` reply), `send_email` returns
//! `Ok(())` immediately with an `info!` log line. This is NOT an error
//! state — it is the documented happy-path for unaddressable receivers.

use std::sync::Arc;

use async_trait::async_trait;
use lettre::message::{header::ContentType, Mailbox, SinglePart};
use lettre::transport::smtp::client::Tls;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use thiserror::Error;
use uuid::Uuid;

use feedbackmonk_core::{FeedbackId, FeedbackStatus};
use feedbackmonk_i18n::Locale;
use feedbackmonk_repository::{EmailTenantBrand, TenantRepo, TenantScope};

use crate::email::templates::{
    render_confirmation, render_public_reply, render_status_change, ConfirmationContext,
    PublicReplyContext, RenderedEmail, StatusChangeContext,
};

/// Notification kind passing through the send chokepoint.
#[derive(Debug, Clone)]
pub enum EmailKind {
    Confirmation,
    StatusChange {
        from: FeedbackStatus,
        to: FeedbackStatus,
        reason_note: Option<String>,
    },
    PublicReply {
        reply_id: Uuid,
    },
}

/// Per-call context for the send chokepoint. The handler builds this from
/// the request + the resolved feedback row.
#[derive(Debug, Clone)]
pub struct EmailContext {
    pub feedback_id: FeedbackId,
    pub submitter_email: Option<String>,
    /// First ~200 chars of submission body (for confirmation emails).
    pub body_excerpt: Option<String>,
    /// Reply body string (for `PublicReply`).
    pub reply_body: Option<String>,
    /// The submitter's UI locale as captured at submit time
    /// (`feedback.submitter_locale`, C37). `None` — the common case for every
    /// row that predates FR-FBR-37, and for a submitter whose browser offered
    /// no language we ship — falls back to the tenant's setting, then English.
    pub submitter_locale: Option<String>,
}

#[derive(Debug, Error)]
pub enum EmailError {
    #[error("brand resolution failed: {0}")]
    BrandFailure(#[from] feedbackmonk_repository::RepoError),

    #[error("smtp/transport failure: {0}")]
    Transport(String),
}

/// Outcome of a send call. Used by handlers to populate the
/// `email_queued: bool` response field per Contract C7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendOutcome {
    Sent,
    Skipped,
}

impl SendOutcome {
    #[must_use]
    pub fn was_queued(self) -> bool {
        matches!(self, Self::Sent)
    }
}

/// `EmailNotifier` decouples the send chokepoint from a concrete mailer
/// transport. Production wires a `LettreEmailNotifier` (Mailpit dev /
/// env SMTP prod); tests inject `RecordingEmailNotifier`.
#[async_trait]
pub trait EmailNotifier: Send + Sync {
    /// Resolve brand, render the appropriate template, and dispatch.
    /// Returns `Skipped` (with an `info!` log line) when `submitter_email`
    /// is `None`; this is the documented no-op for unaddressable
    /// notifications.
    async fn send_email(
        &self,
        scope: &TenantScope,
        kind: EmailKind,
        ctx: EmailContext,
    ) -> Result<SendOutcome, EmailError>;
}

/// Production `EmailNotifier` — composes brand lookup with a lettre SMTP
/// transport (Mailpit dev / env SMTP prod). Construction supplies the
/// transport and from-address envelope; the brand lookup happens per-call.
pub struct LettreEmailNotifier {
    tenants: Arc<dyn TenantRepo>,
    transport: AsyncSmtpTransport<Tokio1Executor>,
    /// Envelope `From:` address used at the SMTP layer. The visible "From"
    /// display name comes from the per-tenant brand
    /// (`brand.sender_display_name`).
    envelope_from: String,
}

impl LettreEmailNotifier {
    /// Build for an unauthenticated SMTP host (Mailpit dev).
    pub fn mailpit(
        tenants: Arc<dyn TenantRepo>,
        host: &str,
        port: u16,
        envelope_from: &str,
    ) -> anyhow::Result<Self> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
            .port(port)
            .tls(Tls::None)
            .build();
        Ok(Self {
            tenants,
            transport,
            envelope_from: envelope_from.to_string(),
        })
    }

    /// Build for an authenticated SMTP relay (env-driven prod).
    pub fn from_transport(
        tenants: Arc<dyn TenantRepo>,
        transport: AsyncSmtpTransport<Tokio1Executor>,
        envelope_from: &str,
    ) -> Self {
        Self {
            tenants,
            transport,
            envelope_from: envelope_from.to_string(),
        }
    }
}

#[async_trait]
impl EmailNotifier for LettreEmailNotifier {
    async fn send_email(
        &self,
        scope: &TenantScope,
        kind: EmailKind,
        ctx: EmailContext,
    ) -> Result<SendOutcome, EmailError> {
        let Some(to_addr) = ctx.submitter_email.as_deref() else {
            tracing::info!(
                target: "email",
                feedback_id = %ctx.feedback_id,
                "skipping email: submitter has no address on file"
            );
            return Ok(SendOutcome::Skipped);
        };

        let brand = self.tenants.get_brand(scope).await?;
        // FR-FBR-37: the recipient's language, resolved at the chokepoint so
        // every notification path inherits the same rule (the same reason brand
        // resolution lives here). A locale read failure must not swallow the
        // email — degrade to English and log.
        let tenant_locale = match self.tenants.get_locale(scope).await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    target: "email",
                    error = %e,
                    "tenant locale lookup failed; falling back to English"
                );
                None
            }
        };
        let locale = resolve_recipient_locale(ctx.submitter_locale.as_deref(), tenant_locale.as_deref());
        let rendered = render_for_kind(&brand, &kind, &ctx, locale);

        let from: Mailbox = build_from_mailbox(&self.envelope_from, &brand.sender_display_name)
            .map_err(|e| EmailError::Transport(format!("invalid envelope_from: {e}")))?;
        let to: Mailbox = to_addr
            .parse()
            .map_err(|e| EmailError::Transport(format!("invalid to address: {e}")))?;

        let msg = Message::builder()
            .from(from)
            .to(to)
            .subject(&rendered.subject)
            .singlepart(
                SinglePart::builder()
                    .header(ContentType::TEXT_PLAIN)
                    .body(rendered.body),
            )
            .map_err(|e| EmailError::Transport(e.to_string()))?;

        self.transport
            .send(msg)
            .await
            .map_err(|e| EmailError::Transport(e.to_string()))?;

        tracing::info!(
            target: "email",
            feedback_id = %ctx.feedback_id,
            "feedback notification email dispatched"
        );
        Ok(SendOutcome::Sent)
    }
}

/// Build the `From:` mailbox with the brand's display name. `envelope_from`
/// is a bare `local@host` string; lettre's `Mailbox::new` glues on the
/// display name.
fn build_from_mailbox(envelope_from: &str, display_name: &str) -> anyhow::Result<Mailbox> {
    let addr: lettre::Address = envelope_from.parse()?;
    Ok(Mailbox::new(Some(display_name.to_string()), addr))
}

/// The FR-FBR-37 recipient-language ladder: the submitter's own captured locale
/// → the tenant's Language setting → English.
///
/// **The submitter wins over the tenant on purpose.** The tenant setting is the
/// admin's language; the recipient of these three emails is the *submitter*, who
/// may share none of it. The tenant value is a fallback for the rows that carry
/// no submitter locale, not a default that overrides one.
///
/// Both inputs are stored canonical codes, so an unparseable value means the row
/// predates the shipped-locale table or the language was retired — either way,
/// fall through rather than fail.
#[must_use]
pub fn resolve_recipient_locale(submitter: Option<&str>, tenant: Option<&str>) -> Locale {
    submitter
        .and_then(Locale::parse)
        .or_else(|| tenant.and_then(Locale::parse))
        .unwrap_or(Locale::EN)
}

/// The recipient-language ladder for ACCOUNT mail (verify, password reset):
/// the tenant's stored Language setting → the request's `Accept-Language` →
/// English.
///
/// Different ladder from [`resolve_recipient_locale`] because the recipient is
/// different: account mail goes to the ADMIN, so their own setting leads. The
/// header is the second rung rather than the first because it describes the
/// browser making *this* request, which at signup is the only signal there is,
/// and after signup is weaker evidence than a setting they chose.
#[must_use]
pub fn resolve_account_locale(tenant_locale: Option<&str>, headers: &axum::http::HeaderMap) -> Locale {
    if let Some(l) = tenant_locale.and_then(Locale::parse) {
        return l;
    }
    let candidates = feedbackmonk_i18n::parse_accept_language(headers);
    let refs: Vec<&str> = candidates.iter().map(String::as_str).collect();
    feedbackmonk_i18n::resolve_opt(&refs).unwrap_or(Locale::EN)
}

fn render_for_kind(
    brand: &EmailTenantBrand,
    kind: &EmailKind,
    ctx: &EmailContext,
    locale: Locale,
) -> RenderedEmail {
    match kind {
        EmailKind::Confirmation => render_confirmation(
            brand,
            &ConfirmationContext {
                feedback_id: &ctx.feedback_id,
                body_excerpt: ctx.body_excerpt.as_deref().unwrap_or(""),
            },
            locale,
        ),
        EmailKind::StatusChange {
            from,
            to,
            reason_note,
        } => render_status_change(
            brand,
            &StatusChangeContext {
                feedback_id: &ctx.feedback_id,
                from_status: *from,
                to_status: *to,
                reason_note: reason_note.as_deref(),
            },
            locale,
        ),
        EmailKind::PublicReply { .. } => render_public_reply(
            brand,
            &PublicReplyContext {
                feedback_id: &ctx.feedback_id,
                reply_body: ctx.reply_body.as_deref().unwrap_or(""),
            },
            locale,
        ),
    }
}

/// Set of transitions that produce a submitter-visible status email.
///
/// Per Contract C6 + FR-FBR-09: notify on every transition to a state the
/// submitter benefits from knowing about. Transitions back to `Submitted`
/// (re-open / un-merge) are admin-internal corrections and are NOT
/// emailed.
#[must_use]
pub fn is_submitter_visible_transition(to: FeedbackStatus) -> bool {
    matches!(
        to,
        FeedbackStatus::Triaged
            | FeedbackStatus::InProgress
            | FeedbackStatus::Shipped
            | FeedbackStatus::WontFix
            | FeedbackStatus::Duplicate
    )
}

/// Test-only `EmailNotifier` that records every send call instead of
/// actually sending. Used by the handler tests + integration tests.
#[cfg(test)]
pub struct RecordingEmailNotifier {
    pub sent: std::sync::Mutex<Vec<(EmailKind, EmailContext)>>,
}

#[cfg(test)]
impl Default for RecordingEmailNotifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl RecordingEmailNotifier {
    #[must_use]
    pub fn new() -> Self {
        Self {
            sent: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl EmailNotifier for RecordingEmailNotifier {
    async fn send_email(
        &self,
        _scope: &TenantScope,
        kind: EmailKind,
        ctx: EmailContext,
    ) -> Result<SendOutcome, EmailError> {
        if ctx.submitter_email.is_none() {
            return Ok(SendOutcome::Skipped);
        }
        self.sent.lock().unwrap().push((kind, ctx));
        Ok(SendOutcome::Sent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submitter_visible_includes_all_but_re_open() {
        assert!(is_submitter_visible_transition(FeedbackStatus::Triaged));
        assert!(is_submitter_visible_transition(FeedbackStatus::InProgress));
        assert!(is_submitter_visible_transition(FeedbackStatus::Shipped));
        assert!(is_submitter_visible_transition(FeedbackStatus::WontFix));
        assert!(is_submitter_visible_transition(FeedbackStatus::Duplicate));
        // Re-open / un-merge: admin-internal correction; not emailed.
        assert!(!is_submitter_visible_transition(FeedbackStatus::Submitted));
    }

    #[test]
    fn send_outcome_was_queued() {
        assert!(SendOutcome::Sent.was_queued());
        assert!(!SendOutcome::Skipped.was_queued());
    }

    #[test]
    fn recipient_locale_prefers_the_submitter_then_the_tenant_then_english() {
        let de = Locale::parse("de").unwrap();
        let fr = Locale::parse("fr").unwrap();

        // The submitter's own language wins over the admin's.
        assert_eq!(resolve_recipient_locale(Some("de"), Some("fr")), de);
        // No submitter locale (every pre-FR-FBR-37 row) → the tenant setting.
        assert_eq!(resolve_recipient_locale(None, Some("fr")), fr);
        // Neither → English, which is exactly today's behaviour.
        assert_eq!(resolve_recipient_locale(None, None), Locale::EN);
    }

    #[test]
    fn recipient_locale_falls_through_unshipped_values_instead_of_failing() {
        let fr = Locale::parse("fr").unwrap();
        // A code we no longer ship (or never did) is not a hard error: it is
        // skipped, and the next rung of the ladder answers.
        assert_eq!(resolve_recipient_locale(Some("da"), Some("fr")), fr);
        assert_eq!(resolve_recipient_locale(Some("da"), Some("xx")), Locale::EN);
        // Not a resolver: a stored value is canonical or it is nothing.
        assert_eq!(resolve_recipient_locale(Some("de-AT"), None), Locale::EN);
    }
}
