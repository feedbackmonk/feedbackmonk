//! Strongly-typed identifiers for feedbackmonk domain entities.

use rand::Rng;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Internal UUID for a signing key (Ed25519 public-key registration row).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SigningKeyId(pub Uuid);

impl SigningKeyId {
    #[must_use]
    pub fn into_uuid(self) -> Uuid {
        self.0
    }
}

impl From<Uuid> for SigningKeyId {
    fn from(u: Uuid) -> Self {
        Self(u)
    }
}

/// Public-facing feedback identifier of the form `FB-XXXXXX`.
///
/// `FeedbackId` is intentionally a short code (NOT a raw UUID) so customers
/// can reference items in conversation, support tickets, and product comms.
/// The internal row PK is still a UUID; `short_code` lives alongside it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeedbackId(pub String);

impl FeedbackId {
    /// Generate a fresh `FB-XXXXXX` short code from cryptographic randomness.
    ///
    /// Six characters drawn from a 32-char Crockford-style alphabet (no I/L/O/U
    /// to avoid visual ambiguity). 32^6 = ~10^9 -- collision probability is
    /// negligible at P0 scale and the repository INSERT enforces uniqueness
    /// via the schema's UNIQUE constraint on `short_code`.
    #[must_use]
    pub fn generate() -> Self {
        const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        let mut rng = rand::thread_rng();
        let mut s = String::with_capacity(9);
        s.push_str("FB-");
        for _ in 0..6 {
            let i = rng.gen_range(0..ALPHABET.len());
            s.push(ALPHABET[i] as char);
        }
        Self(s)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for FeedbackId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl std::fmt::Display for FeedbackId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_emits_fb_prefix_and_total_length_9() {
        let id = FeedbackId::generate();
        assert!(id.as_str().starts_with("FB-"));
        assert_eq!(id.as_str().len(), 9);
    }

    #[test]
    fn generate_uses_unambiguous_alphabet() {
        for _ in 0..1000 {
            let id = FeedbackId::generate();
            let body = &id.as_str()[3..];
            for c in body.chars() {
                assert!(
                    !"ILOUilou".contains(c),
                    "ambiguous char in {id}: {c}"
                );
            }
        }
    }

    #[test]
    fn signing_key_id_uuid_round_trip() {
        let u = Uuid::new_v4();
        let id = SigningKeyId::from(u);
        assert_eq!(id.into_uuid(), u);
    }
}
