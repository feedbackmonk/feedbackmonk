//! Repository-level tests for the hosting registry (FR-FBR-32/33, migration 00030).
//!
//! These cover the properties the API layer *depends on* and therefore does not
//! re-prove: that `resolve_host` is single-valued, that a claim collision is a
//! conflict rather than a silent overwrite, that a cross-tenant delete is
//! indistinguishable from a missing row, and that resolution is genuinely inert
//! when no root domain is configured.
//!
//! Real Postgres per test via `sqlx::test` — DEC-FBR-03 makes the repository the
//! sole query path, so there is nothing meaningful to test against a fake.

use sqlx::PgPool;
use uuid::Uuid;

use feedbackmonk_core::hosting::{DomainKind, DomainStatus};
use feedbackmonk_repository::{
    DomainRepo, HostBinding, RepoError, SqlxDomainRepo, SqlxTenantRepo, TenantRepo, TenantScope,
};

const ROOT: &str = "feedbackmonk.test";

async fn seed_tenant(pool: &PgPool, email: &str, subdomain: Option<&str>) -> TenantScope {
    let tenants = SqlxTenantRepo::new(pool.clone());
    let t = tenants.create(email, "hash").await.unwrap();
    let scope = tenants.scope_for(t.id).await.unwrap();
    if let Some(label) = subdomain {
        tenants.set_subdomain(&scope, Some(label)).await.unwrap();
    }
    scope
}

