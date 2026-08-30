//! Hostname vocabulary for the commercial hosting shape (FR-FBR-32 / FR-FBR-33).
//!
//! Pure functions only — no DB, no network, no config. This module owns the
//! *rules* about hostnames; `feedbackmonk-repository` owns the lookup and
//! `feedbackmonk-api` owns the routing consequences.
//!
//! Two jobs:
//!
//! 1. **Normalisation.** A `Host` header is attacker-controlled and arrives in
//!    many equivalent spellings — `Example.COM`, `example.com:8443`,
//!    `example.com.` (the fully-qualified trailing dot), IPv6 in brackets. Every
//!    one of those must reduce to the same key before it is compared to anything,
//!    or a comparison-based guard can be walked around by changing the case of a
//!    letter. `normalize_host` is the single reduction, and the storage layer
//!    stores only normalised forms.
//!
//! 2. **Subdomain label policy.** `{label}.{root_domain}` must be a legal DNS
//!    label AND must not collide with a hostname the platform itself needs.
//!    Letting a tenant claim `api`, `www` or the admin label would let them serve
//!    tenant-controlled content on an origin the platform's own surfaces live on
//!    — the reserved set is a security boundary, not cosmetics.
//!
//! Lineage: DEC-FBR-13 (public-surface URL shape) · DEC-FBR-IMPL-28 (binding).

/// Hostname labels a tenant may never claim as their subdomain.
///
/// Two categories, deliberately merged into one list because the consequence is
/// identical (a tenant serving content on a platform-owned origin):
///
/// - **Platform surfaces**: `app`/`admin`/`dashboard` (the admin console),
///   `api`/`cdn`/`static`/`assets` (the widget + API delivery hosts DEC-FBR-13's
///   table names), `www` and the marketing surfaces.
/// - **Infrastructure conventions**: mail and nameserver labels, whose
///   mis-assignment breaks mail delivery or DNS for the whole zone.
///
/// Extending this list is a one-way door for anyone who already claimed the
/// label, so additions belong with a migration that checks for collisions.
pub const RESERVED_LABELS: &[&str] = &[
    // platform surfaces
    "app",
    "admin",
    "dashboard",
    "api",
    "cdn",
    "static",
    "assets",
    "www",
    "docs",
    "blog",
    "status",
    "support",
    "help",
    "account",
    "accounts",
    "billing",
    "login",
    "logout",
    "auth",
    "signup",
    "register",
    "public",
    // brand
    "feedbackmonk",
    "monk",
    "feedback",
    // infrastructure conventions
    "mail",
    "smtp",
    "imap",
    "pop",
    "ftp",
    "ns",
    "ns1",
    "ns2",
    "mx",
    "localhost",
    // environments
    "test",
    "staging",
    "stage",
    "dev",
    "demo",
    "preview",
    "internal",
];

/// Why a proposed subdomain label was rejected.
///
/// Distinguished rather than collapsed to a bool because the API maps them to
/// different HTTP statuses: a malformed label is a 400 (the caller's mistake),
/// a reserved one is a 409 (well-formed but unavailable — the same shape as a
/// name already taken).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubdomainError {
    /// Shorter than 3 or longer than 63 characters.
    BadLength,
    /// Contains something other than `[a-z0-9-]`, or starts/ends with a hyphen.
    IllegalCharacters,
    /// All digits — ambiguous with an IPv4 octet and rejected by some resolvers.
    AllNumeric,
    /// Well-formed but on [`RESERVED_LABELS`].
    Reserved,
}

impl SubdomainError {
    /// Operator-facing explanation. Safe to return to the tenant: it describes
    /// their own input and reveals nothing about other tenants.
    #[must_use]
    pub fn as_message(self) -> &'static str {
        match self {
            Self::BadLength => "subdomain must be between 3 and 63 characters",
            Self::IllegalCharacters => {
                "subdomain may contain only lowercase letters, digits and hyphens, \
                 and may not start or end with a hyphen"
            }
            Self::AllNumeric => "subdomain may not be entirely numeric",
            Self::Reserved => "that subdomain is reserved",
        }
    }
}

