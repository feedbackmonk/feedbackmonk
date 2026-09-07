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
//!
//! ## Outbound machine translation (FR-FBR-40)
//!
//! The team writes a status note or a public reply in their language; when the
//! tenant has opted in (`tenants.translate_outbound`, off by default) and a
//! provider is configured (off by default, DEC-FBR-IMPL-26), the chokepoint
//! translates that ONE string into the submitter's own language and the template
//! renders the translation above the original. Everything about it degrades to
//! "send the original": no provider, no opt-in, no submitter locale, an English
//! recipient, a locale the provider has no target for, a provider error — every
//! one of those sends today's email, unchanged. A translation problem can never
//! delay or drop a notification.
//!
//! This is NOT the FR-FBR-30 inbound pipeline and touches none of its state: the
//! text translated here is the TEAM's, it is translated per-send, and it is
//! never written to a feedback row.

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
use crate::translation::TranslationProvider;

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
    /// FR-FBR-40: the outbound machine-translation provider, or `None`.
    ///
    /// `None` is the default and the shipped posture (DEC-FBR-IMPL-26): with no
    /// provider constructed there is no code path from here to any translation
    /// backend, so an operator who never opted in cannot egress a byte no matter
    /// what a tenant ticks.
    translator: Option<Arc<dyn TranslationProvider>>,
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
            translator: None,
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
            translator: None,
        }
    }

    /// Attach the FR-FBR-40 outbound-translation provider (`main.rs` hands over
    /// the same `Option` `build_translation_provider()` returned).
    ///
    /// A builder rather than a fourth constructor argument: translation is
    /// optional in the strongest sense — it is absent by default, absent in
    /// every test that does not ask for it, and absent in every self-host
    /// deployment that has not opted in. Threading it through both constructors
    /// would make every call site restate `None`.
    #[must_use]
    pub fn with_translator(mut self, translator: Option<Arc<dyn TranslationProvider>>) -> Self {
        self.translator = translator;
        self
    }

    /// FR-FBR-40: the machine translation of this email's team-authored text,
    /// or `None` — which is every case where anything at all is missing.
    ///
    /// **This function can only ever return less than it is asked for; it can
    /// never fail.** Its `None` is not an error path, it is the shipped
    /// behaviour: the recipient reads the original, exactly as before FR-FBR-40.
    /// A provider outage, a locale the provider has no target for, an empty
    /// response — all of them land here as `None` and the email goes out
    /// unchanged. There is deliberately no `Result`: a caller cannot be tempted
    /// to `?` a translation problem into a dropped notification.
    ///
    /// **Gate order is cheapest-first, not the order the four conditions are
    /// listed in.** All four are required, so a conjunction may be evaluated in
    /// any order; putting the three free checks (provider present, recipient
    /// language known and non-English, provider has a target code) ahead of the
    /// `translate_outbound` row read means an operator with translation off pays
    /// zero extra database round-trips per email.
    async fn translate_outbound_text(
        &self,
        scope: &TenantScope,
        kind: &EmailKind,
        ctx: &EmailContext,
    ) -> Option<String> {
        // The three free checks first, so the tenant read below never happens on
        // a deployment with no provider.
        let provider = self.translator.as_ref()?;
        outbound_translation_target(ctx.submitter_locale.as_deref())?;

        // The tenant's opt-in. A read failure is NOT an error for the email —
        // treat it as "not opted in", the safe direction for an egress decision.
        let opted_in = match self.tenants.get_translate_outbound(scope).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    target: "email",
                    error = %e,
                    "translate_outbound lookup failed; sending the original untranslated"
                );
                return None;
            }
        };

        outbound_translation(Some(provider.as_ref()), opted_in, kind, ctx).await
    }
}

/// The whole FR-FBR-40 decision, with the one database read lifted out
/// (`tenant_opted_in`) so it is a pure function of its inputs and every branch
/// is testable without a database, an SMTP server, or a network.
///
/// Returns the machine translation, or `None` meaning "send the original" —
/// which is what happens when ANY of the four conditions is missing, and also
/// whenever the provider misbehaves:
///
/// | condition | `None` because |
/// |---|---|
/// | no provider configured | the default posture (DEC-FBR-IMPL-26) |
/// | `tenant_opted_in == false` | the default per tenant (migration `00032`) |
/// | no captured submitter locale, or it is English | nothing to translate into |
/// | the locale has no provider target code (`ga fa ml is si`) | English by design |
/// | nothing team-authored in this email | nothing to translate |
/// | provider error / empty / unchanged output | best-effort, never blocking |
///
/// There is no `Result` on purpose: a translation problem must not be
/// `?`-able into a dropped notification.
pub async fn outbound_translation(
    provider: Option<&dyn TranslationProvider>,
    tenant_opted_in: bool,
    kind: &EmailKind,
    ctx: &EmailContext,
) -> Option<String> {
    if !tenant_opted_in {
        return None;
    }
    let provider = provider?;
    let target = outbound_translation_target(ctx.submitter_locale.as_deref())?;
    let source = outbound_translatable_text(kind, ctx)?;

    match crate::translation::translate_to(provider, source, target).await {
        Ok(t) => t,
        Err(e) => {
            // No body in the log line — the note/reply is customer text.
            tracing::warn!(
                target: "email",
                feedback_id = %ctx.feedback_id,
                error = %e,
                "outbound translation failed; sending the original untranslated"
            );
            None
        }
    }
}

