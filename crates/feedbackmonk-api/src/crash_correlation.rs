//! Crash-event correlation worker (GitCellar customer-#1 parity Gap #2).
//!
//! A feedback row may carry a `crash_event_id` (migration 00010) linking it to
//! an external crash event. GitCellar uses **Glitchtip** (a Sentry-API-
//! compatible error tracker). This module *resolves* that id to human-facing
//! crash detail — title / culprit / level / permalink — which GitCellar Desktop
//! renders as a **crash-link banner** above the feedback
//! (contract: `docs/integrations/gitcellar-adoption.md` §"Crash-link banner").
//!
//! ## Pull, not push (Task Zero decision — `channels/decisions.md`)
//!
//! The link direction is feedback → crash: the `crash_event_id` arrives *with*
//! the submission (Desktop captured it from the Glitchtip SDK), so feedbackmonk
//! already holds the key and only needs to resolve it. We therefore **pull** the
//! detail on demand rather than having Glitchtip push events at us. That keeps
//! coupling minimal (one outbound read-only token, no inbound webhook endpoint /
//! auth / replay handling).
//!
//! ## Best-effort, off the submit hot path
//!
//! Correlation is **never** on the submit critical path — the submit handler
//! only *persists* `crash_event_id`; this worker is consulted later (e.g. when
//! the banner is rendered). Every failure mode (network down, auth rejected,
//! 5xx, malformed body) collapses to [`CorrelationOutcome::Unavailable`], and a
//! genuinely-unknown id to [`CorrelationOutcome::NotFound`]. There is **no `Err`
//! surfaced to callers**: a tracker being down is a normal, non-fatal outcome,
//! so "Glitchtip down ≠ submit failure" holds structurally. A bounded HTTP
//! timeout guarantees the worker never blocks indefinitely.

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Resolved crash detail — the shape GitCellar Desktop renders as a crash-link
/// banner. Optional fields degrade gracefully: a tracker that omits `culprit`
/// or `permalink` still yields a usable banner with at least a `title`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashEvent {
    /// The id we correlated on, echoed back (equals `feedback.crash_event_id`).
    pub crash_event_id: String,
    /// Short human title, e.g. `"TypeError: cannot read 'x' of undefined"`.
    pub title: String,
    /// Code location / culprit, e.g. `"save_changes (app/save.rs)"`.
    pub culprit: Option<String>,
    /// Severity as the tracker reports it (`error` | `warning` | `fatal` | …).
    pub level: Option<String>,
    /// Deep link the banner opens in the tracker UI.
    pub permalink: Option<String>,
    /// Last-seen / received timestamp, as the tracker's RFC3339 string.
    pub last_seen: Option<String>,
}

/// Outcome of a best-effort correlation attempt. Deliberately has **no error
/// variant**: callers treat `Unavailable` as "show a graceful unavailable
/// banner" and never propagate a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorrelationOutcome {
    /// Event found and resolved to detail.
    Linked(CrashEvent),
    /// Tracker responded but has no such event (unknown / purged id, or blank).
    NotFound,
    /// Tracker unreachable or returned an unusable response. Banner should show
    /// a "crash details unavailable" state; correctness never depends on this.
    Unavailable,
}

/// Best-effort resolver from a `crash_event_id` to crash detail.
///
/// Implementations MUST map every failure to `Unavailable`/`NotFound`, never
/// panic, and never block indefinitely. A blank id resolves to `NotFound`
/// without any I/O.
#[async_trait]
pub trait CrashCorrelator: Send + Sync {
    async fn correlate(&self, crash_event_id: &str) -> CorrelationOutcome;
}

// ---------------------------------------------------------------------------
// Glitchtip (Sentry-API-compatible) pull-mode correlator
// ---------------------------------------------------------------------------