/// Reduce a `Host` header (or a stored hostname) to its canonical comparison key.
///
/// Returns `None` when nothing usable remains — an empty header, a bare port, or
/// a value consisting only of separators. Callers MUST treat `None` as
/// "unresolvable host", never as "default host".
///
/// Handles, in order: surrounding whitespace, ASCII case, the IPv6 bracket form
/// (`[::1]:8443` → `[::1]`), a `:port` suffix, and the FQDN trailing dot.
#[must_use]
pub fn normalize_host(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lowered = trimmed.to_ascii_lowercase();

    // IPv6 literals are bracketed, and the brackets are what make the ':port'
    // split unambiguous. Keep the brackets: they are part of the canonical form,
    // and an IPv6 host never resolves to a tenant anyway (it cannot be CNAMEd).
    let without_port = if lowered.starts_with('[') {
        // A malformed bracket form is refused rather than guessed at.
        let close = lowered.find(']')?;
        &lowered[..=close]
    } else {
        match lowered.split_once(':') {
            Some((host, _port)) => host,
            None => lowered.as_str(),
        }
    };

    let host = without_port.trim_end_matches('.');
    if host.is_empty() {
        return None;
    }
    Some(host.to_string())
}

/// Validate a proposed tenant subdomain label against DNS rules and the
/// platform's reserved set.
///
/// The input is expected to be already lowercased by the caller (the API
/// lowercases at the boundary); an uppercase character is reported as
/// [`SubdomainError::IllegalCharacters`] rather than silently coerced, so a
/// tenant sees what they actually stored.
///
/// # Errors
/// Returns the specific [`SubdomainError`] so callers can map to distinct HTTP
/// statuses (400 for malformed, 409 for reserved).
pub fn validate_subdomain_label(label: &str) -> Result<(), SubdomainError> {
    if label.len() < 3 || label.len() > 63 {
        return Err(SubdomainError::BadLength);
    }
    if !label
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(SubdomainError::IllegalCharacters);
    }
    if label.starts_with('-') || label.ends_with('-') {
        return Err(SubdomainError::IllegalCharacters);
    }
    if label.bytes().all(|b| b.is_ascii_digit()) {
        return Err(SubdomainError::AllNumeric);
    }
    if RESERVED_LABELS.contains(&label) {
        return Err(SubdomainError::Reserved);
    }
    Ok(())
}

/// If `host` sits directly under `root_domain`, return its leading label.
///
/// Both arguments must already be normalised. Returns `None` for the apex itself
/// (`root_domain` alone is not a tenant), for unrelated hosts, and — importantly
/// — for **multi-level** subdomains: `a.b.root` yields `None`, not `"a.b"`.
/// Wildcard TLS covers exactly one level, so accepting deeper names would mint a
/// tenant binding for a host that can never present a valid certificate.
#[must_use]
pub fn subdomain_label_of<'h>(host: &'h str, root_domain: &str) -> Option<&'h str> {
    if root_domain.is_empty() {
        return None;
    }
    let suffix_len = root_domain.len() + 1; // the separating dot
    if host.len() <= suffix_len {
        return None;
    }
    let (label, rest) = host.split_at(host.len() - suffix_len);
    // `rest` is ".{root_domain}" when this host is under the root.
    if rest.get(1..) != Some(root_domain) || !rest.starts_with('.') {
        return None;
    }
    if label.is_empty() || label.contains('.') {
        return None;
    }
    Some(label)
}


// ===========================================================================
// `tenant_domains` column vocabulary (migration 00030)
// ===========================================================================
//
// These live in core, beside `Tier` / `ModerationStatus` / `FeedbackStatus`,
// for the same reason those do: a DB enum's legal value set is domain
// vocabulary, not query machinery. Keeping it here also means the parse
// helpers are not repository-crate free functions, which the
// `multi-tenant-isolation-check` oracle would (correctly) flag for taking
// something other than a scope as their first argument.

/// A `tenant_domains.kind` / `.status` column held a value outside its CHECK
/// constraint — schema and code have drifted apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainValueError;

impl core::fmt::Display for DomainValueError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("unrecognised tenant_domains column value")
    }
}

impl std::error::Error for DomainValueError {}

/// `tenant_domains.kind` — mirrors the CHECK in migration 00030 byte-for-byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainKind {
    /// The sellable FR-FBR-33 surface: board + widget/API.
    Public,
    /// Operator-registered redirect-only host (DEC-FBR-IMPL-27).
    AdminAlias,
}

impl DomainKind {
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::AdminAlias => "admin_alias",
        }
    }

    /// Parse a `tenant_domains.kind` value read back from Postgres.
    ///
    /// # Errors
    /// [`DomainValueError`] if the column holds a value outside the CHECK set —
    /// which would mean the schema and this enum have drifted apart.
    pub fn from_db_str(s: &str) -> Result<Self, DomainValueError> {
        match s {
            "public" => Ok(Self::Public),
            "admin_alias" => Ok(Self::AdminAlias),
            _ => Err(DomainValueError),
        }
    }
}

/// `tenant_domains.status` — mirrors the CHECK in migration 00030.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainStatus {
    /// Claimed; DNS has not yet been observed pointing here.
    Pending,
    /// Traffic or certificate issuance has been seen for this host.
    Active,
}

