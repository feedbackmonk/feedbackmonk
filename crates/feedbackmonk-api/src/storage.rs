//! Object storage for feedback attachments (Gap #1).
//!
//! A small storage abstraction so the SAME upload handler works for:
//!   - **self-host / dev**: `LocalFsStorage` — writes objects under a local
//!     directory (a docker volume in the self-host compose stack). This is the
//!     default backend and needs no external service. (Recommended for the
//!     GitCellar customer-#1 self-host path, PF-DEPLOY-01.)
//!   - **SaaS / MinIO**: `S3Storage` — PUTs objects to any S3-compatible
//!     endpoint via AWS SigV4. Works against AWS S3 and MinIO alike (set
//!     `FEEDBACKMONK_S3_ENDPOINT` + path-style for MinIO).
//!
//! Backend selection + configuration is env-driven (`from_env`); every var is
//! catalogued in `docs/operations/SELFHOST_ENV.md` (Contract C21).
//!
//! ## Why a hand-rolled SigV4 (not `aws-sdk-s3`)
//!
//! The AWS SDK pulls ~50 crates and meaningfully slows the workspace build.
//! Attachments need exactly one S3 verb (PUT object). The SigV4 signer here is
//! ~80 lines and is unit-tested against AWS's published GET test vector
//! (`sigv4_matches_aws_documented_example`), so the signing chain
//! (canonical request → string-to-sign → derived key → signature) is verified
//! deterministically with no live endpoint. The thin reqwest PUT wiring on top
//! is the only part not exercised offline.
//!
//! ## Isolation oracle
//!
//! This module lives in `feedbackmonk-api` and contains NO SQL — the
//! `multi-tenant-isolation-check` Probe A (no raw SQL outside the repository
//! crate) is satisfied by construction. Tenant/project scoping of attachment
//! rows happens entirely in `feedbackmonk-repository`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Errors from the storage layer.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("storage configuration error: {0}")]
    Config(String),
    #[error("storage io error: {0}")]
    Io(String),
    #[error("storage backend rejected the object: {0}")]
    Backend(String),
}

