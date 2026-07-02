//! `feedbackmonk-anon` -- anonymous-mode rate-limit + cookie dedup for the
//! public submission endpoint (FR-FBR-06).
//!
//! ## Surface
//!
//! - `AnonGate` -- in-memory keyed rate limiter (governor-backed). Keys are
//!   `(anon_token_hash, project_id)`; quota defaults to
//!   `DEFAULT_RATE_LIMIT_PER_HOUR = 10` (configurable via
//!   `FEEDBACKMONK_ANON_RATE_LIMIT_PER_HOUR`).
//! - `AnonGate::token_hash` -- BLAKE3 of
//!   `b"feedbackmonk-anon-v1" || ip || 0 || cookie || 0 || project_id`. Version
//!   prefix enables hash-domain rotation without ambiguity.
//! - `AnonGate::mint_cookie` -- 16 random bytes, base64url-no-pad. Used by
//!   the handler when no `X-Feedbackmonk-Anon-Cookie` header arrives.
//!
//! Cookie integrity: the hash binds (cookie, IP, project_id) together, so a
//! cookie-forging attacker on a different IP gets a different hash bucket
//! and cannot exhaust someone else's rate budget. No HMAC is required on
//! the cookie itself for the P0 threat model (see GUIDE §Key Implementation
//! Notes).

#![deny(unsafe_code)]

use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Arc;

use governor::clock::{Clock, DefaultClock};
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use thiserror::Error;
use uuid::Uuid;

/// Domain-separation prefix included in every `token_hash`. If the hash
/// algorithm or input layout is ever changed, increment the version
/// suffix; this keeps old and new hashes in disjoint domains.
pub const HASH_DOMAIN_PREFIX: &[u8] = b"feedbackmonk-anon-v1";

/// Default per-(anon_hash, project) submissions per hour. Overridable by
/// `FEEDBACKMONK_ANON_RATE_LIMIT_PER_HOUR` env var (handler-level wiring).
pub const DEFAULT_RATE_LIMIT_PER_HOUR: u32 = 10;

/// Length of the random cookie value (before base64 encoding).
pub const ANON_COOKIE_BYTES: usize = 16;

/// HTTP header that conveys the anonymous-mode cookie. If absent, the
/// handler mints a fresh one and emits `Set-Cookie` in the response.
pub const ANON_COOKIE_HEADER: &str = "X-Feedbackmonk-Anon-Cookie";

/// Returned on rate-limit exceedance. `retry_after_seconds` is suitable for
/// the HTTP `Retry-After` header.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RateLimitError {
    #[error("rate limit exceeded; retry in {retry_after_seconds}s")]
    Exceeded { retry_after_seconds: u64 },
}

/// Successful gate result -- carries the hash + project for downstream
/// repository writes (no behavior, just the typed bundle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnonAccepted {
    pub token_hash: [u8; 32],
    pub project_id: Uuid,
}

type GovKey = ([u8; 32], Uuid);

/// In-memory keyed rate limiter. Holds an `Arc<RateLimiter>` so the gate
/// can be cheaply cloned into request handlers (which axum requires for
/// `State<AppState>` cloning).
#[derive(Clone)]
pub struct AnonGate {
    limiter: Arc<RateLimiter<GovKey, DefaultKeyedStateStore<GovKey>, DefaultClock>>,
    clock: DefaultClock,
    quota_per_hour: NonZeroU32,
}

impl AnonGate {
    /// Build a gate with the given per-(anon_hash, project) hourly quota.
    #[must_use]
    pub fn new(submissions_per_hour: NonZeroU32) -> Self {
        let quota = Quota::per_hour(submissions_per_hour);
        let limiter = RateLimiter::keyed(quota);
        Self {
            limiter: Arc::new(limiter),
            clock: DefaultClock::default(),
            quota_per_hour: submissions_per_hour,
        }
    }