impl DomainStatus {
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
        }
    }

    /// Parse a `tenant_domains.status` value read back from Postgres.
    ///
    /// # Errors
    /// [`DomainValueError`] on a value outside the CHECK set (schema drift).
    pub fn from_db_str(s: &str) -> Result<Self, DomainValueError> {
        match s {
            "pending" => Ok(Self::Pending),
            "active" => Ok(Self::Active),
            _ => Err(DomainValueError),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_lowercases_and_strips_port() {
        assert_eq!(normalize_host("Example.COM:8443").as_deref(), Some("example.com"));
        assert_eq!(normalize_host("  acme.feedbackmonk.com  ").as_deref(), Some("acme.feedbackmonk.com"));
    }

    #[test]
    fn normalize_strips_fqdn_trailing_dot() {
        // The trailing-dot form is legal in a Host header and is the classic way
        // to slip past a naive string comparison.
        assert_eq!(normalize_host("acme.feedbackmonk.com.").as_deref(), Some("acme.feedbackmonk.com"));
        assert_eq!(
            normalize_host("acme.feedbackmonk.com.:443").as_deref(),
            Some("acme.feedbackmonk.com")
        );
    }

    #[test]
    fn normalize_rejects_empty_and_portonly() {
        assert_eq!(normalize_host(""), None);
        assert_eq!(normalize_host("   "), None);
        assert_eq!(normalize_host(":443"), None);
        assert_eq!(normalize_host("."), None);
    }

    #[test]
    fn normalize_handles_ipv6_bracket_form() {
        assert_eq!(normalize_host("[::1]:14304").as_deref(), Some("[::1]"));
        assert_eq!(normalize_host("[::1]").as_deref(), Some("[::1]"));
        // A malformed bracket form is refused rather than guessed at.
        assert_eq!(normalize_host("[::1"), None);
    }

    #[test]
    fn valid_labels_accepted() {
        for label in ["acme", "gitcellar", "acme-corp", "a1b2", "x9z"] {
            assert!(validate_subdomain_label(label).is_ok(), "{label} should be valid");
        }
    }

    #[test]
    fn label_length_bounds_enforced() {
        assert_eq!(validate_subdomain_label("ab"), Err(SubdomainError::BadLength));
        assert_eq!(validate_subdomain_label(&"a".repeat(64)), Err(SubdomainError::BadLength));
        assert!(validate_subdomain_label(&"a".repeat(63)).is_ok());
    }

    #[test]
    fn label_charset_enforced() {
        for bad in ["Acme", "acme_corp", "acme.corp", "-acme", "acme-", "acme corp", "acmé"] {
            assert_eq!(
                validate_subdomain_label(bad),
                Err(SubdomainError::IllegalCharacters),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn all_numeric_label_rejected() {
        assert_eq!(validate_subdomain_label("12345"), Err(SubdomainError::AllNumeric));
    }

    #[test]
    fn reserved_labels_rejected() {
        // The load-bearing ones: claiming these would put tenant-controlled
        // content on a platform origin.
        for reserved in ["app", "admin", "api", "cdn", "www", "feedbackmonk"] {
            assert_eq!(
                validate_subdomain_label(reserved),
                Err(SubdomainError::Reserved),
                "{reserved} must stay reserved"
            );
        }
    }

    #[test]
    fn subdomain_label_extracted_under_root() {
        assert_eq!(
            subdomain_label_of("acme.feedbackmonk.com", "feedbackmonk.com"),
            Some("acme")
        );
    }

    #[test]
    fn apex_is_not_a_tenant() {
        assert_eq!(subdomain_label_of("feedbackmonk.com", "feedbackmonk.com"), None);
    }

    #[test]
    fn unrelated_host_has_no_label() {
        assert_eq!(subdomain_label_of("feedback.gitcellar.com", "feedbackmonk.com"), None);
        // The near-miss that a naive `ends_with` would accept: a host that
        // merely ENDS in the root-domain string without the dot boundary.
        assert_eq!(subdomain_label_of("evilfeedbackmonk.com", "feedbackmonk.com"), None);
    }

    #[test]
    fn multi_level_subdomain_is_not_a_tenant() {
        // Wildcard TLS covers exactly one level; binding a deeper host would
        // mint a tenant scope for a name that can never present a valid cert.
        assert_eq!(
            subdomain_label_of("a.b.feedbackmonk.com", "feedbackmonk.com"),
            None
        );
    }

    #[test]
    fn empty_root_domain_never_matches() {
        assert_eq!(subdomain_label_of("acme.feedbackmonk.com", ""), None);
    }
}
