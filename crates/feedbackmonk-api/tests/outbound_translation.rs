//! FR-FBR-40 — outbound team-authored text reaches the submitter in the
//! submitter's language, and NOTHING about it can stop an email going out.
//!
//! The whole decision is `email::outbound_translation`, which takes the tenant's
//! opt-in as a `bool` rather than reading it — so every branch is exercised here
//! with a fake provider and no database, no SMTP server and no network. The one
//! database read that remains (the `translate_outbound` row) is the same
//! `TenantRepo` method the C38 settings endpoint round-trips, covered by
//! `tests/tenant_locale_settings.rs`.
//!
//! The render half — machine translation ABOVE the original, original always
//! present, separator line from the catalog — is asserted in the `templates.rs`
//! snapshots. Here we assert what the *provider* is asked for and what happens
//! when it misbehaves.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use uuid::Uuid;

use feedbackmonk_api::email::{outbound_translation, EmailContext, EmailKind};
use feedbackmonk_api::translation::{TranslateOutput, TranslationProvider};
use feedbackmonk_core::{FeedbackId, FeedbackStatus};

// ----- A provider that records what it was asked -----------------------------

struct FakeProvider {
    /// `Ok(text)` is returned verbatim as the translation; `Err(msg)` fails.
    outcome: Result<String, String>,
    calls: AtomicUsize,
    last_target: Mutex<Option<String>>,
    last_text: Mutex<Option<String>>,
}