    /// Build a gate at the documented P0 default (10/hr).
    #[must_use]
    pub fn with_default_quota() -> Self {
        // SAFETY: 10 != 0 statically.
        Self::new(NonZeroU32::new(DEFAULT_RATE_LIMIT_PER_HOUR).expect("non-zero default"))
    }

    /// The configured quota; exposed for telemetry / debug.
    #[must_use]
    pub fn quota_per_hour(&self) -> NonZeroU32 {
        self.quota_per_hour
    }

    /// Compute the anonymous-mode token hash. Pure / deterministic:
    /// same inputs -> same hash. Different `project_id` for the same
    /// (ip, cookie) -> different hash (per-project rate-limit isolation).
    #[must_use]
    pub fn token_hash(client_ip: &str, cookie: &str, project_id: Uuid) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(HASH_DOMAIN_PREFIX);
        hasher.update(client_ip.as_bytes());
        hasher.update(b"\0");
        hasher.update(cookie.as_bytes());
        hasher.update(b"\0");
        hasher.update(project_id.as_bytes());
        *hasher.finalize().as_bytes()
    }

    /// Check + decrement the rate budget for `(token_hash, project_id)`.
    /// On exceedance, `retry_after_seconds` indicates the soonest the
    /// caller may try again.
    pub fn check(
        &self,
        token_hash: &[u8; 32],
        project_id: Uuid,
    ) -> Result<AnonAccepted, RateLimitError> {
        let key: GovKey = (*token_hash, project_id);
        match self.limiter.check_key(&key) {
            Ok(()) => Ok(AnonAccepted {
                token_hash: *token_hash,
                project_id,
            }),
            Err(not_until) => {
                let wait = not_until.wait_time_from(self.clock.now());
                // Round up so Retry-After is never "0 seconds" when an actual
                // wait is required (sub-second waits would be silently lost).
                let retry_after_seconds = wait.as_secs().max(1);
                Err(RateLimitError::Exceeded {
                    retry_after_seconds,
                })
            }
        }
    }

    /// Mint a fresh opaque anonymous-mode cookie value (16 random bytes,
    /// base64url-no-pad, ~22 chars). Used by handlers when no
    /// `X-Feedbackmonk-Anon-Cookie` header is present on the request.
    #[must_use]
    pub fn mint_cookie() -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        use rand::RngCore;
        let mut bytes = [0u8; ANON_COOKIE_BYTES];
        rand::thread_rng().fill_bytes(&mut bytes);
        URL_SAFE_NO_PAD.encode(bytes)
    }
}

// ---------------------------------------------------------------------------
// LoginGate -- brute-force + argon2-CPU-DoS throttle for admin login
// (POST /api/v1/login, DEC-FBR-IMPL-10). Sibling of AnonGate: same governor
// substrate, but keyed by (client_ip, email) with NO project coupling, and a
// per-MINUTE quota (login attempts are far rarer than feedback submissions).
// ---------------------------------------------------------------------------

/// Domain-separation prefix for every `LoginGate::key_hash`. Disjoint from
/// `HASH_DOMAIN_PREFIX` so a login bucket can never collide with an anon
/// submission bucket. Bump the version suffix if the input layout changes.
pub const LOGIN_HASH_DOMAIN_PREFIX: &[u8] = b"feedbackmonk-login-v1";

/// Default admin-login attempts per minute, per (client-IP, email) bucket.
/// Overridable via `FEEDBACKMONK_LOGIN_RATE_LIMIT_PER_MIN`. Chosen low: a
/// legitimate human login needs 1-2 attempts; 10/min leaves generous slack
/// while capping both password brute-force and the argon2 CPU-DoS vector
/// (each attempt below the gate costs a full argon2id verify).
pub const DEFAULT_LOGIN_RATE_LIMIT_PER_MIN: u32 = 10;

/// LoginGate key: BLAKE3 of the domain prefix + client IP + email. No project
/// dimension (admin login is tenant-wide, not project-scoped).
type LoginKey = [u8; 32];