/// Default per-request timeout. Bounded so a slow/hung tracker degrades to
/// `Unavailable` rather than stalling whatever renders the banner.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Pull-mode correlator backed by a Glitchtip instance. Reads a single event
/// via the Sentry-compatible REST API:
/// `GET {base}/api/0/projects/{org}/{project}/events/{event_id}/`.
pub struct GlitchtipCorrelator {
    http: reqwest::Client,
    /// Base URL, no trailing slash, e.g. `https://glitchtip.gitcellar.com`.
    base_url: String,
    org_slug: String,
    project_slug: String,
    /// Read-only bearer token; held in memory only, never logged.
    token: String,
}

impl GlitchtipCorrelator {
    /// Construct with an explicit configuration. The HTTP client carries a
    /// bounded timeout so correlation can never hang.
    #[must_use]
    pub fn new(
        base_url: impl Into<String>,
        org_slug: impl Into<String>,
        project_slug: impl Into<String>,
        token: impl Into<String>,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            org_slug: org_slug.into(),
            project_slug: project_slug.into(),
            token: token.into(),
        }
    }

    /// Build from env, returning `None` when Glitchtip is not configured (so the
    /// binary runs fine without it — correlation is simply `Unavailable`).
    ///
    /// Vars: `FEEDBACKMONK_GLITCHTIP_URL`, `FEEDBACKMONK_GLITCHTIP_ORG`,
    /// `FEEDBACKMONK_GLITCHTIP_PROJECT`, `FEEDBACKMONK_GLITCHTIP_TOKEN`.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("FEEDBACKMONK_GLITCHTIP_URL").ok()?;
        let org_slug = std::env::var("FEEDBACKMONK_GLITCHTIP_ORG").ok()?;
        let project_slug = std::env::var("FEEDBACKMONK_GLITCHTIP_PROJECT").ok()?;
        let token = std::env::var("FEEDBACKMONK_GLITCHTIP_TOKEN").ok()?;
        if base_url.is_empty() || org_slug.is_empty() || project_slug.is_empty() || token.is_empty()
        {
            return None;
        }
        Some(Self::new(base_url, org_slug, project_slug, token))
    }

    /// The Sentry-compatible single-event URL for a given id.
    fn event_url(&self, crash_event_id: &str) -> String {
        format!(
            "{}/api/0/projects/{}/{}/events/{}/",
            self.base_url, self.org_slug, self.project_slug, crash_event_id
        )
    }

    /// Parse a Glitchtip/Sentry event JSON body into a [`CrashEvent`]. Pulled
    /// out (and pure) so the parse path is unit-testable against a canned
    /// fixture without a live server. Returns `None` on unparseable input.
    ///
    /// Tolerant of shape drift: only a usable `title` is required; everything
    /// else is optional. `level` is read from the top level or from the
    /// `tags` array (`{"key":"level","value":"error"}`), whichever is present.
    fn parse_event(crash_event_id: &str, body: &str) -> Option<CrashEvent> {
        let v: serde_json::Value = serde_json::from_str(body).ok()?;
        let obj = v.as_object()?;

        let str_field = |k: &str| obj.get(k).and_then(|x| x.as_str()).map(str::to_string);

        // Title: prefer explicit `title`, else `metadata.type[: value]`, else
        // `message`. Without any of these the event is not renderable.
        let title = str_field("title")
            .or_else(|| {
                obj.get("metadata")
                    .and_then(|m| m.get("type"))
                    .and_then(|t| t.as_str())
                    .map(str::to_string)
            })
            .or_else(|| str_field("message"))
            .filter(|s| !s.is_empty())?;

        let level = str_field("level").or_else(|| {
            obj.get("tags")
                .and_then(|t| t.as_array())
                .and_then(|tags| {
                    tags.iter().find_map(|tag| {
                        let key = tag.get("key").and_then(|k| k.as_str())?;
                        if key == "level" {
                            tag.get("value").and_then(|x| x.as_str()).map(str::to_string)
                        } else {
                            None
                        }
                    })
                })
        });

        let last_seen = str_field("dateReceived")
            .or_else(|| str_field("dateCreated"))
            .or_else(|| str_field("lastSeen"));

        Some(CrashEvent {
            crash_event_id: crash_event_id.to_string(),
            title,
            culprit: str_field("culprit"),
            level,
            permalink: str_field("permalink"),
            last_seen,
        })
    }
}