#[sqlx::test(migrations = "../../migrations")]
async fn subdomain_resolves_to_its_tenant(pool: PgPool) {
    let scope = seed_tenant(&pool, "a@example.com", Some("alpha")).await;
    let domains = SqlxDomainRepo::new(pool.clone());

    let hit = domains
        .resolve_host(&format!("alpha.{ROOT}"), Some(ROOT))
        .await
        .unwrap();
    assert_eq!(
        hit,
        Some(HostBinding::TenantSubdomain {
            tenant_id: scope.tenant_id()
        })
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn unknown_and_apex_hosts_resolve_to_nothing(pool: PgPool) {
    seed_tenant(&pool, "a@example.com", Some("alpha")).await;
    let domains = SqlxDomainRepo::new(pool.clone());

    for host in [
        ROOT,                            // the apex is not a tenant
        &format!("nobody.{ROOT}"),       // an unclaimed label
        &format!("a.b.{ROOT}"),          // deeper than a wildcard cert covers
        "feedback.gitcellar.com",        // a stranger
        &format!("not{ROOT}"),           // the ends_with near-miss
    ] {
        assert_eq!(
            domains.resolve_host(host, Some(ROOT)).await.unwrap(),
            None,
            "{host} must not resolve"
        );
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn resolution_is_inert_without_a_root_domain(pool: PgPool) {
    seed_tenant(&pool, "a@example.com", Some("alpha")).await;
    let domains = SqlxDomainRepo::new(pool.clone());

    // The self-host posture: no root configured, so the subdomain the tenant
    // happens to hold means nothing. A custom domain still resolves — it is an
    // explicit, per-host registration, not a wildcard inference.
    assert_eq!(
        domains
            .resolve_host(&format!("alpha.{ROOT}"), None)
            .await
            .unwrap(),
        None
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn custom_domain_and_admin_alias_resolve_to_distinct_bindings(pool: PgPool) {
    let scope = seed_tenant(&pool, "a@example.com", Some("alpha")).await;
    let domains = SqlxDomainRepo::new(pool.clone());

    let public = domains
        .claim(&scope, "feedback.acme.example", DomainKind::Public)
        .await
        .unwrap();
    domains
        .claim(&scope, "triage.acme.example", DomainKind::AdminAlias)
        .await
        .unwrap();

    assert_eq!(public.status, DomainStatus::Pending, "claims start pending");

    assert_eq!(
        domains
            .resolve_host("feedback.acme.example", Some(ROOT))
            .await
            .unwrap(),
        Some(HostBinding::CustomDomain {
            tenant_id: scope.tenant_id(),
            domain_id: public.id
        })
    );
    // The alias must NOT come back as a tenant binding — a single enum variant
    // for both would let a `match` arm serve tenant content on an admin host.
    assert_eq!(
        domains
            .resolve_host("triage.acme.example", Some(ROOT))
            .await
            .unwrap(),
        Some(HostBinding::AdminAlias {
            tenant_id: scope.tenant_id()
        })
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn a_domain_can_be_claimed_by_only_one_tenant(pool: PgPool) {
    let a = seed_tenant(&pool, "a@example.com", Some("alpha")).await;
    let b = seed_tenant(&pool, "b@example.com", Some("bravo")).await;
    let domains = SqlxDomainRepo::new(pool.clone());

    domains
        .claim(&a, "feedback.acme.example", DomainKind::Public)
        .await
        .unwrap();

    // UNIQUE(domain) is what makes host resolution single-valued. A second
    // claim must CONFLICT, never overwrite — an overwrite would silently move
    // a live customer domain to a different tenant.
    let err = domains
        .claim(&b, "feedback.acme.example", DomainKind::Public)
        .await
        .unwrap_err();
    assert!(matches!(err, RepoError::Conflict), "got {err:?}");

    assert_eq!(
        domains
            .resolve_host("feedback.acme.example", Some(ROOT))
            .await
            .unwrap()
            .map(HostBinding::tenant_id),
        Some(a.tenant_id()),
        "the original owner keeps the domain"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn two_tenants_cannot_share_a_subdomain(pool: PgPool) {
    let tenants = SqlxTenantRepo::new(pool.clone());
    seed_tenant(&pool, "a@example.com", Some("alpha")).await;
    let b = seed_tenant(&pool, "b@example.com", None).await;

    let err = tenants.set_subdomain(&b, Some("alpha")).await.unwrap_err();
    assert!(matches!(err, RepoError::Conflict), "got {err:?}");
}

#[sqlx::test(migrations = "../../migrations")]
async fn subdomain_can_be_cleared_and_stops_resolving(pool: PgPool) {
    let tenants = SqlxTenantRepo::new(pool.clone());
    let scope = seed_tenant(&pool, "a@example.com", Some("alpha")).await;
    let domains = SqlxDomainRepo::new(pool.clone());

    assert_eq!(tenants.get_subdomain(&scope).await.unwrap().as_deref(), Some("alpha"));
    tenants.set_subdomain(&scope, None).await.unwrap();
    assert_eq!(tenants.get_subdomain(&scope).await.unwrap(), None);
    assert_eq!(
        domains
            .resolve_host(&format!("alpha.{ROOT}"), Some(ROOT))
            .await
            .unwrap(),
        None
    );

    // And the freed label is claimable by someone else — NULLs do not collide
    // under the UNIQUE index.
    let b = seed_tenant(&pool, "b@example.com", None).await;
    tenants.set_subdomain(&b, Some("alpha")).await.unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn delete_is_tenant_scoped_and_leaks_no_existence(pool: PgPool) {
    let a = seed_tenant(&pool, "a@example.com", Some("alpha")).await;
    let b = seed_tenant(&pool, "b@example.com", Some("bravo")).await;
    let domains = SqlxDomainRepo::new(pool.clone());

    let claimed = domains
        .claim(&a, "feedback.acme.example", DomainKind::Public)
        .await
        .unwrap();

    // B deleting A's domain, and B deleting a domain that does not exist, must
    // be indistinguishable from outside.
    let cross = domains.delete(&b, claimed.id).await.unwrap_err();
    let missing = domains.delete(&b, Uuid::new_v4()).await.unwrap_err();
    assert!(matches!(cross, RepoError::NotFound), "got {cross:?}");
    assert!(matches!(missing, RepoError::NotFound), "got {missing:?}");

    // A's domain is untouched.
    assert_eq!(domains.list_for_tenant(&a).await.unwrap().len(), 1);
    domains.delete(&a, claimed.id).await.unwrap();
    assert!(domains.list_for_tenant(&a).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn list_is_scoped_to_the_owning_tenant(pool: PgPool) {
    let a = seed_tenant(&pool, "a@example.com", Some("alpha")).await;
    let b = seed_tenant(&pool, "b@example.com", Some("bravo")).await;
    let domains = SqlxDomainRepo::new(pool.clone());

    domains.claim(&a, "a1.example", DomainKind::Public).await.unwrap();
    domains.claim(&a, "a2.example", DomainKind::Public).await.unwrap();
    domains.claim(&b, "b1.example", DomainKind::Public).await.unwrap();

    let a_domains = domains.list_for_tenant(&a).await.unwrap();
    assert_eq!(a_domains.len(), 2);
    assert!(a_domains.iter().all(|d| d.tenant_id == a.tenant_id()));
    assert_eq!(domains.list_for_tenant(&b).await.unwrap().len(), 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn mark_active_is_idempotent_and_stamps_once(pool: PgPool) {
    let a = seed_tenant(&pool, "a@example.com", Some("alpha")).await;
    let domains = SqlxDomainRepo::new(pool.clone());
    let claimed = domains
        .claim(&a, "feedback.acme.example", DomainKind::Public)
        .await
        .unwrap();

    domains.mark_active(&a, claimed.id).await.unwrap();
    let first = domains.list_for_tenant(&a).await.unwrap().remove(0);
    assert_eq!(first.status, DomainStatus::Active);
    let stamped = first.verified_at.expect("verified_at set on activation");

    // Called on every certificate ask, so a second call must not move the
    // timestamp — `verified_at` records FIRST observation, not most recent.
    domains.mark_active(&a, claimed.id).await.unwrap();
    let second = domains.list_for_tenant(&a).await.unwrap().remove(0);
    assert_eq!(second.verified_at, Some(stamped));

    // An unknown id is a no-op, not an error: this runs on the request path and
    // must never fail a request.
    domains.mark_active(&a, Uuid::new_v4()).await.unwrap();

    // And another tenant cannot flip a row they do not own — the scope is in
    // the WHERE clause, so this is a silent no-op rather than a cross-tenant
    // write.
    let b = seed_tenant(&pool, "b@example.com", Some("bravo")).await;
    let b_claim = domains
        .claim(&b, "other.acme.example", DomainKind::Public)
        .await
        .unwrap();
    domains.mark_active(&a, b_claim.id).await.unwrap();
    assert_eq!(
        domains.list_for_tenant(&b).await.unwrap()[0].status,
        DomainStatus::Pending,
        "tenant A must not be able to activate tenant B's domain"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn domains_cascade_when_the_tenant_is_deleted(pool: PgPool) {
    let a = seed_tenant(&pool, "a@example.com", Some("alpha")).await;
    let domains = SqlxDomainRepo::new(pool.clone());
    domains
        .claim(&a, "feedback.acme.example", DomainKind::Public)
        .await
        .unwrap();

    sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(a.tenant_id())
        .execute(&pool)
        .await
        .unwrap();

    // A dangling registry row would keep resolving a hostname to a tenant that
    // no longer exists.
    assert_eq!(
        domains
            .resolve_host("feedback.acme.example", Some(ROOT))
            .await
            .unwrap(),
        None
    );
}
