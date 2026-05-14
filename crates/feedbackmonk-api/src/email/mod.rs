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

pub use env_smtp::{EnvSmtpConfig, EnvSmtpMailer};
pub use mailpit::MailpitMailer;
pub use send::{
    EmailContext, EmailError, EmailKind, EmailNotifier, LettreEmailNotifier, SendOutcome,
    is_submitter_visible_transition,
};
pub use templates::{
    render_confirmation, render_public_reply, render_status_change, ConfirmationContext,
    PublicReplyContext, RenderedEmail, StatusChangeContext,
};

/// `Mailer` decouples Worker A's handlers from concrete SMTP transports.
/// Test code substitutes an in-memory recorder.
#[async_trait]
pub trait Mailer: Send + Sync {
    /// Send the verify-email message. `to` is the tenant's email. `link` is
    /// the fully-formed `${PUBLIC_URL}/verify-email?token=...` URL.
    async fn send_verify_email(&self, to: &str, link: &str) -> anyhow::Result<()>;
}