/// Persist opaque bytes under a caller-chosen key. Implementations are cheap to
/// clone (hold only config / an `Arc`-internal client) and `Send + Sync` so an
/// `Arc<dyn ObjectStore>` lives in `AppState`.
#[async_trait]
pub trait ObjectStore: Send + Sync {
    /// Store `bytes` at `key` with `content_type`. Returns the resolved
    /// fetch/public URL for the stored object.
    async fn put(
        &self,
        key: &str,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<String, StorageError>;
}

// ---------------------------------------------------------------------------
// Env-driven factory
// ---------------------------------------------------------------------------

/// Build the configured `ObjectStore` from the environment.
///
/// `FEEDBACKMONK_STORAGE_BACKEND` selects the backend (`local` default, or
/// `s3`). See `docs/operations/SELFHOST_ENV.md` § Attachment Storage for the
/// full var catalog.
pub fn from_env() -> Result<Arc<dyn ObjectStore>, StorageError> {
    let backend = std::env::var("FEEDBACKMONK_STORAGE_BACKEND")
        .unwrap_or_else(|_| "local".to_string());
    match backend.as_str() {
        "local" => {
            let dir = std::env::var("FEEDBACKMONK_STORAGE_LOCAL_DIR")
                .unwrap_or_else(|_| "./data/attachments".to_string());
            // Returned URLs prefix with the public base so the widget can
            // fetch them. Defaults off FEEDBACKMONK_PUBLIC_URL.
            let public_url = std::env::var("FEEDBACKMONK_PUBLIC_URL")
                .unwrap_or_else(|_| "http://localhost:14304".to_string());
            let url_base = format!("{}/attachments", public_url.trim_end_matches('/'));
            Ok(Arc::new(LocalFsStorage::new(dir, url_base)))
        }
        "s3" => {
            let cfg = S3Config::from_env()?;
            Ok(Arc::new(S3Storage::new(cfg)?))
        }
        other => Err(StorageError::Config(format!(
            "FEEDBACKMONK_STORAGE_BACKEND must be 'local' or 's3', got {other:?}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Local filesystem backend
// ---------------------------------------------------------------------------

/// Writes objects under a local directory. The default backend — zero external
/// dependencies, ideal for self-host (`docker compose` volume) and dev.
#[derive(Debug, Clone)]
pub struct LocalFsStorage {
    root: PathBuf,
    url_base: String,
}

impl LocalFsStorage {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>, url_base: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            url_base: url_base.into(),
        }
    }
}

#[async_trait]
impl ObjectStore for LocalFsStorage {
    async fn put(
        &self,
        key: &str,
        _content_type: &str,
        bytes: &[u8],
    ) -> Result<String, StorageError> {
        // Reject path-traversal in the key (defense in depth; the handler mints
        // keys from UUIDs, but never trust it implicitly).
        if key.contains("..") || Path::new(key).is_absolute() {
            return Err(StorageError::Config(format!("unsafe storage key: {key:?}")));
        }
        let dest = self.root.join(key);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| StorageError::Io(e.to_string()))?;
        }
        tokio::fs::write(&dest, bytes)
            .await
            .map_err(|e| StorageError::Io(e.to_string()))?;
        Ok(format!("{}/{}", self.url_base.trim_end_matches('/'), key))
    }
}

// ---------------------------------------------------------------------------
// S3-compatible backend
// ---------------------------------------------------------------------------

/// Configuration for the S3-compatible backend.
#[derive(Debug, Clone)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    /// Custom endpoint (e.g. `http://minio:9000`). `None` → AWS S3.
    pub endpoint: Option<String>,
    pub access_key_id: String,
    pub secret_access_key: String,
    /// Base URL for returned object URLs. `None` → derived from endpoint+bucket.
    pub public_base_url: Option<String>,
    /// Path-style addressing (`{endpoint}/{bucket}/{key}`). Required for MinIO.
    pub force_path_style: bool,
}

impl S3Config {
    fn from_env() -> Result<Self, StorageError> {
        let req = |name: &str| {
            std::env::var(name)
                .map_err(|_| StorageError::Config(format!("{name} is required when FEEDBACKMONK_STORAGE_BACKEND=s3")))
        };
        let endpoint = std::env::var("FEEDBACKMONK_S3_ENDPOINT").ok().filter(|s| !s.is_empty());
        // Default true when a custom endpoint is set (MinIO needs path-style).
        let force_path_style = std::env::var("FEEDBACKMONK_S3_FORCE_PATH_STYLE")
            .map_or_else(|_| endpoint.is_some(), |s| s == "true");
        Ok(Self {
            bucket: req("FEEDBACKMONK_S3_BUCKET")?,
            region: std::env::var("FEEDBACKMONK_S3_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
            endpoint,
            access_key_id: req("FEEDBACKMONK_S3_ACCESS_KEY_ID")?,
            secret_access_key: req("FEEDBACKMONK_S3_SECRET_ACCESS_KEY")?,
            public_base_url: std::env::var("FEEDBACKMONK_S3_PUBLIC_BASE_URL").ok().filter(|s| !s.is_empty()),
            force_path_style,
        })
    }
}

/// S3-compatible object store using a SigV4-signed PUT over reqwest.
#[derive(Clone)]
pub struct S3Storage {
    cfg: S3Config,
    client: reqwest::Client,
    /// `https://host` (no trailing slash) of the S3 endpoint.
    endpoint_origin: String,
    /// Host header value for the endpoint.
    host: String,
}

impl S3Storage {
    pub fn new(cfg: S3Config) -> Result<Self, StorageError> {
        let endpoint_origin = match &cfg.endpoint {
            Some(e) => e.trim_end_matches('/').to_string(),
            None => format!("https://s3.{}.amazonaws.com", cfg.region),
        };
        let host = endpoint_origin
            .split("://")
            .nth(1)
            .unwrap_or(&endpoint_origin)
            .to_string();
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| StorageError::Config(e.to_string()))?;
        Ok(Self {
            cfg,
            client,
            endpoint_origin,
            host,
        })
    }

    /// Canonical resource path for `key` (path-style includes the bucket).
    fn canonical_path(&self, key: &str) -> String {
        if self.cfg.force_path_style {
            format!("/{}/{}", self.cfg.bucket, key)
        } else {
            format!("/{key}")
        }
    }

