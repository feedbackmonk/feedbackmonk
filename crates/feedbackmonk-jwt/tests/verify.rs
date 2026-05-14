//! JWT fixture corpus -- Task Zero for Stage 2 Worker B (FR-FBR-05).
//!
//! Per the P0 plan's Testability Gate finding: each Contract C2 hard
//! invariant gets a NAMED test against a deterministically minted fixture.
//! Fixtures are minted in-process from committed seed bytes, so the suite
//! is hermetic and reproducible.
//!
//! Corpus (a)-(h) below covers each Contract C2 hard invariant:
//!
//! | # | Name | Expected |
//! |---|---|---|
//! | a | `fixture_a_valid_key_1`         | Ok(VerifiedClaims) |
//! | b | `fixture_b_valid_key_2_rotation`| Ok(VerifiedClaims) -- verifier tries all active keys |
//! | c | `fixture_c_expired`             | Err(Expired) |
//! | d | `fixture_d_wrong_aud`           | Err(WrongAudience) |
//! | e | `fixture_e_alg_none_attack`     | Err(AlgorithmNotAllowed) |
//! | f | `fixture_f_hs256_confusion_attack` | Err(AlgorithmNotAllowed) |
//! | g | `fixture_g_missing_claim_sub` (+ iat, exp, aud) | Err(MissingRequiredClaim(name)) |
//! | h | `fixture_h_oversize_external_metadata` | Err(ExternalMetadataTooLarge) |
//!
//! Plus several integration-shape tests for the multi-key rotation
//! semantics, BadSignature path, and `verify_with_leeway` boundary.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey as EdSigningKey};
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;
use uuid::Uuid;

use feedbackmonk_core::{SigningKey, SigningKeyId};
use feedbackmonk_jwt::{
    verify, verify_with_leeway, JwtError, ACCEPTED_ALG, MAX_EXTERNAL_METADATA_BYTES,
};

// ============================================================================
// Deterministic seeds (test-only secrets; reproducibility, not security)
// ============================================================================
//
// These seeds drive ed25519-dalek's SigningKey::from_bytes -- same seed bytes
// in every test run produce the same keypair, and therefore the same JWT
// bytes for a given (header, payload) pair. This is how the fixture corpus
// is hermetic without committing the raw .jwt strings to the repo.

const SEED_KEY_1: [u8; 32] = [
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
];
const SEED_KEY_2: [u8; 32] = [
    0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
    0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
];
// A signing key used to make a "valid signature for project A" used in the
// wrong-aud test -- the verifier is called with project_B as expected_aud,
// project_B's active keys; this seed never appears in any verifier's
// active_keys, but the wrong_aud check fires BEFORE signature check, so the
// fixture's expected outcome is WrongAudience.
const SEED_FOREIGN_KEY: [u8; 32] = [
    0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
    0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
];

const PROJECT_A: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_00aa);
const PROJECT_B: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_00bb);

const NOW_FIXED: i64 = 1_715_625_600; // 2026-05-13T20:00:00Z, stable across runs

// ============================================================================
// Helpers: minting + active-keys construction
// ============================================================================

fn mint_eddsa_jwt(seed: [u8; 32], header_json: &str, payload_json: &str) -> String {
    let signing_key = EdSigningKey::from_bytes(&seed);
    let header_b64 = URL_SAFE_NO_PAD.encode(header_json.as_bytes());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload_json.as_bytes());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let signature = signing_key.sign(signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());
    format!("{header_b64}.{payload_b64}.{sig_b64}")
}

fn public_key_for(seed: [u8; 32]) -> [u8; 32] {
    EdSigningKey::from_bytes(&seed).verifying_key().to_bytes()
}

fn signing_key_model(seed: [u8; 32], project_id: Uuid, label: &str) -> SigningKey {
    SigningKey {
        id: SigningKeyId(Uuid::new_v4()),
        project_id,
        public_key: public_key_for(seed).to_vec(),
        label: label.to_string(),
        active: true,
        registered_at: Utc::now(),
        deactivated_at: None,
    }
}

fn default_eddsa_header() -> String {
    serde_json::to_string(&json!({"alg": ACCEPTED_ALG, "typ": "JWT"})).unwrap()
}

