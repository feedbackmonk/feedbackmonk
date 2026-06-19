//! Customer-side runner-token mint helper (Contract C25, FR-FBR-24 — Worker B).
//!
//! **feedbackmonk holds no private key (DEC-FBR-04).** The customer mints the
//! runner write-token *client-side* with the private half of a registered
//! `runner`-class Ed25519 signing key; feedbackmonk only ever verifies (the
//! audited `feedbackmonk_jwt::verify` path). This module is the sign side — it
//! lives in the runner crate (NOT the API server) so the server never gains a
//! signing capability.
//!
//! The minted token is a JWT the backend's `verify_runner_token` accepts:
//!   - header `{"alg":"EdDSA","typ":"JWT"}` (the only algorithm the verifier
//!     allows — alg-none / alg-confusion are rejected fast);
//!   - claims `{ sub, scope:"runner:write", aud:<project_id>, iat, exp, jti }`;
//!   - signature = Ed25519 over `b64url(header).b64url(payload)`.
//!
//! Short TTL (default 24h) is the backstop; `jti` is the revocation key the
//! owner can kill via `DELETE /runner-tokens/:jti`.

use std::path::Path;

use anyhow::Context;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey as Ed25519SigningKey};
use serde_json::json;
use uuid::Uuid;

/// The signed class marker that makes a token a runner write-token. MUST equal
/// the backend's `RUNNER_TOKEN_SCOPE` (`feedbackmonk-api` `work_orders.rs`); a
/// token without this exact scope is rejected `403` even with a valid signature.
pub const RUNNER_TOKEN_SCOPE: &str = "runner:write";

/// Default token lifetime: 24h. Rotation = re-mint (the runner is long-lived;
/// the token is short-lived).
pub const DEFAULT_TTL_SECONDS: i64 = 86_400;

/// Length of a raw Ed25519 private seed.
const SEED_LEN: usize = 32;

/// Load a runner-class Ed25519 **private** key (the 32-byte seed) from a file.
/// Accepts the seed as hex (64 chars) or base64 (standard / url-safe, padded or
/// not). PKCS8/PEM is intentionally NOT supported — the seed format keeps the
/// helper dependency-light and matches how the public half is registered (raw
/// 32 bytes).
///
/// # Errors
/// File-read failure, or content that is not a 32-byte seed in a recognised
/// encoding.
pub fn load_signing_key(path: &Path) -> anyhow::Result<Ed25519SigningKey> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading runner key file {}", path.display()))?;
    let seed = parse_seed(raw.trim()).with_context(|| {
        format!(
            "key file {} must hold a 32-byte Ed25519 seed as hex (64 chars) or base64",
            path.display()
        )
    })?;
    Ok(Ed25519SigningKey::from_bytes(&seed))
}

/// Parse a 32-byte seed from hex or base64 text. Returns `None` if no encoding
/// yields exactly 32 bytes.
fn parse_seed(s: &str) -> Option<[u8; SEED_LEN]> {
    // Hex (64 chars).
    if s.len() == SEED_LEN * 2 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        let bytes: Option<Vec<u8>> = (0..SEED_LEN)
            .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok())
            .collect();
        if let Some(b) = bytes {
            return b.try_into().ok();
        }
    }
    // Base64 (try the common alphabets / padding modes).
    for engine in [
        STANDARD,
        STANDARD_NO_PAD,
        URL_SAFE,
        URL_SAFE_NO_PAD,
    ] {
        if let Ok(decoded) = engine.decode(s) {
            if decoded.len() == SEED_LEN {
                return decoded.try_into().ok();
            }
        }
    }
    None
}

/// Mint a runner write-token with explicit, injectable time + `jti` (the pure,
/// deterministic core — tests drive it with fixed values).
///
/// # Errors
/// JSON serialization failure (effectively never for these fixed shapes).
pub fn mint_token(
    signing_key: &Ed25519SigningKey,
    sub: &str,
    project_id: Uuid,
    iat: i64,
    exp: i64,
    jti: &str,
) -> anyhow::Result<String> {
    let header = serde_json::to_vec(&json!({ "alg": "EdDSA", "typ": "JWT" }))?;
    let payload = serde_json::to_vec(&json!({
        "sub": sub,
        "scope": RUNNER_TOKEN_SCOPE,
        "aud": project_id.to_string(),
        "iat": iat,
        "exp": exp,
        "jti": jti,
    }))?;
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header),
        URL_SAFE_NO_PAD.encode(payload)
    );
    let signature = signing_key.sign(signing_input.as_bytes());
    Ok(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    ))
}