impl FakeProvider {
    fn translating(to: &str) -> Self {
        Self {
            outcome: Ok(to.to_string()),
            calls: AtomicUsize::new(0),
            last_target: Mutex::new(None),
            last_text: Mutex::new(None),
        }
    }
    fn failing() -> Self {
        Self {
            outcome: Err("provider unreachable".into()),
            calls: AtomicUsize::new(0),
            last_target: Mutex::new(None),
            last_text: Mutex::new(None),
        }
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl TranslationProvider for FakeProvider {
    async fn translate(&self, text: &str, target_lang: &str) -> anyhow::Result<TranslateOutput> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_target.lock().unwrap() = Some(target_lang.to_string());
        *self.last_text.lock().unwrap() = Some(text.to_string());
        match &self.outcome {
            Ok(t) => Ok(TranslateOutput {
                translated: t.clone(),
                detected_source_lang: "EN".into(),
            }),
            Err(e) => Err(anyhow::anyhow!("{e}")),
        }
    }
}

// ----- Fixtures ---------------------------------------------------------------

const NOTE: &str = "We couldn't reproduce this without a logged-in user.";
const REPLY: &str = "Thanks — we've reproduced it and pushed a fix.";

fn status_change_with_note() -> EmailKind {
    EmailKind::StatusChange {
        from: FeedbackStatus::Triaged,
        to: FeedbackStatus::WontFix,
        reason_note: Some(NOTE.into()),
    }
}

fn ctx(submitter_locale: Option<&str>) -> EmailContext {
    EmailContext {
        feedback_id: FeedbackId::from("FB-OUT001".to_string()),
        submitter_email: Some("submitter@example.com".into()),
        // The submitter's own words. Present on purpose: no case in this file
        // may ever hand it to a provider.
        body_excerpt: Some("Der Login-Button funktioniert nicht.".into()),
        reply_body: Some(REPLY.into()),
        submitter_locale: submitter_locale.map(str::to_string),
    }
}

// ----- The happy path ---------------------------------------------------------

#[tokio::test]
async fn enabled_and_translatable_translates_the_note_into_the_submitters_language() {
    let p = FakeProvider::translating("Wir konnten das nicht reproduzieren.");
    let got = outbound_translation(Some(&p), true, &status_change_with_note(), &ctx(Some("de"))).await;

    assert_eq!(got.as_deref(), Some("Wir konnten das nicht reproduzieren."));
    assert_eq!(p.calls(), 1);
    // Asked for the C34 table's provider code for `de`, and asked about the
    // TEAM's note — never the submitter's own words.
    assert_eq!(p.last_target.lock().unwrap().as_deref(), Some("DE"));
    assert_eq!(p.last_text.lock().unwrap().as_deref(), Some(NOTE));
}

#[tokio::test]
async fn a_public_reply_body_is_translated_too() {
    let p = FakeProvider::translating("Danke — behoben.");
    let kind = EmailKind::PublicReply { reply_id: Uuid::nil() };
    let got = outbound_translation(Some(&p), true, &kind, &ctx(Some("fr"))).await;

    assert_eq!(got.as_deref(), Some("Danke — behoben."));
    assert_eq!(p.last_target.lock().unwrap().as_deref(), Some("FR"));
    assert_eq!(p.last_text.lock().unwrap().as_deref(), Some(REPLY));
}

// ----- Every way it declines, and none of them is an error --------------------

#[tokio::test]
async fn disabled_for_the_tenant_never_reaches_the_provider() {
    let p = FakeProvider::translating("nope");
    let got = outbound_translation(Some(&p), false, &status_change_with_note(), &ctx(Some("de"))).await;
    assert_eq!(got, None, "opt-out must send the original");
    assert_eq!(p.calls(), 0, "an opted-out tenant must not egress a byte");
}

#[tokio::test]
async fn no_provider_configured_sends_the_original() {
    // The shipped posture (DEC-FBR-IMPL-26): even with the tenant opted in,
    // there is nothing to call.
    let got = outbound_translation(None, true, &status_change_with_note(), &ctx(Some("de"))).await;
    assert_eq!(got, None);
}

#[tokio::test]
async fn an_english_submitter_is_never_translated_for() {
    let p = FakeProvider::translating("nope");
    let got = outbound_translation(Some(&p), true, &status_change_with_note(), &ctx(Some("en"))).await;
    assert_eq!(got, None);
    assert_eq!(p.calls(), 0);
}

#[tokio::test]
async fn no_captured_submitter_locale_is_not_a_request_for_a_translation() {
    // The pre-FR-FBR-37 shape (and every submitter whose browser offered no
    // language we ship). The tenant setting is the ADMIN's language and is not
    // evidence about the submitter.
    let p = FakeProvider::translating("nope");
    let got = outbound_translation(Some(&p), true, &status_change_with_note(), &ctx(None)).await;
    assert_eq!(got, None);
    assert_eq!(p.calls(), 0);
}

#[tokio::test]
async fn a_locale_with_no_provider_code_sends_the_original() {
    // `fa` is one of the five the C34 table gives no provider target
    // (`ga fa ml is si`) — English by design, never an error.
    let p = FakeProvider::translating("nope");
    let got = outbound_translation(Some(&p), true, &status_change_with_note(), &ctx(Some("fa"))).await;
    assert_eq!(got, None);
    assert_eq!(p.calls(), 0, "no provider code means no call at all");
}

#[tokio::test]
async fn a_provider_failure_sends_the_original_and_does_not_error() {
    // The load-bearing guarantee: there is no path from a provider outage to a
    // dropped or delayed notification. The signature has no `Result` to `?`.
    let p = FakeProvider::failing();
    let got = outbound_translation(Some(&p), true, &status_change_with_note(), &ctx(Some("de"))).await;
    assert_eq!(got, None);
    assert_eq!(p.calls(), 1, "it did try");
}

#[tokio::test]
async fn an_empty_or_echoed_translation_is_treated_as_no_translation() {
    for useless in ["", "   ", NOTE] {
        let p = FakeProvider::translating(useless);
        let got =
            outbound_translation(Some(&p), true, &status_change_with_note(), &ctx(Some("de"))).await;
        assert_eq!(got, None, "a {useless:?} translation must not be rendered");
    }
}

#[tokio::test]
async fn a_status_change_with_no_note_has_nothing_team_authored_to_translate() {
    let p = FakeProvider::translating("nope");
    let kind = EmailKind::StatusChange {
        from: FeedbackStatus::Triaged,
        to: FeedbackStatus::Shipped,
        reason_note: None,
    };
    let got = outbound_translation(Some(&p), true, &kind, &ctx(Some("de"))).await;
    assert_eq!(got, None);
    assert_eq!(p.calls(), 0);
}

// ----- Q24: the submitter's own words are never sent anywhere -----------------

#[tokio::test]
async fn the_confirmation_excerpt_is_never_handed_to_a_provider() {
    // `body_excerpt` is the submitter's own submission quoted back to them — it
    // is already in their language, and it is the string Q24 is about. The
    // confirmation email has no team-authored text at all, so a fully enabled,
    // fully translatable configuration still calls nothing.
    let p = FakeProvider::translating("nope");
    let got = outbound_translation(Some(&p), true, &EmailKind::Confirmation, &ctx(Some("de"))).await;
    assert_eq!(got, None);
    assert_eq!(p.calls(), 0, "the submitter's own words must never egress");
}