fn default_valid_payload(project_id: Uuid) -> serde_json::Value {
    json!({
        "sub": "auth0|user-deterministic-1",
        "iat": NOW_FIXED - 30,
        "exp": NOW_FIXED + 300,
        "aud": project_id.to_string(),
        "email": "u@example.com",
        "name": "Alice",
    })
}

// ============================================================================
// (a) valid_key_1 -- Ok(VerifiedClaims)
// ============================================================================

#[test]
fn fixture_a_valid_key_1() {
    let header = default_eddsa_header();
    let payload = default_valid_payload(PROJECT_A);
    let token = mint_eddsa_jwt(SEED_KEY_1, &header, &payload.to_string());
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "key-1")];

    let claims = verify(&token, PROJECT_A, &active, NOW_FIXED).expect("must verify");
    assert_eq!(claims.sub, "auth0|user-deterministic-1");
    assert_eq!(claims.email.as_deref(), Some("u@example.com"));
    assert_eq!(claims.name.as_deref(), Some("Alice"));
    assert_eq!(claims.iat, NOW_FIXED - 30);
    assert_eq!(claims.exp, NOW_FIXED + 300);
}

// ============================================================================
// (b) valid_key_2_rotation -- verifier tries all active keys
// ============================================================================

#[test]
fn fixture_b_valid_key_2_rotation() {
    // Signed by key-2; project has BOTH key-1 and key-2 active (key-1 first
    // in list, so verifier tries it FIRST and fails, then tries key-2 which
    // succeeds). This proves the rotation/zero-downtime invariant.
    let header = default_eddsa_header();
    let payload = default_valid_payload(PROJECT_A);
    let token = mint_eddsa_jwt(SEED_KEY_2, &header, &payload.to_string());
    let active = vec![
        signing_key_model(SEED_KEY_1, PROJECT_A, "key-1-old"),
        signing_key_model(SEED_KEY_2, PROJECT_A, "key-2-new"),
    ];

    let claims = verify(&token, PROJECT_A, &active, NOW_FIXED).expect("rotation key wins");
    assert_eq!(claims.sub, "auth0|user-deterministic-1");
}

// ============================================================================
// (c) expired -- Err(Expired)
// ============================================================================

#[test]
fn fixture_c_expired() {
    let header = default_eddsa_header();
    let mut payload = default_valid_payload(PROJECT_A);
    payload["exp"] = json!(NOW_FIXED - 60); // exp 60s in the past
    payload["iat"] = json!(NOW_FIXED - 120);
    let token = mint_eddsa_jwt(SEED_KEY_1, &header, &payload.to_string());
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "key-1")];

    let err = verify(&token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::Expired);
}

#[test]
fn fixture_c_expired_is_strict_no_leeway() {
    // exp = now exactly is NOT expired (boundary: now > exp is the test).
    let header = default_eddsa_header();
    let mut payload = default_valid_payload(PROJECT_A);
    payload["exp"] = json!(NOW_FIXED);
    let token = mint_eddsa_jwt(SEED_KEY_1, &header, &payload.to_string());
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "key-1")];
    let result = verify(&token, PROJECT_A, &active, NOW_FIXED);
    assert!(result.is_ok(), "exp == now is not yet expired");

    // exp = now - 1 IS expired -- strict, no leeway.
    let mut payload2 = default_valid_payload(PROJECT_A);
    payload2["exp"] = json!(NOW_FIXED - 1);
    let token2 = mint_eddsa_jwt(SEED_KEY_1, &header, &payload2.to_string());
    let err = verify(&token2, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::Expired);
}

// ============================================================================
// (d) wrong_aud -- Err(WrongAudience)
// ============================================================================

#[test]
fn fixture_d_wrong_aud_token_for_project_a_delivered_to_project_b() {
    // Signed by a key that IS valid for project_A, with aud=project_A.
    // Verifier called as if request hit project_B's endpoint:
    // expected_aud=PROJECT_B, active_keys=project_B's keys (different).
    // Per Contract C2 invariant 3: WrongAudience BEFORE signature check.
    let header = default_eddsa_header();
    let payload = default_valid_payload(PROJECT_A); // aud=PROJECT_A in claims
    let token = mint_eddsa_jwt(SEED_FOREIGN_KEY, &header, &payload.to_string());

    let project_b_keys = vec![signing_key_model(SEED_KEY_1, PROJECT_B, "b-only")];
    let err = verify(&token, PROJECT_B, &project_b_keys, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::WrongAudience);
}

