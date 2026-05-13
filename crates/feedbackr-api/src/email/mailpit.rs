//! Mailpit dev mailer -- unauthenticated SMTP on `localhost:1025`.
//!
//! Mailpit accepts everything and exposes the captured messages on its web UI
//! at `http://localhost:8025`. The P0 dev container runs Mailpit; see
//! `docs/operations/LOCAL_DEV.md` for the docker-compose entry.

use async_trait::async_trait;
use lettre::message::{header::ContentType, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::client::Tls;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

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
    async fn send_verify_email(&self, to: &str, link: &str) -> anyhow::Result<()> {
        let to: Mailbox = to.parse()?;
        let msg = build_verify_email(self.from.clone(), to, link)?;
        self.transport.send(msg).await?;
        Ok(())
    }
}

pub(crate) fn build_verify_email(
    from: Mailbox,
    to: Mailbox,
    link: &str,
) -> anyhow::Result<Message> {
    let text = format!(
        "Welcome to Feedbackr.\n\n\
         Confirm your account by opening this link:\n{link}\n\n\
         If you didn't sign up for Feedbackr, ignore this email."
    );
    let html = format!(
        "<p>Welcome to Feedbackr.</p>\
         <p>Confirm your account: <a href=\"{link}\">{link}</a></p>\
         <p>If you didn't sign up for Feedbackr, ignore this email.</p>"
    );

    Ok(Message::builder()
        .from(from)
        .to(to)
        .subject("Confirm your Feedbackr account")
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