#[async_trait]
impl CrashCorrelator for GlitchtipCorrelator {
    async fn correlate(&self, crash_event_id: &str) -> CorrelationOutcome {
        let id = crash_event_id.trim();
        if id.is_empty() {
            return CorrelationOutcome::NotFound;
        }

        let resp = match self
            .http
            .get(self.event_url(id))
            .bearer_auth(&self.token)
            .send()
            .await
        {
            Ok(r) => r,
            // Network failure / timeout / DNS — tracker unreachable.
            Err(_) => return CorrelationOutcome::Unavailable,
        };

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return CorrelationOutcome::NotFound;
        }
        if !status.is_success() {
            // 401/403/5xx — usable banner can't be built; degrade gracefully.
            return CorrelationOutcome::Unavailable;
        }

        let body = match resp.text().await {
            Ok(b) => b,
            Err(_) => return CorrelationOutcome::Unavailable,
        };

        match Self::parse_event(id, &body) {
            Some(ev) => CorrelationOutcome::Linked(ev),
            None => CorrelationOutcome::Unavailable,
        }
    }
}

// ---------------------------------------------------------------------------
// Mock Glitchtip fixture (Task Zero) — testable without a live tracker
// ---------------------------------------------------------------------------

/// In-memory mock correlator (Task Zero "mock Glitchtip"): canned events keyed
/// by `crash_event_id`, plus a switch to simulate the tracker being down. Lets
/// consumers test best-effort handling (Linked / NotFound / Unavailable)
/// without any network. Exposed under `#[cfg(test)]` only — never compiled into
/// the production binary.
#[cfg(test)]
pub mod mock {
    use std::collections::HashMap;

    use super::{CorrelationOutcome, CrashCorrelator, CrashEvent};
    use async_trait::async_trait;

    #[derive(Default)]
    pub struct MockGlitchtipCorrelator {
        events: HashMap<String, CrashEvent>,
        unavailable: bool,
    }

    impl MockGlitchtipCorrelator {
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }

        /// A mock that simulates the tracker being unreachable for every id.
        #[must_use]
        pub fn down() -> Self {
            Self {
                events: HashMap::new(),
                unavailable: true,
            }
        }

