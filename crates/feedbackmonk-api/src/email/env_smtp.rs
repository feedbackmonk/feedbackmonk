//! Prod SMTP mailer -- credentials and host from `FEEDBACKMONK_SMTP_*` env vars.
//!
//! Env vars consumed at startup (`main.rs::build_state`):
//!   - `FEEDBACKMONK_SMTP_HOST`
//!   - `FEEDBACKMONK_SMTP_PORT` (default 587)
//!   - `FEEDBACKMONK_SMTP_USER`
//!   - `FEEDBACKMONK_SMTP_PASS`
//!   - `FEEDBACKMONK_SMTP_FROM` -- the visible From address
//!   - `FEEDBACKMONK_SMTP_STARTTLS` (`true`/`false`, default `true`)

use async_trait::async_trait;
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

use crate::email::mailpit::build_verify_email;
use crate::email::Mailer;

#[derive(Clone)]
pub struct EnvSmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

pub struct EnvSmtpConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
    pub from: String,
    pub starttls: bool,
}

impl EnvSmtpMailer {
    pub fn new(cfg: EnvSmtpConfig) -> anyhow::Result<Self> {
        let builder = if cfg.starttls {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)?
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host)?
        };
        let transport = builder
            .port(cfg.port)
            .credentials(Credentials::new(cfg.user, cfg.pass))
            .build();
        let from = cfg.from.parse::<Mailbox>()?;
        Ok(Self { transport, from })
    }
}

#[async_trait]
impl Mailer for EnvSmtpMailer {
    async fn send_verify_email(&self, to: &str, link: &str) -> anyhow::Result<()> {
        let to: Mailbox = to.parse()?;
        let msg = build_verify_email(self.from.clone(), to, link)?;
        self.transport.send(msg).await?;
        Ok(())
    }
}
