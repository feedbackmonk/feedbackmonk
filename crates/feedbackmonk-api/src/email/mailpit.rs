//! Mailpit dev mailer -- unauthenticated SMTP on `localhost:1025`.
//!
//! Mailpit accepts everything and exposes the captured messages on its web UI
//! at `http://localhost:8025`. The P0 dev container runs Mailpit; see
//! `docs/operations/LOCAL_DEV.md` for the docker-compose entry.

use async_trait::async_trait;
use lettre::message::{header::ContentType, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::client::Tls;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use feedbackmonk_i18n::{t, Locale};

use crate::email::Mailer;

#[derive(Clone)]
pub struct MailpitMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl MailpitMailer {
    pub fn new(host: &str, port: u16, from: &str) -> anyhow::Result<Self> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
            .port(port)
            .tls(Tls::None)
            .build();
        let from = from.parse::<Mailbox>()?;
        Ok(Self { transport, from })
    }
}

#[async_trait]
impl Mailer for MailpitMailer {
    async fn send_verify_email(&self, to: &str, link: &str, locale: Locale) -> anyhow::Result<()> {
        let to: Mailbox = to.parse()?;
        let msg = build_verify_email(self.from.clone(), to, link, locale)?;
        self.transport.send(msg).await?;
        Ok(())
    }

    async fn send_password_reset_email(
        &self,
        to: &str,
        link: &str,
        locale: Locale,
    ) -> anyhow::Result<()> {
        let to: Mailbox = to.parse()?;
        let msg = build_password_reset_email(self.from.clone(), to, link, locale)?;
        self.transport.send(msg).await?;
        Ok(())
    }
}

pub(crate) fn build_verify_email(
    from: Mailbox,
    to: Mailbox,
    link: &str,
    locale: Locale,
) -> anyhow::Result<Message> {
    let text = verify_email_text(link, locale);
    let html = verify_email_html(link, locale);

    Ok(Message::builder()
        .from(from)
        .to(to)
        .subject(t(locale, "email.account.verify.subject").as_ref())
        .multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(text),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(html),
                ),
        )?)
}

/// Build the admin password-reset email (scrutiny P1-1). Shared by the Mailpit
/// (dev) and env-SMTP (prod) mailers via the same construction path as the
/// verify email. The copy deliberately notes the link is short-lived and that
/// an unrequested reset can be ignored (the token simply expires unused).
pub(crate) fn build_password_reset_email(
    from: Mailbox,
    to: Mailbox,
    link: &str,
    locale: Locale,
) -> anyhow::Result<Message> {
    let text = password_reset_text(link, locale);
    let html = password_reset_html(link, locale);

    Ok(Message::builder()
        .from(from)
        .to(to)
        .subject(t(locale, "email.account.reset.subject").as_ref())
        .multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(text),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(html),
                ),
        )?)
}

// ---------------------------------------------------------------------------
// Body renderers (FR-FBR-37)
// ---------------------------------------------------------------------------
//
// Split out of the `Message` builders so the account emails' TEXT is
// snapshot-testable. They had no test at all before this change, which meant
// nothing would have noticed English drifting — the one class of bug this whole
// change most needed to avoid. Every literal comes from
// `i18n/locales/<code>/email.json`; `feedbackmonk` is a brand name and is never
// translated (C35 rule 8), which is why it sits inside catalog values rather
// than being interpolated in.

fn verify_email_text(link: &str, locale: Locale) -> String {
    format!(
        "{intro}\n\n\
         {instruction}\n{link}\n\n\
         {ignore}",
        intro = t(locale, "email.account.verify.intro"),
        instruction = t(locale, "email.account.verify.textInstruction"),
        ignore = t(locale, "email.account.verify.ignore"),
    )
}

fn verify_email_html(link: &str, locale: Locale) -> String {
    format!(
        "<p>{intro}</p>\
         <p>{instruction} <a href=\"{link}\">{link}</a></p>\
         <p>{ignore}</p>",
        intro = t(locale, "email.account.verify.intro"),
        instruction = t(locale, "email.account.verify.htmlInstruction"),
        ignore = t(locale, "email.account.verify.ignore"),
    )
}