/// The language to machine-translate outbound team text INTO, or `None`.
///
/// Only the submitter's own captured locale counts, and only when it is not
/// English. That is narrower than [`resolve_recipient_locale`], which also
/// renders the chrome for a recipient we know nothing about, and the difference
/// is deliberate: the tenant setting is the ADMIN's language — the language the
/// note was most likely written in — so treating it as evidence about the
/// submitter would translate text into its own source language and charge a
/// provider for the privilege. No submitter locale is not a request for a
/// translation.
#[must_use]
pub fn outbound_translation_target(submitter_locale: Option<&str>) -> Option<Locale> {
    submitter_locale
        .and_then(Locale::parse)
        .filter(|l| *l != Locale::EN)
}

/// The one string in this email that is TEAM-authored and therefore eligible for
/// outbound translation (FR-FBR-40), or `None`.
///
/// Exactly two qualify: a status-change `reason_note` and a public `reply_body`.
/// The confirmation email's `body_excerpt` is the SUBMITTER'S OWN WORDS quoted
/// back at them — already in their language, and the string Q24 is about — so it
/// is never eligible. Neither is any chrome: that comes from the `email.*`
/// catalog in all 31 languages (FR-FBR-37) and needs no provider.
fn outbound_translatable_text<'a>(kind: &'a EmailKind, ctx: &'a EmailContext) -> Option<&'a str> {
    let text = match kind {
        EmailKind::StatusChange { reason_note, .. } => reason_note.as_deref(),
        EmailKind::PublicReply { .. } => ctx.reply_body.as_deref(),
        EmailKind::Confirmation => None,
    }?;
    (!text.trim().is_empty()).then_some(text)
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
        let translated = self.translate_outbound_text(scope, &kind, &ctx).await;
        let rendered = render_for_kind(&brand, &kind, &ctx, locale, translated.as_deref());

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

/// `translated` is the FR-FBR-40 machine translation of the team-authored text,
/// or `None` for "render exactly what shipped before FR-FBR-40". It is applied
/// to the note or the reply body only — never to the confirmation excerpt, which
/// is the submitter's own words.
fn render_for_kind(
    brand: &EmailTenantBrand,
    kind: &EmailKind,
    ctx: &EmailContext,
    locale: Locale,
    translated: Option<&str>,
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
                translated_reason_note: translated,
            },
            locale,
        ),
        EmailKind::PublicReply { .. } => render_public_reply(
            brand,
            &PublicReplyContext {
                feedback_id: &ctx.feedback_id,
                reply_body: ctx.reply_body.as_deref().unwrap_or(""),
                translated_reply: translated,
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

    fn ctx_with(reply_body: Option<&str>, submitter_locale: Option<&str>) -> EmailContext {
        EmailContext {
            feedback_id: FeedbackId::from("FB-ABC123".to_string()),
            submitter_email: Some("s@example.com".into()),
            body_excerpt: Some("the submitter's own words".into()),
            reply_body: reply_body.map(str::to_string),
            submitter_locale: submitter_locale.map(str::to_string),
        }
    }

    #[test]
    fn outbound_target_is_the_submitters_own_non_english_locale() {
        let de = Locale::parse("de").unwrap();
        assert_eq!(outbound_translation_target(Some("de")), Some(de));
        // English recipient: nothing to translate INTO.
        assert_eq!(outbound_translation_target(Some("en")), None);
        // No captured locale is not a request for a translation — the tenant
        // setting is the ADMIN's language, i.e. the note's own source language.
        assert_eq!(outbound_translation_target(None), None);
        // A value we do not ship is not a language we can target.
        assert_eq!(outbound_translation_target(Some("da")), None);
    }

    #[test]
    fn only_team_authored_text_is_eligible() {
        let note = EmailKind::StatusChange {
            from: FeedbackStatus::Triaged,
            to: FeedbackStatus::InProgress,
            reason_note: Some("We've started.".into()),
            };
        let ctx = ctx_with(None, Some("de"));
        assert_eq!(outbound_translatable_text(&note, &ctx), Some("We've started."));

        let reply = EmailKind::PublicReply { reply_id: Uuid::nil() };
        let ctx = ctx_with(Some("Fixed in 2.1."), Some("de"));
        assert_eq!(outbound_translatable_text(&reply, &ctx), Some("Fixed in 2.1."));

        // The confirmation excerpt is the SUBMITTER's own words — the string Q24
        // is about. Never eligible, whatever else is set.
        assert_eq!(outbound_translatable_text(&EmailKind::Confirmation, &ctx), None);
    }

    #[test]
    fn nothing_to_translate_when_the_team_wrote_nothing() {
        let ctx = ctx_with(None, Some("de"));
        let no_note = EmailKind::StatusChange {
            from: FeedbackStatus::Triaged,
            to: FeedbackStatus::Shipped,
            reason_note: None,
        };
        assert_eq!(outbound_translatable_text(&no_note, &ctx), None);
        let blank_note = EmailKind::StatusChange {
            from: FeedbackStatus::Triaged,
            to: FeedbackStatus::Shipped,
            reason_note: Some("   \n".into()),
        };
        assert_eq!(outbound_translatable_text(&blank_note, &ctx), None);
        // A PublicReply whose body never made it into the context.
        let reply = EmailKind::PublicReply { reply_id: Uuid::nil() };
        assert_eq!(outbound_translatable_text(&reply, &ctx_with(None, Some("de"))), None);
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