#[test]
fn fixture_d_wrong_aud_even_if_signature_would_have_verified() {
    // Stronger form of invariant 3: token signed by key-1, aud=project_A.
    // Verifier expects project_B; SAME key-1 is also in project_B's
    // active_keys (cross-registered). Signature would verify. WrongAudience
    // must still fire BEFORE signature check.
    let header = default_eddsa_header();
    let payload = default_valid_payload(PROJECT_A);
    let token = mint_eddsa_jwt(SEED_KEY_1, &header, &payload.to_string());

    let active_b = vec![signing_key_model(SEED_KEY_1, PROJECT_B, "cross-registered")];
    let err = verify(&token, PROJECT_B, &active_b, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::WrongAudience);
}

// ============================================================================
// (e) alg_none_attack -- Err(AlgorithmNotAllowed)
// ============================================================================

#[test]
fn fixture_e_alg_none_attack() {
    let header_json = r#"{"alg":"none","typ":"JWT"}"#;
    let payload = default_valid_payload(PROJECT_A);
    let header_b64 = URL_SAFE_NO_PAD.encode(header_json.as_bytes());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
    // alg=none typically has empty signature -- preserve that as part of
    // the attack surface (and our verifier must NOT accept it).
    let token = format!("{header_b64}.{payload_b64}.");

    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "key-1")];
    let err = verify(&token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::AlgorithmNotAllowed);
}

#[test]
fn fixture_e_alg_none_attack_fails_even_with_no_active_keys() {
    // Alg check fires BEFORE signature check, so this must reject even when
    // the verifier could not have verified anyway. Prevents fallback bugs
    // where an unverifiable token gets through because no keys were tried.
    let header_json = r#"{"alg":"none","typ":"JWT"}"#;
    let payload = default_valid_payload(PROJECT_A);
    let header_b64 = URL_SAFE_NO_PAD.encode(header_json.as_bytes());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
    let token = format!("{header_b64}.{payload_b64}.");
    let no_keys: Vec<SigningKey> = vec![];
    let err = verify(&token, PROJECT_A, &no_keys, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::AlgorithmNotAllowed);
}

// ============================================================================
// (f) hs256_confusion_attack -- Err(AlgorithmNotAllowed)
// ============================================================================

#[test]
fn fixture_f_hs256_confusion_attack() {
    // Classic algorithm-confusion: attacker takes project_A's Ed25519 PUBLIC
    // key and uses it as an HMAC-SHA256 secret to sign a token with alg=HS256.
    // If the verifier accepts HS256 with the project's stored public key as
    // the HMAC secret, the token verifies. Defense: alg allowlist = EdDSA only.
    let header_json = r#"{"alg":"HS256","typ":"JWT"}"#;
    let payload = default_valid_payload(PROJECT_A);
    let header_b64 = URL_SAFE_NO_PAD.encode(header_json.as_bytes());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
    let signing_input = format!("{header_b64}.{payload_b64}");

    let public_key = public_key_for(SEED_KEY_1);
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(&public_key).unwrap();
    mac.update(signing_input.as_bytes());
    let hmac_sig = mac.finalize().into_bytes();
    let sig_b64 = URL_SAFE_NO_PAD.encode(hmac_sig);
    let token = format!("{header_b64}.{payload_b64}.{sig_b64}");

    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "key-1")];
    let err = verify(&token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::AlgorithmNotAllowed);
}

// ============================================================================
// (g) missing_claim_{sub,iat,exp,aud} -- Err(MissingRequiredClaim(name))
// ============================================================================

#[test]
fn fixture_g_missing_claim_sub() {
    let header = default_eddsa_header();
    let mut payload = default_valid_payload(PROJECT_A);
    payload.as_object_mut().unwrap().remove("sub");
    let token = mint_eddsa_jwt(SEED_KEY_1, &header, &payload.to_string());
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "k")];
    let err = verify(&token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::MissingRequiredClaim("sub"));
}