        /// Register a canned event (keyed by its `crash_event_id`).
        #[must_use]
        pub fn with_event(mut self, ev: CrashEvent) -> Self {
            self.events.insert(ev.crash_event_id.clone(), ev);
            self
        }
    }

    #[async_trait]
    impl CrashCorrelator for MockGlitchtipCorrelator {
        async fn correlate(&self, crash_event_id: &str) -> CorrelationOutcome {
            if self.unavailable {
                return CorrelationOutcome::Unavailable;
            }
            let id = crash_event_id.trim();
            if id.is_empty() {
                return CorrelationOutcome::NotFound;
            }
            match self.events.get(id) {
                Some(ev) => CorrelationOutcome::Linked(ev.clone()),
                None => CorrelationOutcome::NotFound,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock::MockGlitchtipCorrelator;
    use super::*;

    fn sample_event(id: &str) -> CrashEvent {
        CrashEvent {
            crash_event_id: id.to_string(),
            title: "panic: index out of bounds".to_string(),
            culprit: Some("commit_changes (src/save.rs)".to_string()),
            level: Some("error".to_string()),
            permalink: Some("https://glitchtip.example/x/y/events/".to_string()),
            last_seen: Some("2026-06-02T12:00:00Z".to_string()),
        }
    }

    // ---- Consumer-side behavior against the mock Glitchtip fixture ----

    #[tokio::test]
    async fn mock_returns_linked_for_known_id() {
        let id = "a1b2c3d4e5f60718293a4b5c6d7e8f90";
        let correlator = MockGlitchtipCorrelator::new().with_event(sample_event(id));
        match correlator.correlate(id).await {
            CorrelationOutcome::Linked(ev) => {
                assert_eq!(ev.crash_event_id, id);
                assert_eq!(ev.title, "panic: index out of bounds");
                assert_eq!(ev.level.as_deref(), Some("error"));
            }
            other => panic!("expected Linked, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mock_returns_not_found_for_unknown_id() {
        let correlator =
            MockGlitchtipCorrelator::new().with_event(sample_event("known-id"));
        assert_eq!(
            correlator.correlate("never-seen").await,
            CorrelationOutcome::NotFound
        );
    }

    #[tokio::test]
    async fn mock_blank_id_is_not_found_without_io() {
        let correlator = MockGlitchtipCorrelator::new();
        assert_eq!(correlator.correlate("   ").await, CorrelationOutcome::NotFound);
    }

    #[tokio::test]
    async fn mock_down_is_unavailable_not_an_error() {
        // The load-bearing resilience property: a down tracker is a normal,
        // non-fatal outcome — never an Err that could bubble into submit.
        let correlator = MockGlitchtipCorrelator::down();
        assert_eq!(
            correlator.correlate("any-id").await,
            CorrelationOutcome::Unavailable
        );
    }

    // ---- Real GlitchtipCorrelator parse + URL, no live server ----

    #[test]
    fn glitchtip_event_url_is_sentry_compatible() {
        let c = GlitchtipCorrelator::new(
            "https://glitchtip.gitcellar.com/", // trailing slash trimmed
            "gitcellar",
            "desktop",
            "tok",
        );
        assert_eq!(
            c.event_url("abc123"),
            "https://glitchtip.gitcellar.com/api/0/projects/gitcellar/desktop/events/abc123/"
        );
    }

    #[test]
    fn parse_event_reads_canned_glitchtip_json() {
        // Canned Glitchtip/Sentry event JSON fixture (trimmed to the fields we
        // read), exercising the real parse path with no network.
        let body = r#"{
            "eventID": "a1b2c3d4e5f60718293a4b5c6d7e8f90",
            "title": "TypeError: cannot read 'id' of undefined",
            "culprit": "renderBanner (app/banner.tsx)",
            "permalink": "https://glitchtip.gitcellar.com/gitcellar/desktop/events/a1b2.../",
            "dateReceived": "2026-06-02T11:59:00Z",
            "tags": [
                {"key": "browser", "value": "Firefox"},
                {"key": "level", "value": "fatal"}
            ]
        }"#;
        let ev = GlitchtipCorrelator::parse_event("a1b2c3d4e5f60718293a4b5c6d7e8f90", body)
            .expect("canned fixture must parse");
        assert_eq!(ev.crash_event_id, "a1b2c3d4e5f60718293a4b5c6d7e8f90");
        assert_eq!(ev.title, "TypeError: cannot read 'id' of undefined");
        assert_eq!(ev.culprit.as_deref(), Some("renderBanner (app/banner.tsx)"));
        assert_eq!(ev.level.as_deref(), Some("fatal")); // read from tags[]
        assert_eq!(ev.last_seen.as_deref(), Some("2026-06-02T11:59:00Z"));
        assert!(ev.permalink.is_some());
    }

    #[test]
    fn parse_event_falls_back_to_metadata_type_for_title() {
        let body = r#"{"metadata": {"type": "SegFault"}, "level": "error"}"#;
        let ev = GlitchtipCorrelator::parse_event("id1", body).expect("must parse");
        assert_eq!(ev.title, "SegFault");
        assert_eq!(ev.level.as_deref(), Some("error"));
        assert_eq!(ev.culprit, None);
    }

    #[test]
    fn parse_event_rejects_untitled_or_garbage() {
        // No title / type / message → not renderable → None.
        assert!(GlitchtipCorrelator::parse_event("id", r#"{"culprit": "x"}"#).is_none());
        // Not JSON at all.
        assert!(GlitchtipCorrelator::parse_event("id", "<html>down</html>").is_none());
    }
}
