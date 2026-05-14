# feedbackmonk — Architecture

**Status**: P0 Stage 1 SHIPPED — repository layer + core types + placeholder API crate. P0 Stage 2 (signup + submission path) is next; arc continues through P4.

---

## Components (P0 Stage 1 — IMPLEMENTED)

| Component ID | Component | Crate | Layer | Purpose |
|---|---|---|---|---|
| **CMP-FBR-CORE-01** | `feedbackmonk-core` | `crates/feedbackmonk-core/` | data | Pure domain types (no DB, no async, no network). Records mirror P0 schema; backs the request/response shapes that Stage 2 surfaces will use. |
| **CMP-FBR-REPO-01** | `feedbackmonk-repository` | `crates/feedbackmonk-repository/` | DB | The SOLE query path. Four repository traits (`TenantRepo`, `ProjectRepo`, `SigningKeyRepo`, `FeedbackRepo`) with sqlx-backed implementations. `TenantScope` / `ProjectScope` newtypes enforce tenant isolation at the type system. Contract C1 frozen for Stage 2 consumption. |
| **CMP-FBR-API-01** | `feedbackmonk-api` | `crates/feedbackmonk-api/` | HTTP | Stage 1 ships a placeholder axum binary binding `FEEDBACKMONK_PORT` (default `14304`). Stage 2 Workers A + B add the real router tree. |
| **CMP-FBR-SCHEMA-01** | P0 schema | `migrations/00001_p0_schema.sql` | persistence | Tables for `tenants`, `projects`, `signing_keys`, `feedback`, `anon_submissions`, `rate_limit_counters`. Authoritative source for column names. |
| **CMP-FBR-ORACLE-01** | `multi-tenant-isolation-check` | `.claude/oracles/multi-tenant-isolation-check/` | verification | Verification Oracle (built as P0 Task Zero). AST-grade enforcement of DEC-FBR-03 "raw SQL outside repository = security incident." Three-leg defense: type system (CMP-FBR-REPO-01) + AST oracle (this) + clippy/cargo-deny (workspace baseline + pedantic on repo crate). |

## Components (P0 Stage 2 — PLANNED)

| Component ID | Component | Crate | Layer | Owner |
|---|---|---|---|---|
| **CMP-FBR-JWT-01** | `feedbackmonk-jwt` | `crates/feedbackmonk-jwt/` (forthcoming) | auth | Worker B Task Zero |
| **CMP-FBR-ANON-01** | `feedbackmonk-anon` | `crates/feedbackmonk-anon/` (forthcoming) | rate-limit | Worker B |

## Components (later phases — DEFERRED)

- **CMP-FBR-ADMIN-UI-01** — Admin UI (FR-FBR-07, P1)
- **CMP-FBR-WIDGET-01** — Embeddable widget (FR-FBR-04, P2)
- **CMP-FBR-ROADMAP-01** — Public roadmap page (FR-FBR-11, P2)
- **CMP-FBR-BILLING-01** — Polar billing (FR-FBR-15, P3)
- **CMP-FBR-MARKETING-01** — Astro marketing site (FR-FBR-16, P4)

## Reference implementation

The GitCellar-integrated feedback system at `gitcellar-cloud/src/feedback/` is the working reference. feedbackmonk's architecture borrows from it but diverges on:

- **Multi-tenancy** — GitCellar is single-tenant; feedbackmonk must support multiple customer organizations
- **Auth** — GitCellar uses PassKey-native (Ed25519); feedbackmonk customers' end-users will not
- **Roadmap backend** — GitCellar uses a Gitea fork (Cloud Forge); feedbackmonk likely uses native DB + UI
- **Storage** — GitCellar's B2/R2 dual-region is GitCellar-specific (home-region routing on a GitCellar user); feedbackmonk needs a generic storage abstraction
- **Branding** — Hardcoded "GitCellar" strings need to be config-driven

## Components (legacy speculative shape — superseded by the table above)

The original spec session sketched the component shape before code landed; the above tables now reflect implementation reality (P0 Stage 1 SHIPPED). For historical context the original speculative list was:

- ~~`feedback-core` extracted reusable library at `Shared/feedback-core/`~~ — superseded by three-crate split (`-core`, `-repository`, `-api`) per DEC-FBR-07 (greenfield repo, not shared library).
- ~~Backend service "sibling to gitcellar-cloud"~~ — superseded by standalone repo at `E:\Developer\SourceControlled\Apps\Feedbackr\` per DEC-FBR-07.
- Admin dashboard, widget, public roadmap, marketing site: still planned per the deferred-components table.

## Data flow (to be defined)

Pending.

## Decisions

See [`DECISIONS.md`](DECISIONS.md).
