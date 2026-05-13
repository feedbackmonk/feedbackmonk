//! Argon2id password hashing -- RFC 9106 first-recommended defaults.
//!
//! Hashes are stored as PHC-format strings in `tenants.password_hash` (TEXT).
//! Verification compares the user-supplied password against the stored PHC
//! hash; salt + parameters live in the hash itself.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

use crate::error::ApiError;

/// Hash `password` with argon2id defaults. Returns the PHC-format string.
pub fn hash_password(password: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::Internal(format!("argon2 hash failed: {e}")))
}

/// Verify `password` against the stored PHC-format hash. Returns `Ok(true)`
/// on match, `Ok(false)` on mismatch. `Err` only on malformed-hash error.
pub fn verify_password(password: &str, phc_hash: &str) -> Result<bool, ApiError> {
    let parsed = PasswordHash::new(phc_hash)
        .map_err(|e| ApiError::Internal(format!("malformed argon2 hash: {e}")))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_round_trip() {
        let hash = hash_password("hunter2-correct-horse").unwrap();
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password("hunter2-correct-horse", &hash).unwrap());
        assert!(!verify_password("wrong-password", &hash).unwrap());
    }

    #[test]
    fn hash_produces_distinct_salts() {
        let h1 = hash_password("samepass").unwrap();
        let h2 = hash_password("samepass").unwrap();
        // Distinct random salts => distinct hash strings.
        assert_ne!(h1, h2);
    }

    #[test]
    fn verify_against_malformed_hash_errors() {
        let result = verify_password("anything", "not-a-real-phc-hash");
        assert!(matches!(result, Err(ApiError::Internal(_))));
    }
}
