//! Mailpit-backed SMTP integration test for the email send chokepoint
//! (FR-FBR-09). Boots a `LettreEmailNotifier` pointed at the local Mailpit
//! dev container, triggers a `StatusChange` send, then polls Mailpit's
//! REST API to confirm the message landed with the expected subject /
//! from / to / body shape.
//!
//! Gating: skipped unless `MAILPIT_INTEGRATION_TESTS=1` is set OR a TCP
//! connect to `localhost:1025` succeeds within 200ms. The TCP probe lets
//! local dev runs (where the Mailpit container is up via docker-compose)
//! exercise the test without an explicit env-var opt-in, while CI without
//! the container quietly skips.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use feedbackmonk_api::email::EmailNotifier;
use feedbackmonk_core::{FeedbackId, FeedbackStatus};
use feedbackmonk_repository::{EmailTenantBrand, RepoError, TenantRepo, TenantScope};

// ----- Skip-detection ---------------------------------------------------------

fn mailpit_reachable() -> bool {
    if std::env::var("MAILPIT_INTEGRATION_TESTS").as_deref() == Ok("1") {
        return true;
    }
    // Quick TCP-connect probe. Mailpit defaults to 1025 SMTP / 8025 HTTP.
    std::net::TcpStream::connect_timeout(
        &"127.0.0.1:1025".parse().unwrap(),
        StdDuration::from_millis(200),
    )
    .is_ok()
        && std::net::TcpStream::connect_timeout(
            &"127.0.0.1:8025".parse().unwrap(),
            StdDuration::from_millis(200),
        )
        .is_ok()
}

// ----- Fake TenantRepo --------------------------------------------------------
//
// We only need `get_brand` for the send-chokepoint path under test; every
// other method panics. Keeps the integration test independent of a live
// Postgres (the test exercises the SMTP + template-rendering path only).

struct FakeTenantRepo {
    brand: EmailTenantBrand,
}

#[async_trait::async_trait]
impl feedbackmonk_repository::TenantRepo for FakeTenantRepo {
    async fn create(
        &self,
        _email: &str,
        _password_hash: &str,
    ) -> Result<feedbackmonk_core::Tenant, RepoError> {
        unimplemented!()
    }
    async fn find_by_email(
        &self,
        _email: &str,
    ) -> Result<Option<feedbackmonk_core::Tenant>, RepoError> {
        unimplemented!()
    }
    async fn get(&self, _scope: &TenantScope) -> Result<feedbackmonk_core::Tenant, RepoError> {
        unimplemented!()
    }
    async fn mark_verified(&self, _scope: &TenantScope) -> Result<(), RepoError> {
        unimplemented!()
    }
    async fn scope_for(&self, _id: uuid::Uuid) -> Result<TenantScope, RepoError> {
        unimplemented!()
    }
    async fn get_brand(&self, _scope: &TenantScope) -> Result<EmailTenantBrand, RepoError> {
        Ok(self.brand.clone())
    }
    async fn update_brand(
        &self,
        _scope: &TenantScope,
        _brand: &EmailTenantBrand,
    ) -> Result<(), RepoError> {
        unimplemented!()
    }
    async fn get_widget_brand(
        &self,
        _scope: &TenantScope,
    ) -> Result<feedbackmonk_core::WidgetBrand, RepoError> {
        // Test fixture stub — `feedbackmonk-api` integration tests in this
        // file exercise the email-send path, not the widget-config path.
        // Real defaults live in `SqlxTenantRepo::get_widget_brand`.
        unimplemented!()
    }

    // P3 Stage 1 fixture extension — see
    // docs/test-modifications/20260514-p3-appstate-tier-quotas.md.
    // This file's mailpit integration test exercises the email-send
    // chokepoint; tier-cap reads are out of scope. Stubs are
    // unimplemented! — calling any of these from this test path would
    // be a bug, and the panic is the early-warning surface.
    async fn get_widget_brand_override(
        &self,
        _scope: &TenantScope,
    ) -> Result<feedbackmonk_repository::WidgetBrandOverride, RepoError> {
        unimplemented!()
    }
    async fn set_widget_brand_override(
        &self,
        _scope: &TenantScope,
        _over: &feedbackmonk_repository::WidgetBrandOverride,
    ) -> Result<(), RepoError> {
        unimplemented!()
    }
    async fn set_tier(
        &self,
        _scope: &TenantScope,
        _tier: feedbackmonk_core::Tier,
    ) -> Result<(), RepoError> {
        unimplemented!()
    }
    async fn get_tier(&self, _scope: &TenantScope) -> Result<feedbackmonk_core::Tier, RepoError> {
        unimplemented!()
    }
    async fn count_projects(&self, _scope: &TenantScope) -> Result<i64, RepoError> {
        unimplemented!()
    }
    async fn count_feedback_in_window(
        &self,
        _scope: &TenantScope,
        _window_days: i64,
    ) -> Result<i64, RepoError> {
        unimplemented!()
    }
}