#[test]
fn fixture_g_missing_claim_iat() {
    let header = default_eddsa_header();
    let mut payload = default_valid_payload(PROJECT_A);
    payload.as_object_mut().unwrap().remove("iat");
    let token = mint_eddsa_jwt(SEED_KEY_1, &header, &payload.to_string());
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "k")];
    let err = verify(&token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::MissingRequiredClaim("iat"));
}

#[test]
fn fixture_g_missing_claim_exp() {
    let header = default_eddsa_header();
    let mut payload = default_valid_payload(PROJECT_A);
    payload.as_object_mut().unwrap().remove("exp");
    let token = mint_eddsa_jwt(SEED_KEY_1, &header, &payload.to_string());
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "k")];
    let err = verify(&token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::MissingRequiredClaim("exp"));
}

#[test]
fn fixture_g_missing_claim_aud() {
    let header = default_eddsa_header();
    let mut payload = default_valid_payload(PROJECT_A);
    payload.as_object_mut().unwrap().remove("aud");
    let token = mint_eddsa_jwt(SEED_KEY_1, &header, &payload.to_string());
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "k")];
    let err = verify(&token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::MissingRequiredClaim("aud"));
}

// ============================================================================
// (h) oversize_external_metadata -- Err(ExternalMetadataTooLarge)
// ============================================================================

#[test]
fn fixture_h_oversize_external_metadata() {
    let header = default_eddsa_header();
    let mut payload = default_valid_payload(PROJECT_A);
    // 5KB of JSON: a single string value of ~5000 bytes serializes to ~5002
    // bytes (with quotes), well over the 4096 cap.
    let big_string: String = "x".repeat(5000);
    payload["external_metadata"] = json!({"blob": big_string});
    let token = mint_eddsa_jwt(SEED_KEY_1, &header, &payload.to_string());
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "key-1")];
    let err = verify(&token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::ExternalMetadataTooLarge);
}

#[test]
fn fixture_h_external_metadata_at_cap_passes() {
    // Just-under-cap metadata is accepted -- documents the boundary.
    let header = default_eddsa_header();
    let mut payload = default_valid_payload(PROJECT_A);
    // Aim for ~4000 bytes encoded (well under 4096).
    let mid_string: String = "y".repeat(4000);
    payload["external_metadata"] = json!({"blob": mid_string});
    let token = mint_eddsa_jwt(SEED_KEY_1, &header, &payload.to_string());
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "key-1")];
    let claims = verify(&token, PROJECT_A, &active, NOW_FIXED).expect("under cap must pass");
    let meta = claims.external_metadata.expect("metadata round-trips");
    assert!(meta["blob"].as_str().unwrap().len() == 4000);
    // Sanity-check the cap is what we documented.
    assert_eq!(MAX_EXTERNAL_METADATA_BYTES, 4096);
}

// ============================================================================
// Additional invariants: bad signature, leeway boundary, malformed inputs
// ============================================================================

#[test]
fn bad_signature_is_caught_after_aud_and_temporal_checks() {
    // Token with a tampered signature byte; alg + aud + claims look fine but
    // the signature fails verification. Result: BadSignature (NOT
    // MalformedToken -- the parse succeeds, the cryptographic check fails).
    let header = default_eddsa_header();
    let payload = default_valid_payload(PROJECT_A);
    let mut token = mint_eddsa_jwt(SEED_KEY_1, &header, &payload.to_string());
    // Flip the last character of the signature -- still valid base64, but
    // produces a different byte sequence and therefore an invalid signature.
    let last = token.pop().unwrap();
    let replacement = if last == 'A' { 'B' } else { 'A' };
    token.push(replacement);

    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "key-1")];
    let err = verify(&token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::BadSignature);
}

#[test]
fn bad_signature_when_no_active_key_matches() {
    // Token signed by SEED_KEY_2; verifier has only SEED_KEY_1 in
    // active_keys -- signature won't verify against the wrong key.
    let header = default_eddsa_header();
    let payload = default_valid_payload(PROJECT_A);
    let token = mint_eddsa_jwt(SEED_KEY_2, &header, &payload.to_string());
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "wrong-key")];
    let err = verify(&token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::BadSignature);
}