    fn object_url(&self, key: &str) -> String {
        if let Some(base) = &self.cfg.public_base_url {
            return format!("{}/{}", base.trim_end_matches('/'), key);
        }
        if self.cfg.force_path_style {
            format!("{}/{}/{}", self.endpoint_origin, self.cfg.bucket, key)
        } else {
            // vhost-style endpoint: bucket is part of the host for AWS.
            format!("{}/{}", self.endpoint_origin, key)
        }
    }
}

#[async_trait]
impl ObjectStore for S3Storage {
    async fn put(
        &self,
        key: &str,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<String, StorageError> {
        let payload_hash = sha256_hex(bytes);
        let (amz_date, datestamp) = amz_timestamps();
        let canonical_uri = uri_encode_path(&self.canonical_path(key));

        // Signed headers (lowercase names), sorted lexically in the signer.
        let headers = vec![
            ("content-type".to_string(), content_type.to_string()),
            ("host".to_string(), self.host.clone()),
            ("x-amz-content-sha256".to_string(), payload_hash.clone()),
            ("x-amz-date".to_string(), amz_date.clone()),
        ];

        let signer = SigV4 {
            access_key: &self.cfg.access_key_id,
            secret_key: &self.cfg.secret_access_key,
            region: &self.cfg.region,
            service: "s3",
        };
        let authorization = signer.authorization_header(
            "PUT",
            &canonical_uri,
            "",
            &headers,
            &payload_hash,
            &amz_date,
            &datestamp,
        );

        let url = format!("{}{}", self.endpoint_origin, canonical_uri);
        let resp = self
            .client
            .put(&url)
            .header("content-type", content_type)
            .header("x-amz-content-sha256", &payload_hash)
            .header("x-amz-date", &amz_date)
            .header("authorization", authorization)
            .body(bytes.to_vec())
            .send()
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(StorageError::Backend(format!(
                "S3 PUT {key} → {status}: {body}"
            )));
        }
        Ok(self.object_url(key))
    }
}

// ---------------------------------------------------------------------------
// AWS Signature Version 4 (PUT object) — see module docs for verification.
// ---------------------------------------------------------------------------

struct SigV4<'a> {
    access_key: &'a str,
    secret_key: &'a str,
    region: &'a str,
    service: &'a str,
}

impl SigV4<'_> {
    /// Build the `Authorization` header value for the request.
    ///
    /// `headers` are `(lowercase-name, value)` pairs and MUST include `host`.
    /// They are sorted internally for the canonical-headers block.
    #[allow(clippy::too_many_arguments)] // SigV4 canonical request is inherently multi-field.
    #[allow(clippy::format_collect)] // canonical-headers block is clearest as a map+collect.
    fn authorization_header(
        &self,
        method: &str,
        canonical_uri: &str,
        canonical_querystring: &str,
        headers: &[(String, String)],
        payload_sha256_hex: &str,
        amz_date: &str,
        datestamp: &str,
    ) -> String {
        let mut sorted = headers.to_vec();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));

        let canonical_headers: String = sorted
            .iter()
            .map(|(k, v)| format!("{k}:{}\n", v.trim()))
            .collect();
        let signed_headers: String = sorted
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(";");

        let canonical_request = format!(
            "{method}\n{canonical_uri}\n{canonical_querystring}\n{canonical_headers}\n{signed_headers}\n{payload_sha256_hex}"
        );

        let credential_scope =
            format!("{datestamp}/{}/{}/aws4_request", self.region, self.service);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );

        let signing_key = self.signing_key(datestamp);
        let signature = hex(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));

        format!(
            "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key
        )
    }

    /// Derive the SigV4 signing key for `datestamp`.
    fn signing_key(&self, datestamp: &str) -> Vec<u8> {
        let k_secret = format!("AWS4{}", self.secret_key);
        let k_date = hmac_sha256(k_secret.as_bytes(), datestamp.as_bytes());
        let k_region = hmac_sha256(&k_date, self.region.as_bytes());
        let k_service = hmac_sha256(&k_region, self.service.as_bytes());
        hmac_sha256(&k_service, b"aws4_request")
    }
}

