//! Shared application state. Constructed once at binary startup and cloned
//! into every handler via axum's `State` extractor.
//!
//! Repository handles are `Arc<dyn Trait>` so tests can swap in fakes without
//! changing handler signatures. The session secret is held inline (`[u8; 32]`)
//! rather than `Arc`-wrapped because the entire `AppState` is already inside
//! an `Arc` once axum hands it to handlers (`State<AppState>` clones cheaply
//! because all fields are `Arc`/`Clone`).

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

use feedbackmonk_anon::{AnonGate, IpGate, LoginGate};
use feedbackmonk_repository::{
    AnalysisSweepRepo, BoardVoteRepo, ClusterRepo, EmailVerificationRepo, FeedbackReplyRepo,
    FeedbackRepo, FeedbackStatusHistoryRepo, ProjectRepo, RecommendationRepo, RoadmapItemRepo,
    RoadmapVoteRepo, RunnerTokenRepo, RunnerTokenRevocationRepo, SigningKeyRepo, SqlxHealthCheck,
    TenantRepo, TierQuotaRepo, WorkOrderEventRepo, WorkOrderRepo,
};

use crate::email::{EmailNotifier, Mailer};
use crate::roadmap_voting_cache::VotingCache;

/// Application context shared across all handlers.
#[derive(Clone)]
pub struct AppState {
    // -- Stage 1 carry-state (Contract C1: repository surface) -------------
    pub pool: PgPool,
    pub tenants: Arc<dyn TenantRepo>,
    pub projects: Arc<dyn ProjectRepo>,
    pub signing_keys: Arc<dyn SigningKeyRepo>,
    pub feedback: Arc<dyn FeedbackRepo>,
    pub feedback_history: Arc<dyn FeedbackStatusHistoryRepo>,
    pub feedback_replies: Arc<dyn FeedbackReplyRepo>,
    pub email_verifications: Arc<dyn EmailVerificationRepo>,

    // -- Worker A: signup / onboarding -------------------------------------
    pub mailer: Arc<dyn Mailer>,
    /// Feedback notification chokepoint (FR-FBR-09). Wraps the lettre
    /// transport + per-call `EmailTenantBrand` resolution. Distinct from
    /// `mailer` (which sends signup-verification emails) so the two paths
    /// can evolve independently — same SMTP transport in dev (Mailpit) but
    /// the brand-resolved chokepoint is what FR-FBR-09 mandates.
    pub email_notifier: Arc<dyn EmailNotifier>,
    /// HMAC key for signed admin-session cookies. 32 bytes, loaded from
    /// `FEEDBACKMONK_SESSION_SECRET` (hex-encoded, 64 hex chars).
    pub session_secret: Arc<[u8; 32]>,
    /// Customer-facing base URL used in verify-email links (no trailing slash).
    pub public_url: Arc<str>,
    /// TTL for email-verification tokens.
    pub verify_token_ttl: Duration,
    // -- Worker B: submission path (FR-FBR-03/05/06) -----------------------
    /// Anonymous-mode rate-limit + cookie dedup. Holds an `Arc<RateLimiter>`
    /// internally, so cloning the gate is cheap. Default quota
    /// `feedbackmonk_anon::DEFAULT_RATE_LIMIT_PER_HOUR = 10`; override via
    /// `FEEDBACKMONK_ANON_RATE_LIMIT_PER_HOUR`.
    pub anon_gate: AnonGate,
    /// Admin-login brute-force + argon2-CPU-DoS throttle (DEC-FBR-IMPL-10).
    /// Keyed by (client-IP, email); per-minute quota. Checked BEFORE the
    /// argon2 verify in `POST /api/v1/login` so an attacker cannot exhaust
    /// CPU with unbounded password guesses. Default quota
    /// `feedbackmonk_anon::DEFAULT_LOGIN_RATE_LIMIT_PER_MIN = 10`; override via
    /// `FEEDBACKMONK_LOGIN_RATE_LIMIT_PER_MIN`.
    pub login_gate: LoginGate,
    /// Class-level per-IP `DoS` ceiling for EVERY public route (submission,
    /// attachments, board, roadmap). Applied as router middleware, keyed on the
    /// resolved client IP alone — the hard floor the cookie-keyed `anon_gate`
    /// sits on top of (scrutiny P0-2). Default
    /// `feedbackmonk_anon::DEFAULT_PUBLIC_RATE_LIMIT_PER_MIN = 120`; override via
    /// `FEEDBACKMONK_PUBLIC_RATE_LIMIT_PER_MIN`.
    pub ip_gate: IpGate,
    /// Number of trusted reverse proxies in front of the app
    /// (`FEEDBACKMONK_TRUSTED_PROXY_HOPS`; 0 = none/secure default, 1 for a
    /// single LB such as Railway). Controls how many `X-Forwarded-For` entries
    /// the IP resolver trusts (P1-2); 0 never trusts the spoofable header.
    pub trusted_proxy_hops: usize,
    /// `iat` clock-skew tolerance for the JWT verifier, in seconds. Read from
    /// `FEEDBACKMONK_JWT_LEEWAY_SECONDS` at startup; default 5s. Only `iat` is
    /// leeway-tolerant — `exp` is strict per Contract C2 invariant 5.
    pub jwt_iat_leeway_seconds: i64,

