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
}