// ----- Synthesise a TenantScope ----------------------------------------------
//
// TenantScope's constructor is pub(crate) inside the repository crate. We
// can't construct one directly from an integration test — but we don't
// need to: the `LettreEmailNotifier::send_email` path passes the scope to
// the (faked) TenantRepo unchanged. We obtain a real TenantScope by
// connecting to the dev DB and seeding a tenant, OR — since the fake
// repo ignores the scope — pass any scope we can construct.
//
// Solution: connect to a temp Postgres just long enough to mint one scope
// via the real `TenantRepo::create + scope_for` path. The Mailpit test
// itself doesn't touch the DB after that.

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn mailpit_status_change_email_lands_with_brand_subject_and_footer() {
    if !mailpit_reachable() {
        eprintln!("Mailpit not reachable; skipping integration test. Set MAILPIT_INTEGRATION_TESTS=1 to force.");
        return;
    }

    let brand = EmailTenantBrand::from_db(
        "MailpitTestBrand".into(),
        "MPT".into(),
        "support@mailpit-test.example".into(),
        Some("https://mailpit-test.example/unsub".into()),
        "— The MailpitTest team".into(),
    );

    let tenants = Arc::new(FakeTenantRepo { brand: brand.clone() });

    // Build a real TenantScope via a temp Postgres connection. We need
    // the database for this one step because `TenantScope::new` is
    // pub(crate) inside the repository crate. The fake repo above will
    // ignore the scope, so this is just a workaround to construct one.
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:dev@localhost:5433/feedbackmonk_dev".into()
    });
    let pool = match sqlx::PgPool::connect(&database_url).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Postgres unreachable; skipping integration test: {e}");
            return;
        }
    };
    let real_tenants = feedbackmonk_repository::SqlxTenantRepo::new(pool);
    let unique_email = format!("mailpit-it-{}@example.com", uuid::Uuid::new_v4());
    let t = match real_tenants.create(&unique_email, "h").await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("could not seed tenant for scope construction; skipping: {e}");
            return;
        }
    };
    let scope = real_tenants.scope_for(t.id).await.unwrap();

    let notifier = feedbackmonk_api::email::LettreEmailNotifier::mailpit(
        tenants as Arc<dyn feedbackmonk_repository::TenantRepo>,
        "localhost",
        1025,
        "no-reply@mailpit-test.example",
    )
    .expect("LettreEmailNotifier::mailpit construction");

    let unique_submitter = format!("recipient-{}@example.com", uuid::Uuid::new_v4());
    let fb_id = FeedbackId::from("FB-MPTEST".to_string());
    let ctx = feedbackmonk_api::email::EmailContext {
        feedback_id: fb_id.clone(),
        submitter_email: Some(unique_submitter.clone()),
        body_excerpt: None,
        reply_body: None,
    };
    let kind = feedbackmonk_api::email::EmailKind::StatusChange {
        from: FeedbackStatus::Submitted,
        to: FeedbackStatus::Triaged,
        reason_note: None,
    };

    let outcome = notifier
        .send_email(&scope, kind, ctx)
        .await
        .expect("send_email succeeded");
    assert!(
        outcome.was_queued(),
        "outcome should report Sent (not Skipped) given a submitter email"
    );

    // Poll Mailpit REST API for the message. Mailpit exposes
    // `GET /api/v1/messages?query=...` for search. We filter by the
    // unique recipient address to avoid races with concurrent runs.
    let client = reqwest::Client::builder()
        .timeout(StdDuration::from_secs(5))
        .build()
        .expect("reqwest client");
    let url = format!(
        "http://127.0.0.1:8025/api/v1/messages?query=to%3A{}",
        urlencoded(&unique_submitter)
    );

    let mut found: Option<serde_json::Value> = None;
    for _ in 0..20 {
        if let Ok(resp) = client.get(&url).send().await {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(messages) = json.get("messages").and_then(|m| m.as_array()) {
                    if !messages.is_empty() {
                        found = Some(messages[0].clone());
                        break;
                    }
                }
            }
        }
        tokio::time::sleep(StdDuration::from_millis(100)).await;
    }

    let msg = found.expect("Mailpit captured the message");
    let subject = msg
        .get("Subject")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        subject.contains("[MPT #FB-MPTEST]"),
        "subject should contain branded prefix + FB id; got: {subject}"
    );

    // Fetch full message body to verify footer rendering.
    let msg_id = msg
        .get("ID")
        .and_then(|v| v.as_str())
        .expect("message ID");
    let body_url = format!("http://127.0.0.1:8025/api/v1/message/{msg_id}");
    let body_json: serde_json::Value = client
        .get(&body_url)
        .send()
        .await
        .expect("fetch full message")
        .json()
        .await
        .expect("parse message JSON");

    let text = body_json
        .get("Text")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(text.contains("MailpitTestBrand"), "body should reference brand name; got: {text}");
    assert!(text.contains("support@mailpit-test.example"), "body should include support email; got: {text}");
    assert!(
        text.contains("Unsubscribe: https://mailpit-test.example/unsub"),
        "body should include unsubscribe line; got: {text}"
    );
}

fn urlencoded(s: &str) -> String {
    // Minimal URL-encoder for the @ symbol. Mailpit's query parser
    // tolerates raw `@` in queries; we still encode it for correctness.
    s.replace('@', "%40")
}