/// In-memory keyed rate limiter for admin login. `Arc`-backed so the gate can
/// be cheaply cloned into `AppState` like `AnonGate`.
#[derive(Clone)]
pub struct LoginGate {
    limiter: Arc<RateLimiter<LoginKey, DefaultKeyedStateStore<LoginKey>, DefaultClock>>,
    clock: DefaultClock,
    quota_per_min: NonZeroU32,
}

impl LoginGate {
    /// Build a gate with the given per-(IP, email) per-minute attempt quota.
    #[must_use]
    pub fn new(attempts_per_min: NonZeroU32) -> Self {
        let quota = Quota::per_minute(attempts_per_min);
        let limiter = RateLimiter::keyed(quota);
        Self {
            limiter: Arc::new(limiter),
            clock: DefaultClock::default(),
            quota_per_min: attempts_per_min,
        }
    }

    /// Build a gate at the documented default (10/min).
    #[must_use]
    pub fn with_default_quota() -> Self {
        // SAFETY: 10 != 0 statically.
        Self::new(NonZeroU32::new(DEFAULT_LOGIN_RATE_LIMIT_PER_MIN).expect("non-zero default"))
    }

    /// The configured quota; exposed for telemetry / debug.
    #[must_use]
    pub fn quota_per_min(&self) -> NonZeroU32 {
        self.quota_per_min
    }