/// The minted token plus its `jti` (so the caller can register it for admin
/// visibility / future revocation via `POST /runner-tokens`).
#[derive(Debug, Clone)]
pub struct MintedToken {
    pub token: String,
    pub jti: String,
    pub expires_at_unix: i64,
}

/// Mint a runner write-token from a key file, using the current wall clock and
/// a fresh random `jti`. This is the production entrypoint behind
/// `feedbackmonk-runner mint-token`.
///
/// # Errors
/// Key-load failure or signing failure.
pub fn mint_from_key_file(
    key_path: &Path,
    sub: &str,
    project_id: Uuid,
    ttl_seconds: i64,
) -> anyhow::Result<MintedToken> {
    let signing_key = load_signing_key(key_path)?;
    let iat = chrono::Utc::now().timestamp();
    let exp = iat + ttl_seconds.max(1);
    let jti = Uuid::new_v4().to_string();
    let token = mint_token(&signing_key, sub, project_id, iat, exp, &jti)?;
    Ok(MintedToken {
        token,
        jti,
        expires_at_unix: exp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_key() -> Ed25519SigningKey {
        Ed25519SigningKey::from_bytes(&[7u8; SEED_LEN])
    }

    #[test]
    fn parse_seed_accepts_hex_and_base64() {
        let seed = [9u8; SEED_LEN];
        let hex: String = seed.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(parse_seed(&hex), Some(seed));
        assert_eq!(parse_seed(&STANDARD.encode(seed)), Some(seed));
        assert_eq!(parse_seed(&URL_SAFE_NO_PAD.encode(seed)), Some(seed));
        assert_eq!(parse_seed("not-a-key"), None);
        // 31 bytes — wrong length, rejected.
        assert_eq!(parse_seed(&STANDARD.encode([1u8; 31])), None);
    }

    #[test]
    fn minted_token_is_three_part_eddsa() {
        let token = mint_token(&fixed_key(), "ci-runner", Uuid::nil(), 1000, 2000, "jti-1").unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["alg"], "EdDSA");
        let payload: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(payload["scope"], RUNNER_TOKEN_SCOPE);
        assert_eq!(payload["jti"], "jti-1");
    }

    /// The load-bearing cross-check: a token this helper mints must verify under
    /// the EXACT verifier the backend runs (`feedbackmonk_jwt::verify`), against
    /// the public half of the same key. If this passes, the runner's tokens are
    /// accepted by `verify_runner_token` (modulo the runner-class key selection
    /// + scope/jti gates, which it also satisfies: scope == "runner:write").
    #[test]
    fn minted_token_verifies_under_backend_verifier() {
        use feedbackmonk_core::SigningKey as CoreSigningKey;
        use feedbackmonk_core::SigningKeyId;

        let sk = fixed_key();
        let project_id = Uuid::new_v4();
        let now = 10_000;
        let token = mint_token(&sk, "ci-runner", project_id, now, now + 3600, "jti-x").unwrap();

        let core_key = CoreSigningKey {
            id: SigningKeyId(Uuid::new_v4()),
            project_id,
            public_key: sk.verifying_key().to_bytes().to_vec(),
            label: "runner".into(),
            active: true,
            registered_at: chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap(),
            deactivated_at: None,
        };

        let claims = feedbackmonk_jwt::verify(&token, project_id, &[core_key], now + 1).unwrap();
        assert_eq!(claims.sub, "ci-runner");
    }

    #[test]
    fn wrong_audience_token_is_rejected_by_verifier() {
        use feedbackmonk_core::SigningKey as CoreSigningKey;
        use feedbackmonk_core::SigningKeyId;

        let sk = fixed_key();
        let minted_for = Uuid::new_v4();
        let other_project = Uuid::new_v4();
        let now = 10_000;
        let token = mint_token(&sk, "r", minted_for, now, now + 3600, "j").unwrap();

        let core_key = CoreSigningKey {
            id: SigningKeyId(Uuid::new_v4()),
            project_id: other_project,
            public_key: sk.verifying_key().to_bytes().to_vec(),
            label: "runner".into(),
            active: true,
            registered_at: chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).unwrap(),
            deactivated_at: None,
        };
        // Delivered to a DIFFERENT project's endpoint → WrongAudience.
        assert!(feedbackmonk_jwt::verify(&token, other_project, &[core_key], now + 1).is_err());
    }
}