fn password_reset_text(link: &str, locale: Locale) -> String {
    format!(
        "{intro}\n\n\
         {instruction}\n{link}\n\n\
         {expiry}",
        intro = t(locale, "email.account.reset.intro"),
        instruction = t(locale, "email.account.reset.textInstruction"),
        expiry = t(locale, "email.account.reset.expiry"),
    )
}

fn password_reset_html(link: &str, locale: Locale) -> String {
    format!(
        "<p>{intro}</p>\
         <p>{instruction} <a href=\"{link}\">{link}</a></p>\
         <p>{expiry}</p>",
        intro = t(locale, "email.account.reset.intro"),
        instruction = t(locale, "email.account.reset.htmlInstruction"),
        expiry = t(locale, "email.account.reset.expiry"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str = "https://example.test/verify-email?token=abc123";

    // --- English byte-identity ------------------------------------------
    //
    // These snapshots are the regression gate for FR-FBR-37 on the account
    // emails: catalog-driven rendering must reproduce, byte for byte, the text
    // these two emails have sent since P0. The values were captured from the
    // pre-change source before any literal moved into the catalog.

    #[test]
    fn snapshot_verify_email_text_en() {
        insta::assert_snapshot!(verify_email_text(LINK, Locale::EN), @r###"
        Welcome to feedbackmonk.

        Confirm your account by opening this link:
        https://example.test/verify-email?token=abc123

        If you didn't sign up for feedbackmonk, ignore this email.
        "###);
    }

    #[test]
    fn snapshot_verify_email_html_en() {
        insta::assert_snapshot!(verify_email_html(LINK, Locale::EN), @r###"<p>Welcome to feedbackmonk.</p><p>Confirm your account: <a href="https://example.test/verify-email?token=abc123">https://example.test/verify-email?token=abc123</a></p><p>If you didn't sign up for feedbackmonk, ignore this email.</p>"###);
    }

    #[test]
    fn snapshot_password_reset_text_en() {
        insta::assert_snapshot!(password_reset_text(LINK, Locale::EN), @r###"
        A password reset was requested for your feedbackmonk admin account.

        Reset your password by opening this link:
        https://example.test/verify-email?token=abc123

        This link expires shortly. If you didn't request a reset, you can safely ignore this email — your password will not change.
        "###);
    }

    #[test]
    fn snapshot_password_reset_html_en() {
        insta::assert_snapshot!(password_reset_html(LINK, Locale::EN), @r###"<p>A password reset was requested for your feedbackmonk admin account.</p><p>Reset your password: <a href="https://example.test/verify-email?token=abc123">https://example.test/verify-email?token=abc123</a></p><p>This link expires shortly. If you didn't request a reset, you can safely ignore this email — your password will not change.</p>"###);
    }

    // --- structural, locale-independent ---------------------------------

    #[test]
    fn every_shipped_locale_renders_both_account_emails_without_raw_keys() {
        // C35 rule 6 at the point of use: whatever state a catalog is in, the
        // link is present and no raw `email.account.*` key leaks into a message
        // an admin actually receives.
        for locale in Locale::all() {
            for body in [
                verify_email_text(LINK, locale),
                verify_email_html(LINK, locale),
                password_reset_text(LINK, locale),
                password_reset_html(LINK, locale),
            ] {
                assert!(body.contains(LINK), "{locale}: link missing");
                assert!(
                    !body.contains("email.account."),
                    "{locale}: raw catalog key leaked: {body}"
                );
                assert!(!body.contains("{{"), "{locale}: unfilled placeholder: {body}");
            }
        }
    }

    #[test]
    fn subjects_resolve_for_every_shipped_locale() {
        for locale in Locale::all() {
            for key in [
                "email.account.verify.subject",
                "email.account.reset.subject",
            ] {
                let s = t(locale, key);
                assert!(!s.is_empty() && s.as_ref() != key, "{locale}/{key} unresolved");
            }
        }
    }
}