    /// Compute the login rate-limit bucket key. Pure / deterministic. Callers
    /// MUST pass an already-normalized (trimmed + lowercased) email so the
    /// bucket and the DB lookup agree. Same (ip, email) -> same bucket.
    #[must_use]
    pub fn key_hash(client_ip: &str, normalized_email: &str) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(LOGIN_HASH_DOMAIN_PREFIX);
        hasher.update(client_ip.as_bytes());
        hasher.update(b"\0");
        hasher.update(normalized_email.as_bytes());
        *hasher.finalize().as_bytes()
    }

    /// Compute the ACCOUNT-level (email-only, IP-independent) bucket key
    /// (scrutiny P2-19). The per-(ip,email) key above throttles single-source
    /// brute-force; this second bucket caps DISTRIBUTED password-spray — many
    /// IPs against ONE account — which would otherwise get a fresh per-IP budget
    /// each. Domain-separated from `key_hash` so the two buckets never collide.
    /// Same quota (per-minute) as the (ip,email) bucket: generous for a human
    /// (1-2 attempts) while capping total cross-IP guesses at one account.
    #[must_use]
    pub fn account_key_hash(normalized_email: &str) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(LOGIN_HASH_DOMAIN_PREFIX);
        hasher.update(b"account\0"); // domain-separate from the (ip,email) layout
        hasher.update(normalized_email.as_bytes());
        *hasher.finalize().as_bytes()
    }

    /// Check + decrement the attempt budget for `key`. On exceedance,
    /// `retry_after_seconds` indicates the soonest the caller may retry.
    pub fn check(&self, key: &[u8; 32]) -> Result<(), RateLimitError> {
        match self.limiter.check_key(key) {
            Ok(()) => Ok(()),
            Err(not_until) => {
                let wait = not_until.wait_time_from(self.clock.now());
                let retry_after_seconds = wait.as_secs().max(1);
                Err(RateLimitError::Exceeded {
                    retry_after_seconds,
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// IpGate -- class-level per-IP DoS ceiling for EVERY public route
// (submission, attachments, board, roadmap). Sibling of AnonGate/LoginGate:
// same governor substrate, keyed on the resolved client IP ALONE (no cookie,
// no email) and applied as a router-level middleware, not per-handler.
//
// WHY (scrutiny 2026-07-01, P0-2): the fine-grained AnonGate keys on a
// client-CONTROLLED cookie, so rotating the cookie mints unlimited budgets,
// and the authenticated (Bearer-present) path had NO limiter at all. IpGate is
// the hard floor the cookie gate sits on top of: an attacker can vary the
// cookie freely, but every request from one IP still shares one coarse ceiling.
// The IP is resolved via `resolve_client_ip` (trusted-proxy aware) so it is not
// the raw TCP peer that collapses to the load balancer behind Railway (P1-2).
// ---------------------------------------------------------------------------

/// Domain-separation prefix for `IpGate` bucket keys. Disjoint from the anon +
/// login prefixes so an IP bucket can never collide with either.
pub const IP_HASH_DOMAIN_PREFIX: &[u8] = b"feedbackmonk-ip-v1";

/// Default per-IP requests per minute across ALL public routes. Coarse by
/// design: a legitimate human browsing the board + voting + submitting stays
/// far below it; it exists to cap floods, not to shape normal traffic.
/// Overridable via `FEEDBACKMONK_PUBLIC_RATE_LIMIT_PER_MIN`.
pub const DEFAULT_PUBLIC_RATE_LIMIT_PER_MIN: u32 = 120;

type IpKey = [u8; 32];

/// In-memory keyed per-IP rate limiter. `Arc`-backed so it clones cheaply into
/// the middleware state like `AnonGate`.
#[derive(Clone)]
pub struct IpGate {
    limiter: Arc<RateLimiter<IpKey, DefaultKeyedStateStore<IpKey>, DefaultClock>>,
    clock: DefaultClock,
    quota_per_min: NonZeroU32,
}

impl IpGate {
    /// Build a gate with the given per-IP per-minute request quota.
    #[must_use]
    pub fn new(requests_per_min: NonZeroU32) -> Self {
        let quota = Quota::per_minute(requests_per_min);
        Self {
            limiter: Arc::new(RateLimiter::keyed(quota)),
            clock: DefaultClock::default(),
            quota_per_min: requests_per_min,
        }
    }

    /// Build a gate at the documented default (120/min).
    #[must_use]
    pub fn with_default_quota() -> Self {
        // SAFETY: 120 != 0 statically.
        Self::new(NonZeroU32::new(DEFAULT_PUBLIC_RATE_LIMIT_PER_MIN).expect("non-zero default"))
    }

    /// The configured quota; exposed for telemetry / debug.
    #[must_use]
    pub fn quota_per_min(&self) -> NonZeroU32 {
        self.quota_per_min
    }

    /// Bucket key for a client IP: BLAKE3 of the domain prefix + the IP's
    /// canonical string form. Hashing (rather than keying on the raw IP) keeps
    /// the bucket domain-separated and fixed-width.
    #[must_use]
    pub fn key_hash(ip: IpAddr) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(IP_HASH_DOMAIN_PREFIX);
        hasher.update(ip.to_string().as_bytes());
        *hasher.finalize().as_bytes()
    }

    /// Check + decrement the per-IP budget. On exceedance, `retry_after_seconds`
    /// indicates the soonest the caller may retry.
    pub fn check(&self, ip: IpAddr) -> Result<(), RateLimitError> {
        let key = Self::key_hash(ip);
        match self.limiter.check_key(&key) {
            Ok(()) => Ok(()),
            Err(not_until) => {
                let wait = not_until.wait_time_from(self.clock.now());
                let retry_after_seconds = wait.as_secs().max(1);
                Err(RateLimitError::Exceeded { retry_after_seconds })
            }
        }
    }
}

/// Resolve the real client IP for rate limiting, trusted-proxy aware (P1-2).
///
/// `trusted_hops` is the number of trusted reverse proxies in front of the app
/// (`FEEDBACKMONK_TRUSTED_PROXY_HOPS`; 0 = none, the secure default; 1 for a
/// single LB such as Railway). Only that many entries are trusted from the
/// RIGHT of `X-Forwarded-For` (proxies append; the rightmost is the nearest
/// proxy). We strip `trusted_hops` trusted entries and take the next one as the
/// client. `X-Forwarded-For` is NEVER trusted when `trusted_hops == 0` (it is
/// client-spoofable), and any shortfall falls back to the TCP `peer`.
#[must_use]
pub fn resolve_client_ip(xff: Option<&str>, peer: IpAddr, trusted_hops: usize) -> IpAddr {
    if trusted_hops == 0 {
        return peer; // never trust a spoofable header without a known proxy
    }
    let Some(raw) = xff else { return peer };
    let entries: Vec<IpAddr> = raw
        .split(',')
        .filter_map(|s| s.trim().parse::<IpAddr>().ok())
        .collect();
    if entries.is_empty() {
        return peer;
    }
    // Strip `trusted_hops` entries from the right; the entry before them is the
    // client as seen by the outermost trusted proxy. If the chain is shorter
    // than we trust (a client that forged fewer entries than the real proxy
    // count would append), fall back to the peer rather than trusting a forged
    // leftmost value.
    match entries.len().checked_sub(trusted_hops + 1) {
        Some(idx) => entries[idx],
        None => peer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_hash_is_deterministic_for_same_inputs() {
        let pid = Uuid::from_u128(0x1234);
        let h1 = AnonGate::token_hash("10.0.0.1", "cookie-xyz", pid);
        let h2 = AnonGate::token_hash("10.0.0.1", "cookie-xyz", pid);
        assert_eq!(h1, h2);
    }

    #[test]
    fn token_hash_differs_per_project_id() {
        let pid1 = Uuid::from_u128(0x1234);
        let pid2 = Uuid::from_u128(0x5678);
        let h1 = AnonGate::token_hash("10.0.0.1", "cookie-xyz", pid1);
        let h2 = AnonGate::token_hash("10.0.0.1", "cookie-xyz", pid2);
        assert_ne!(h1, h2, "per-project hash isolation");
    }

    #[test]
    fn token_hash_differs_per_ip() {
        let pid = Uuid::from_u128(0x1234);
        let h1 = AnonGate::token_hash("10.0.0.1", "cookie-xyz", pid);
        let h2 = AnonGate::token_hash("10.0.0.2", "cookie-xyz", pid);
        assert_ne!(h1, h2);
    }

    #[test]
    fn token_hash_differs_per_cookie() {
        let pid = Uuid::from_u128(0x1234);
        let h1 = AnonGate::token_hash("10.0.0.1", "cookie-A", pid);
        let h2 = AnonGate::token_hash("10.0.0.1", "cookie-B", pid);
        assert_ne!(h1, h2);
    }

    #[test]
    fn token_hash_is_domain_separated_against_simple_concat() {
        // Sanity-check: a hash of just (ip || cookie || project_id) without
        // the domain prefix produces a DIFFERENT value than ours. Guards
        // against accidental loss of the prefix in refactoring.
        let pid = Uuid::from_u128(0x1234);
        let our = AnonGate::token_hash("10.0.0.1", "cookie", pid);

        let mut bare = blake3::Hasher::new();
        bare.update(b"10.0.0.1");
        bare.update(b"\0");
        bare.update(b"cookie");
        bare.update(b"\0");
        bare.update(pid.as_bytes());
        let bare = *bare.finalize().as_bytes();
        assert_ne!(our, bare, "prefix must change hash");
    }

    #[test]
    fn mint_cookie_yields_url_safe_string_of_expected_length() {
        let c = AnonGate::mint_cookie();
        // 16 bytes base64url-no-pad -> 22 chars.
        assert_eq!(c.len(), 22);
        assert!(c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
    }

    #[test]
    fn mint_cookie_is_random_per_call() {
        let a = AnonGate::mint_cookie();
        let b = AnonGate::mint_cookie();
        assert_ne!(a, b);
    }

    #[test]
    fn rate_limit_first_n_pass_then_11th_fails() {
        // Burst of 10 succeeds; 11th fails immediately (governor leaky
        // bucket starts full). Quota refills at 10/hour = 1 per 6min, so
        // the 11th cannot retry for ~6 minutes.
        let gate = AnonGate::with_default_quota();
        let hash = AnonGate::token_hash("ip", "c", Uuid::nil());
        let pid = Uuid::nil();
        for i in 0..10 {
            gate.check(&hash, pid).unwrap_or_else(|e| panic!("call {i} should pass: {e:?}"));
        }
        let err = gate.check(&hash, pid).unwrap_err();
        match err {
            RateLimitError::Exceeded {
                retry_after_seconds,
            } => assert!(retry_after_seconds >= 1, "retry_after should be at least 1s"),
        }
    }

    #[test]
    fn rate_limit_buckets_are_per_project() {
        // Exhausting (hash, project_a) does NOT affect (hash, project_b).
        let gate = AnonGate::with_default_quota();
        let hash = AnonGate::token_hash("ip", "c", Uuid::nil());
        let pa = Uuid::from_u128(0xAA);
        let pb = Uuid::from_u128(0xBB);
        for _ in 0..10 {
            gate.check(&hash, pa).unwrap();
        }
        assert!(gate.check(&hash, pa).is_err());
        // Same hash, different project: budget is independent.
        gate.check(&hash, pb).expect("per-project isolation");
    }

    #[test]
    fn rate_limit_buckets_are_per_hash() {
        // Exhausting (hash_a, project) does NOT affect (hash_b, project).
        let gate = AnonGate::with_default_quota();
        let pid = Uuid::from_u128(0xAA);
        let ha = AnonGate::token_hash("ip-a", "c", pid);
        let hb = AnonGate::token_hash("ip-b", "c", pid);
        for _ in 0..10 {
            gate.check(&ha, pid).unwrap();
        }
        assert!(gate.check(&ha, pid).is_err());
        gate.check(&hb, pid).expect("per-hash isolation");
    }

    #[test]
    fn anon_accepted_carries_inputs_through() {
        let gate = AnonGate::with_default_quota();
        let pid = Uuid::from_u128(0xFEED);
        let h = AnonGate::token_hash("ip", "c", pid);
        let accepted = gate.check(&h, pid).unwrap();
        assert_eq!(accepted.token_hash, h);
        assert_eq!(accepted.project_id, pid);
    }

    // ----- LoginGate (DEC-FBR-IMPL-10) -----

    #[test]
    fn login_key_hash_is_deterministic_and_input_sensitive() {
        let a = LoginGate::key_hash("10.0.0.1", "user@example.com");
        let b = LoginGate::key_hash("10.0.0.1", "user@example.com");
        assert_eq!(a, b, "same inputs -> same bucket");
        // Different IP, different email -> different buckets.
        assert_ne!(a, LoginGate::key_hash("10.0.0.2", "user@example.com"));
        assert_ne!(a, LoginGate::key_hash("10.0.0.1", "other@example.com"));
        // Disjoint from the anon hash domain even with coincidentally-similar
        // inputs (domain-separation prefix differs).
        assert_ne!(a, AnonGate::token_hash("10.0.0.1", "user@example.com", Uuid::nil()));
    }

    #[test]
    fn login_rate_limit_trips_after_quota() {
        // Default 10/min: a burst of 10 passes, the 11th is throttled.
        let gate = LoginGate::with_default_quota();
        let key = LoginGate::key_hash("1.2.3.4", "victim@example.com");
        for i in 0..10 {
            gate.check(&key).unwrap_or_else(|e| panic!("attempt {i} should pass: {e:?}"));
        }
        match gate.check(&key).unwrap_err() {
            RateLimitError::Exceeded { retry_after_seconds } => {
                assert!(retry_after_seconds >= 1);
            }
        }
    }

    #[test]
    fn account_key_is_ip_independent_and_domain_separated() {
        // Same account from two different IPs → SAME account bucket (so
        // distributed spray is capped), and disjoint from the (ip,email) bucket.
        let a = LoginGate::account_key_hash("victim@example.com");
        let b = LoginGate::account_key_hash("victim@example.com");
        assert_eq!(a, b, "account bucket is IP-independent");
        assert_ne!(a, LoginGate::account_key_hash("other@example.com"));
        assert_ne!(
            a,
            LoginGate::key_hash("1.2.3.4", "victim@example.com"),
            "account bucket must not collide with any (ip,email) bucket"
        );
    }

    #[test]
    fn account_bucket_caps_distributed_spray() {
        // A burst of 10 against one account trips it regardless of source IP
        // (the caller keys on account_key_hash, not the client IP).
        let gate = LoginGate::with_default_quota();
        let acct = LoginGate::account_key_hash("victim@example.com");
        for i in 0..10 {
            gate.check(&acct).unwrap_or_else(|e| panic!("attempt {i} should pass: {e:?}"));
        }
        assert!(gate.check(&acct).is_err(), "11th cross-IP attempt on the account is throttled");
    }

    #[test]
    fn login_rate_limit_buckets_are_independent() {
        // Exhausting one (ip, email) bucket does not throttle a different one.
        let gate = LoginGate::with_default_quota();
        let victim = LoginGate::key_hash("1.2.3.4", "victim@example.com");
        let other = LoginGate::key_hash("1.2.3.4", "bystander@example.com");
        for _ in 0..10 {
            gate.check(&victim).unwrap();
        }
        assert!(gate.check(&victim).is_err());
        gate.check(&other).expect("a different account's budget is independent");
    }

    // ----- IpGate (P0-2, class-level per-IP ceiling) -----

    use std::net::{IpAddr, Ipv4Addr};
    fn ip(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn ip_gate_trips_after_quota_and_is_per_ip() {
        // A small gate: 3/min. First 3 pass, 4th throttled; a DIFFERENT IP is
        // unaffected (so one flooding host can't starve everyone).
        let gate = IpGate::new(NonZeroU32::new(3).unwrap());
        let attacker = ip(1, 2, 3, 4);
        let bystander = ip(9, 9, 9, 9);
        for i in 0..3 {
            gate.check(attacker).unwrap_or_else(|e| panic!("call {i} should pass: {e:?}"));
        }
        assert!(gate.check(attacker).is_err(), "4th from the same IP is throttled");
        gate.check(bystander).expect("a different IP has its own budget");
    }

    #[test]
    fn ip_gate_key_is_domain_separated_from_anon_and_login() {
        // The same textual value must land in disjoint buckets across gates.
        let k_ip = IpGate::key_hash(ip(10, 0, 0, 1));
        let k_login = LoginGate::key_hash("10.0.0.1", "");
        assert_ne!(k_ip, k_login, "ip and login domains must not collide");
    }

    #[test]
    fn resolve_client_ip_ignores_xff_when_no_trusted_proxy() {
        // hops=0: XFF is spoofable and MUST be ignored — always the peer.
        let peer = ip(203, 0, 113, 7);
        let got = resolve_client_ip(Some("1.1.1.1, 2.2.2.2"), peer, 0);
        assert_eq!(got, peer);
    }

    #[test]
    fn resolve_client_ip_strips_one_trusted_proxy() {
        // hops=1 (e.g. Railway's single LB): XFF = "client, lb". Strip the
        // rightmost (the LB) → the client.
        let peer = ip(10, 0, 0, 1); // the LB's address as the TCP peer
        let got = resolve_client_ip(Some("198.51.100.9, 10.0.0.1"), peer, 1);
        assert_eq!(got, ip(198, 51, 100, 9));
    }

    #[test]
    fn resolve_client_ip_falls_back_when_chain_too_short() {
        // hops=2 but only one XFF entry: a client can't forge fewer entries
        // than the real proxy count into trust — fall back to the peer.
        let peer = ip(10, 0, 0, 1);
        let got = resolve_client_ip(Some("198.51.100.9"), peer, 2);
        assert_eq!(got, peer, "shortfall must not trust the leftmost forged value");
    }

    #[test]
    fn resolve_client_ip_falls_back_on_missing_or_garbage_xff() {
        let peer = ip(10, 0, 0, 1);
        assert_eq!(resolve_client_ip(None, peer, 1), peer);
        assert_eq!(resolve_client_ip(Some("not-an-ip"), peer, 1), peer);
    }
}