    // -- P2: roadmap + voting (FR-FBR-11 + FR-FBR-13, Contracts C13–C15) ---
    /// Public + admin roadmap-item repository. Constructor allowlisted as a
    /// structural mirror of `SqlxFeedbackRepo::new`.
    pub roadmap_items: Arc<dyn RoadmapItemRepo>,
    /// Roadmap-vote repository. Carries the 409-on-duplicate hard invariant
    /// (Contract C14) and the retraction-window enforcement.
    pub roadmap_votes: Arc<dyn RoadmapVoteRepo>,
    /// In-process 60s voting aggregate cache (Contract C15). Cloneable
    /// handle backed by `Arc<RwLock<…>>`.
    pub voting_cache: VotingCache,
    /// Public-feedback-board vote repository (Contract C30, PF-BOARD-VOTING-01).
    /// Sibling of `roadmap_votes` keyed on `feedback_id`; carries the same
    /// 409-on-duplicate + retraction-window invariants. Board `vote_count` is a
    /// direct SQL aggregate (D1), so there is no board voting cache.
    pub board_votes: Arc<dyn BoardVoteRepo>,

    // -- Stage 3: health + observability (FR-FBR-18) -----------------------
    /// Wall-clock timestamp captured at binary startup. Retained for internal
    /// uptime accounting and any future ops-token-gated build/health detail.
    /// NOT exposed on the PUBLIC `/health` bodies (scrutiny P2-7: the public
    /// probe carries only the liveness/readiness signal, never a fingerprint).
    pub started_at: DateTime<Utc>,
    /// Database health probe (runs `SELECT 1` via the repository crate so
    /// the `multi-tenant-isolation-check` oracle's raw-SQL ban remains
    /// honored). Used by `/health` + `/health/ready`.
    pub health: SqlxHealthCheck,

    // -- P3 Stage 1: commercial gate (FR-FBR-14, Contract C17) -------------
    /// Tier-cap predicate repository. Every domain-write handler MUST
    /// consult `check_tier_quota(scope, ResourceKind::*)` BEFORE its
    /// first write — the `tier-enforcement-status` Verification Oracle
    /// Probe A enforces this at AST grade, and the schema's
    /// `tenants_tier_check` CHECK constraint enforces canonical values
    /// at the DB layer.
    pub tier_quotas: Arc<dyn TierQuotaRepo>,

    // -- Post-v1: operator surface (DEC-FBR-IMPL-11) -----------------------
    /// Shared-secret bearer token guarding the ops mutation endpoint
    /// (`PATCH /api/v1/ops/tenants/{id}` — tier + widget brand override).
    /// Loaded from `FEEDBACKMONK_OPS_TOKEN`. `None` ⇒ the ops endpoint is
    /// DISABLED (returns 404), so a deployment that does not set the token
    /// exposes no operator surface. Compared constant-time by the `OpsAuth`
    /// extractor. NOT reachable by tenant self-serve — this is the privilege
    /// separation that keeps FR-FBR-14 intact (a Free tenant's own
    /// `AdminSession` cannot flip its tier or strip its footer badge).
    pub ops_token: Option<Arc<str>>,

    // -- P5a: agentic feedback resolution loop (FR-FBR-19..22, Contracts
    //    C22/C23) -----------------------------------------------------------
    /// Feedback-cluster repository (FR-FBR-19). Worker B drives
    /// clustering-on-submit + merge/split on top.
    pub clusters: Arc<dyn ClusterRepo>,
    /// Recommendation repository (FR-FBR-20). The analyst's proposed action
    /// per cluster; Worker A reads it when creating a work order.
    pub recommendations: Arc<dyn RecommendationRepo>,
    /// Analysis-sweep provenance repository (FR-FBR-20).
    pub analysis_sweeps: Arc<dyn AnalysisSweepRepo>,
    /// Work-order repository (FR-FBR-22, Contract C22). The approval state
    /// machine's backing store; Worker A composes transitions with the event
    /// ledger in the same transaction (C22 inv. 3).
    pub work_orders: Arc<dyn WorkOrderRepo>,
    /// APPEND-ONLY work-order event ledger (Contract C22). The substrate the
    /// `approval-gate-enforcement` oracle reads; `has_approved_event` is the
    /// security predicate for C22 inv. 1.
    pub work_order_events: Arc<dyn WorkOrderEventRepo>,

    // -- P5b: runner-token issuance/auth + lifecycle (FR-FBR-24, Contract
    //    C25) ----------------------------------------------------------------
    /// Runner-token lifecycle REGISTRY (admin visibility). Lists issued runner
    /// write-tokens (jti + label + expiry) for the admin UI; NOT consulted by
    /// the verify hot path.
    pub runner_tokens: Arc<dyn RunnerTokenRepo>,
    /// APPEND-ONLY runner-token revocation denylist (the security table).
    /// `verify_runner_token` rejects a token whose `jti` is listed here before
    /// honoring it — the owner's kill-switch for a leaked runner token
    /// (Contract C25). Independent of the registry (revoke-before-register).
    pub runner_token_revocations: Arc<dyn RunnerTokenRevocationRepo>,
}
