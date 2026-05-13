//! Project repository. `ProjectRepo::open` is the SOLE constructor of
//! `ProjectScope` (Contract C1) -- it enforces tenant -> project ownership
//! at the type-system boundary and emits `RepoError::TenantProjectMismatch`
//! on a cross-tenant probe.

use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use feedbackr_core::Project;

use crate::error::{RepoError, Result};
use crate::scope::{ProjectScope, TenantScope};

#[async_trait]
pub trait ProjectRepo: Send + Sync {
    async fn create(&self, scope: &TenantScope, name: &str, slug: &str) -> Result<Project>;

    async fn list_for_tenant(&self, scope: &TenantScope) -> Result<Vec<Project>>;

    async fn get(&self, scope: &ProjectScope) -> Result<Project>;

    /// Mint a `ProjectScope` for `(tenant, project_id)`. Returns
    /// `RepoError::TenantProjectMismatch` if `project_id` is not owned by
    /// the tenant in `scope`. This is the SOLE constructor of
    /// `ProjectScope` outside the repository crate.
    async fn open(&self, scope: &TenantScope, project_id: Uuid) -> Result<ProjectScope>;
}

#[derive(Clone)]
pub struct SqlxProjectRepo {
    pool: PgPool,
}

impl SqlxProjectRepo {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProjectRepo for SqlxProjectRepo {
    async fn create(&self, scope: &TenantScope, name: &str, slug: &str) -> Result<Project> {
        let row = sqlx::query!(
            r#"
            INSERT INTO projects (tenant_id, name, slug)
            VALUES ($1, $2, $3)
            RETURNING id, tenant_id, name, slug, created_at
            "#,
            scope.tenant_id(),
            name,
            slug
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => RepoError::Conflict,
            other => RepoError::Sqlx(other),
        })?;

        Ok(Project {
            id: row.id,
            tenant_id: row.tenant_id,
            name: row.name,
            slug: row.slug,
            created_at: row.created_at,
        })
    }

    async fn list_for_tenant(&self, scope: &TenantScope) -> Result<Vec<Project>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, tenant_id, name, slug, created_at
            FROM projects WHERE tenant_id = $1
            ORDER BY created_at ASC
            "#,
            scope.tenant_id()
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Project {
                id: r.id,
                tenant_id: r.tenant_id,
                name: r.name,
                slug: r.slug,
                created_at: r.created_at,
            })
            .collect())
    }

    async fn get(&self, scope: &ProjectScope) -> Result<Project> {
        // Both project_id AND tenant_id constrain the lookup -- a cross-tenant
        // ProjectScope cannot be constructed (open() rejects it), but we add
        // the tenant_id predicate as a defense-in-depth bound.
        let row = sqlx::query!(
            r#"
            SELECT id, tenant_id, name, slug, created_at
            FROM projects WHERE id = $1 AND tenant_id = $2
            "#,
            scope.project_id(),
            scope.tenant_id()
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(RepoError::NotFound)?;

        Ok(Project {
            id: row.id,
            tenant_id: row.tenant_id,
            name: row.name,
            slug: row.slug,
            created_at: row.created_at,
        })
    }

    async fn open(&self, scope: &TenantScope, project_id: Uuid) -> Result<ProjectScope> {
        let row = sqlx::query!(
            "SELECT tenant_id FROM projects WHERE id = $1",
            project_id
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) if r.tenant_id == scope.tenant_id() => {
                Ok(ProjectScope::new(*scope, project_id))
            }
            Some(_) => Err(RepoError::TenantProjectMismatch),
            None => Err(RepoError::NotFound),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tenants::{SqlxTenantRepo, TenantRepo};
    use sqlx::PgPool;

    async fn seed_tenant(pool: &PgPool, email: &str) -> TenantScope {
        let trepo = SqlxTenantRepo::new(pool.clone());
        let t = trepo.create(email, "h").await.unwrap();
        trepo.scope_for(t.id).await.unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_and_list_for_tenant_returns_only_own_projects(pool: PgPool) {
        let repo = SqlxProjectRepo::new(pool.clone());
        let t1 = seed_tenant(&pool, "t1@example.com").await;
        let t2 = seed_tenant(&pool, "t2@example.com").await;

        let p1 = repo.create(&t1, "P1", "p1").await.unwrap();
        let p2a = repo.create(&t2, "P2a", "p2a").await.unwrap();
        let p2b = repo.create(&t2, "P2b", "p2b").await.unwrap();

        let t1_projects = repo.list_for_tenant(&t1).await.unwrap();
        let t2_projects = repo.list_for_tenant(&t2).await.unwrap();

        assert_eq!(t1_projects.len(), 1);
        assert_eq!(t1_projects[0].id, p1.id);

        assert_eq!(t2_projects.len(), 2);
        let ids: Vec<_> = t2_projects.iter().map(|p| p.id).collect();
        assert!(ids.contains(&p2a.id));
        assert!(ids.contains(&p2b.id));

        // Cross-tenant invariant: t2's projects are not in t1's list.
        assert!(!t1_projects.iter().any(|p| p.id == p2a.id));
        assert!(!t1_projects.iter().any(|p| p.id == p2b.id));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn open_rejects_cross_tenant_project(pool: PgPool) {
        let repo = SqlxProjectRepo::new(pool.clone());
        let t1 = seed_tenant(&pool, "t1@example.com").await;
        let t2 = seed_tenant(&pool, "t2@example.com").await;

        let p2 = repo.create(&t2, "P2", "p2").await.unwrap();

        // t1 attempts to open t2's project -- must fail with TenantProjectMismatch.
        let err = repo.open(&t1, p2.id).await.unwrap_err();
        assert!(matches!(err, RepoError::TenantProjectMismatch));

        // t2 opening their own project succeeds.
        let scope = repo.open(&t2, p2.id).await.unwrap();
        assert_eq!(scope.project_id(), p2.id);
        assert_eq!(scope.tenant_id(), t2.tenant_id());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn open_unknown_project_returns_not_found(pool: PgPool) {
        let repo = SqlxProjectRepo::new(pool.clone());
        let t = seed_tenant(&pool, "t@example.com").await;
        let err = repo.open(&t, Uuid::new_v4()).await.unwrap_err();
        assert!(matches!(err, RepoError::NotFound));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn create_duplicate_slug_within_tenant_yields_conflict(pool: PgPool) {
        let repo = SqlxProjectRepo::new(pool.clone());
        let t = seed_tenant(&pool, "t@example.com").await;
        repo.create(&t, "First", "shared-slug").await.unwrap();
        let err = repo.create(&t, "Second", "shared-slug").await.unwrap_err();
        assert!(matches!(err, RepoError::Conflict));
    }
}