// ---------------------------------------------------------------------------
// Crypto + encoding helpers
// ---------------------------------------------------------------------------

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex(&h.finalize())
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(msg);
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    const HEXDIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEXDIGITS[(b >> 4) as usize] as char);
        out.push(HEXDIGITS[(b & 0x0f) as usize] as char);
    }
    out
}

/// URI-encode a path, preserving `/` separators (S3 canonical-URI rules).
fn uri_encode_path(path: &str) -> String {
    path.split('/')
        .map(uri_encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

/// Percent-encode one path segment per RFC 3986 unreserved set
/// (`A-Za-z0-9-_.~` unreserved; everything else encoded).
fn uri_encode_segment(seg: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(seg.len());
    for b in seg.bytes() {
        let unreserved = b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'_' | b'.' | b'~');
        if unreserved {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// Current UTC time as `(amz_date, datestamp)` = (`YYYYMMDDThhmmssZ`, `YYYYMMDD`).
fn amz_timestamps() -> (String, String) {
    let now = chrono::Utc::now();
    (
        now.format("%Y%m%dT%H%M%SZ").to_string(),
        now.format("%Y%m%d").to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_encodes_lowercase() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff, 0xab]), "000fffab");
    }

    #[test]
    fn sha256_hex_empty() {
        // Well-known SHA-256 of the empty string.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn uri_encode_preserves_slashes_encodes_specials() {
        assert_eq!(uri_encode_path("/bucket/a b/c.png"), "/bucket/a%20b/c.png");
        assert_eq!(uri_encode_path("/k/uuid.png"), "/k/uuid.png");
    }

    /// AWS SigV4 test-suite `get-vanilla` vector. Reproducing its signature
    /// proves the entire signing chain: canonical request → string-to-sign →
    /// derived signing key → HMAC. The expected value below is the published
    /// `get-vanilla` signature, independently re-derived with a stdlib Python
    /// `hmac`/`hashlib` reference for these exact inputs (so it is not a
    /// self-fulfilling copy of this implementation's own output).
    ///
    /// Inputs (`get-vanilla`):
    ///   access key  AKIDEXAMPLE
    ///   secret key  wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY
    ///   region      us-east-1   service `service`
    ///   GET / (no query)   host example.amazonaws.com   empty payload
    ///   x-amz-date  20150830T123600Z
    /// Expected: 5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31
    #[test]
    fn sigv4_matches_aws_documented_example() {
        let signer = SigV4 {
            access_key: "AKIDEXAMPLE",
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            region: "us-east-1",
            service: "service",
        };
        let headers = vec![
            ("host".to_string(), "example.amazonaws.com".to_string()),
            ("x-amz-date".to_string(), "20150830T123600Z".to_string()),
        ];
        let empty_payload =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let auth = signer.authorization_header(
            "GET",
            "/",
            "",
            &headers,
            empty_payload,
            "20150830T123600Z",
            "20150830",
        );
        assert!(
            auth.contains(
                "Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
            ),
            "SigV4 signature mismatch — got: {auth}"
        );
        assert!(auth.contains("SignedHeaders=host;x-amz-date"));
        assert!(auth.contains("Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request"));
    }

    #[tokio::test]
    async fn local_fs_round_trip_returns_url_and_writes_bytes() {
        let tmp = std::env::temp_dir().join(format!("fbm-store-test-{}", std::process::id()));
        let store = LocalFsStorage::new(&tmp, "http://localhost:14304/attachments");
        let url = store
            .put("p1/fb1/abc.png", "image/png", b"PNGDATA")
            .await
            .unwrap();
        assert_eq!(url, "http://localhost:14304/attachments/p1/fb1/abc.png");
        let written = tokio::fs::read(tmp.join("p1/fb1/abc.png")).await.unwrap();
        assert_eq!(written, b"PNGDATA");
        let _ = tokio::fs::remove_dir_all(&tmp).await;
    }

    #[tokio::test]
    async fn local_fs_rejects_traversal_keys() {
        let store = LocalFsStorage::new(".", "http://x");
        let err = store.put("../escape.png", "image/png", b"x").await.unwrap_err();
        assert!(matches!(err, StorageError::Config(_)));
    }
}
