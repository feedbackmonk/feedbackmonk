//! Email integration -- `Mailer` trait + impls.
//!
//! Two concrete impls:
//!   - `MailpitMailer` -- dev, SMTP on `localhost:1025`, no auth.
//!   - `EnvSmtpMailer` -- prod, SMTP host/port/user/pass from `FEEDBACKMONK_SMTP_*`.
//!
//! The verify-email link target is the SPA admin UI (P1 work). For P0 the
//! link points to a placeholder page; the API's `POST /api/v1/verify-email`
//! is what actually redeems the token.

pub mod env_smtp;
pub mod mailpit;
pub mod send;
pub mod templates;

use async_trait::async_trait;

use feedbackmonk_i18n::Locale;

pub use env_smtp::{EnvSmtpConfig, EnvSmtpMailer};
pub use mailpit::MailpitMailer;
pub use send::{
    EmailContext, EmailError, EmailKind, EmailNotifier, LettreEmailNotifier, SendOutcome,
    is_submitter_visible_transition, outbound_translation, outbound_translation_target,
    resolve_account_locale, resolve_recipient_locale,
};
pub use templates::{
    render_confirmation, render_public_reply, render_status_change, ConfirmationContext,
    PublicReplyContext, RenderedEmail, StatusChangeContext,
};

/// `Mailer` decouples Worker A's handlers from concrete SMTP transports.
/// Test code substitutes an in-memory recorder.
///
/// FR-FBR-37: `locale` is a REQUIRED parameter, not an `Option` with a default.
/// Account mail is the one path where "forgot to pass the language" would be
/// invisible — the English output is indistinguishable from correct — so the
/// type system asks every caller the question instead.
#[async_trait]
pub trait Mailer: Send + Sync {
    /// Send the verify-email message. `to` is the tenant's email. `link` is
    /// the fully-formed `${PUBLIC_URL}/verify-email?token=...` URL. `locale` is
    /// the recipient's language (see `account_locale` in `handlers::signup`).
    async fn send_verify_email(
        &self,
        to: &str,
        link: &str,
        locale: Locale,
    ) -> anyhow::Result<()>;

    /// Send the password-reset message (scrutiny P1-1). `to` is the tenant's
    /// email. `link` is the fully-formed
    /// `${PUBLIC_URL}/reset-password?token=...` URL. Short-lived by design
    /// (the token TTL is `FEEDBACKMONK_RESET_TOKEN_TTL_HOURS`, default 1h).
    async fn send_password_reset_email(
        &self,
        to: &str,
        link: &str,
        locale: Locale,
    ) -> anyhow::Result<()>;
}