#[test]
fn iat_leeway_tolerates_future_iat_within_window() {
    // iat slightly in the future (clock skew) is tolerated up to leeway.
    let header = default_eddsa_header();
    let mut payload = default_valid_payload(PROJECT_A);
    payload["iat"] = json!(NOW_FIXED + 3); // 3s ahead
    let token = mint_eddsa_jwt(SEED_KEY_1, &header, &payload.to_string());
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "k")];

    // Default leeway (5s) accepts.
    let claims = verify(&token, PROJECT_A, &active, NOW_FIXED).expect("within leeway");
    assert_eq!(claims.iat, NOW_FIXED + 3);
}

#[test]
fn iat_in_far_future_is_not_yet_valid() {
    let header = default_eddsa_header();
    let mut payload = default_valid_payload(PROJECT_A);
    payload["iat"] = json!(NOW_FIXED + 3600); // 1h ahead
    let token = mint_eddsa_jwt(SEED_KEY_1, &header, &payload.to_string());
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "k")];
    let err = verify(&token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::NotYetValid);
}

#[test]
fn verify_with_leeway_zero_rejects_any_future_iat() {
    let header = default_eddsa_header();
    let mut payload = default_valid_payload(PROJECT_A);
    payload["iat"] = json!(NOW_FIXED + 1);
    let token = mint_eddsa_jwt(SEED_KEY_1, &header, &payload.to_string());
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "k")];
    let err = verify_with_leeway(&token, PROJECT_A, &active, NOW_FIXED, 0).unwrap_err();
    assert_eq!(err, JwtError::NotYetValid);
}

#[test]
fn header_with_no_alg_field_is_algorithm_not_allowed() {
    // Per Contract C2 invariant 1: any header that does not explicitly
    // declare alg=EdDSA fails fast. Missing-alg is the strongest case.
    let header_json = r#"{"typ":"JWT"}"#;
    let payload = default_valid_payload(PROJECT_A);
    let header_b64 = URL_SAFE_NO_PAD.encode(header_json.as_bytes());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
    let token = format!("{header_b64}.{payload_b64}.AAAA");
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "k")];
    let err = verify(&token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::AlgorithmNotAllowed);
}

#[test]
fn rs256_attack_rejected() {
    // Extra invariant beyond the labelled corpus: any non-EdDSA alg is
    // rejected, including ones not in the brief's table (RS256, ES256).
    for alg in ["RS256", "ES256", "PS256", "EdDsa", "eddsa", " "] {
        let header_json = format!(r#"{{"alg":"{alg}","typ":"JWT"}}"#);
        let payload = default_valid_payload(PROJECT_A);
        let header_b64 = URL_SAFE_NO_PAD.encode(header_json.as_bytes());
        let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let token = format!("{header_b64}.{payload_b64}.AAAA");
        let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "k")];
        let err = verify(&token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
        assert_eq!(
            err,
            JwtError::AlgorithmNotAllowed,
            "alg={alg:?} must be rejected"
        );
    }
}

#[test]
fn malformed_header_base64() {
    // Non-base64url bytes in the header position fail with MalformedToken.
    let token = "!!notbase64!!.eyJzdWIiOiJhIn0.AAAA";
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "k")];
    let err = verify(token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::MalformedToken);
}

#[test]
fn malformed_signature_wrong_length() {
    // Signature decodes to bytes but is not 64 bytes -> MalformedToken.
    let header = default_eddsa_header();
    let payload = default_valid_payload(PROJECT_A);
    let header_b64 = URL_SAFE_NO_PAD.encode(header.as_bytes());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
    let short_sig = URL_SAFE_NO_PAD.encode([0u8; 10]);
    let token = format!("{header_b64}.{payload_b64}.{short_sig}");
    let active = vec![signing_key_model(SEED_KEY_1, PROJECT_A, "k")];
    let err = verify(&token, PROJECT_A, &active, NOW_FIXED).unwrap_err();
    assert_eq!(err, JwtError::MalformedToken);
}
