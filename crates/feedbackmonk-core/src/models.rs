//! Plain data structs mirroring the P0 schema.
//!
//! These are read/written by `feedbackmonk-repository` and serialized at the
//! API boundary by `feedbackmonk-api`. They carry NO DB connection, NO async,
//! and NO scope -- scope discipline lives in the repository layer.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ids::{FeedbackId, SigningKeyId};
use crate::status::FeedbackStatus;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tenant {
    pub id: Uuid,
    pub email: String,
    /// Argon2id-hashed password. Never serialize this externally; it is
    /// included here for the repository -> handler boundary, and the API
    /// layer must avoid exposing it in any response body.
    pub password_hash: String,
    pub verified_at: Option<DateTime<Utc>>,
    pub tier: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigningKey {
    pub id: SigningKeyId,
    pub project_id: Uuid,
    /// Raw Ed25519 public key (32 bytes).
    pub public_key: Vec<u8>,
    pub label: String,
    pub active: bool,
    pub registered_at: DateTime<Utc>,
    pub deactivated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeedbackKind {
    Bug,
    Feature,
    Question,
    #[default]
    Other,
}

impl FeedbackKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bug => "bug",
            Self::Feature => "feature",
            Self::Question => "question",
            Self::Other => "other",
        }
    }

    #[must_use]
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "bug" => Self::Bug,
            "feature" => Self::Feature,
            "question" => Self::Question,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feedback {
    pub id: Uuid,
    pub short_code: FeedbackId,
    pub project_id: Uuid,
    pub tenant_id: Uuid,
    /// JWT `sub` claim when submitted in auth mode; `None` when anonymous.
    pub end_user_sub: Option<String>,
    pub end_user_email: Option<String>,
    /// JWT `name` claim, auth mode only.
    pub end_user_name: Option<String>,
    /// JWT `external_metadata` claim (auth mode); enforced <= 4KB at app layer.
    pub external_metadata: Option<serde_json::Value>,
    /// 32-byte hash of (cookie + project_id + IP). Anonymous mode only.
    pub anon_token_hash: Option<Vec<u8>>,
    pub body: String,
    pub kind: FeedbackKind,
    pub accepted_at: DateTime<Utc>,
    /// FR-FBR-08 status workflow column. Defaults to `Submitted` for rows
    /// inserted before migration 00003 (the column has a server-side
    /// default; the repository layer reads it as part of `get_with_history`).
    #[serde(default)]
    pub status: FeedbackStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnonSubmission {
    pub anon_token_hash: Vec<u8>,
    pub project_id: Uuid,
    pub first_seen_at: DateTime<Utc>,
    pub last_submission_at: DateTime<Utc>,
    pub submission_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitCounter {
    pub bucket_key: String,
    pub project_id: Uuid,
    pub window_start: DateTime<Utc>,
    pub count: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_kind_round_trip() {
        for k in [
            FeedbackKind::Bug,
            FeedbackKind::Feature,
            FeedbackKind::Question,
            FeedbackKind::Other,
        ] {
            assert_eq!(FeedbackKind::from_db_str(k.as_str()), k);
        }
    }

    #[test]
    fn feedback_kind_default_is_other() {
        assert_eq!(FeedbackKind::default(), FeedbackKind::Other);
    }

    #[test]
    fn feedback_kind_unknown_db_value_falls_back_to_other() {
        assert_eq!(FeedbackKind::from_db_str("definitely-not-a-kind"), FeedbackKind::Other);
    }
}
