//! Tenant- and project-scope newtypes -- the type-system half of the
//! multi-tenant isolation defense (DEC-FBR-03).
//!
//! Construction is `pub(crate)` only. Callers MUST obtain a `TenantScope`
//! through an authenticated repository call (e.g. `TenantRepo::login`,
//! Stage 2 work) and a `ProjectScope` through `ProjectRepo::open`, which
//! proves tenant -> project ownership at the type-system boundary.

use uuid::Uuid;

/// Carries tenant identity through every repository call. NEVER constructed
/// outside an authenticated session boundary -- constructors are `pub(crate)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TenantScope {
    tenant_id: Uuid,
}

impl TenantScope {
    /// Crate-private constructor. The repository layer mints `TenantScope`
    /// values only after verifying that the caller's credentials match an
    /// existing tenant row (Stage 2 work).
    pub(crate) fn new(tenant_id: Uuid) -> Self {
        Self { tenant_id }
    }

    #[must_use]
    pub fn tenant_id(&self) -> Uuid {
        self.tenant_id
    }
}

/// Project-scoped operations require a `ProjectScope`, which can only be
/// constructed by `ProjectRepo::open`. That method enforces that the
/// supplied `project_id` belongs to the tenant in the supplied scope --
/// returning `RepoError::TenantProjectMismatch` otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProjectScope {
    tenant: TenantScope,
    project_id: Uuid,
}

impl ProjectScope {
    pub(crate) fn new(tenant: TenantScope, project_id: Uuid) -> Self {
        Self { tenant, project_id }
    }

    #[must_use]
    pub fn tenant(&self) -> &TenantScope {
        &self.tenant
    }

    #[must_use]
    pub fn project_id(&self) -> Uuid {
        self.project_id
    }

    #[must_use]
    pub fn tenant_id(&self) -> Uuid {
        self.tenant.tenant_id
    }
}
