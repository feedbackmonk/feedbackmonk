# Execution Plan — feedbackmonk P0 — Foundation
**Source**: /0-uldf-ldis-plan
**Generated**: 2026-05-13T21:01:33Z
**Task**: Build the P0 (Foundation) slice of feedbackmonk v1 — multi-tenant data model + tenant-scoped repository layer + customer signup/onboarding + submission API (JWT-verified + anonymous) + EdDSA JWT verification + health/observability. Exit gate: "tenant signup → create first project → POST feedback works end-to-end via curl, multi-tenant-isolation-check oracle green, /health returns structured JSON."
**Strategy**: STAGED (3 stages; Stage 2 is the 2-3-worker parallel fan-out; Stages 1 and 3 are single-agent)
**Arc Plan**: `docs/planning/plans/20260513T185711-feedbackmonk-v1-build-arc.md`
**Spec Source**: `docs/specs/SPECIFICATION.md` (FR-FBR-01, FR-FBR-02, FR-FBR-03, FR-FBR-05, FR-FBR-06, FR-FBR-18) + `docs/specs/DECISIONS.md` (DEC-FBR-03, DEC-FBR-04, DEC-FBR-07)
**Intake Source**: derived from arc plan + spec (no new intake needed at phase scope)

---

## Strategy Rationale

### Why STAGED with three stages (not flat PARALLEL, not flat SEQUENTIAL)

The arc plan already committed to STAGED at arc scope; this phase plan refines it as **three stages within P0**:

| Stage | Scope | Topology | Why |
|---|---|---|---|
| **Stage 1** — Foundation contract | Task Zero (`multi-tenant-isolation-check` oracle) + Sub-task 1 (data model + tenant-scoped repository layer, FR-FBR-01) | **SEQUENTIAL** (1 agent) | The repository API surface is the contract every other P0+ surface consumes. Freezing it sequentially in one head — with the oracle policing from commit 1 — eliminates the dominant fidelity risk (Q2=5) before parallel workers can drift apart on the contract's edges. |
| **Stage 2** — Parallel surfaces | Sub-tasks 2 + 3 (FR-FBR-03 submission API + FR-FBR-05 JWT EdDSA + FR-FBR-06 anonymous mode; FR-FBR-02 signup/onboarding) | **PODS, 2 workers** (see decomposition below) | Once Stage 1's contract is frozen, FR-FBR-02 signup/onboarding is fully independent of the submission path; FR-FBR-03 + 05 + 06 form a coherent submission-path bundle. Worker boundaries are clean (no shared mutable state); each worker owns ~30-40% of P0 scope; specialization is real (auth/crypto on one worker vs. CRUD-flow ergonomics on the other). |
| **Stage 3** — Observability | Sub-task 4 (FR-FBR-18 health + structured logging) | **SEQUENTIAL** (1 agent, in the converging session) | Small port-pattern work consuming both Stage 2 outputs; runs once Stage 2 converges so all error-paths are wired and the log/health surfaces see real traffic shapes. |

PARALLEL across Stage 1 + Stage 2 would race contract definition against contract consumers — the textbook way to ship a fragile multi-tenant layer. SEQUENTIAL through all four sub-tasks would burn ~30% of Stage 2's potential parallelism for no fidelity gain (Stage 2 boundaries are clean).

### Collaboration Value Assessment (P0 scope)

| Factor | Score (1-5) | Notes |
|---|---|---|
| **Specialization** | 3 | Stage 2's two workers diverge meaningfully — auth/crypto/rate-limit on Worker B vs. CRUD/email-verify/admin-surface on Worker A. |
| **Quality** | 4 | JWT verification (alg-confusion, aud-binding, key-rotation race), anon-mode dedup, and signup-flow email semantics each benefit from focused review. |
| **Discovery** | 2 | GitCellar reference impl reduces unknowns substantially. Most Stage 2 work is "port the pattern, adapt to multi-tenant." |
| **Speed** | 3 | Stage 2 parallelism converts ~1 calendar-week to ~3-4 days; meaningful but not decisive at P0 scope. |
| **Boundary Clarity** | 5 | Stage 1 freezes the repository contract; Workers in Stage 2 consume it as a frozen library surface. Contract is documented before fan-out. |
| **Coupling** | 4 | Stage 2 workers share only the repository API and the JWT fixture corpus (both frozen carry-state); no in-flight inter-worker dependencies. |

**Value**: 12/20. **Friction (higher=less friction)**: 9/10. **Net**: 12 − (10 − 9)/2 = **11.5** → PARALLEL strongly recommended within Stage 2.

### Why 2 workers in Stage 2, not 3

The arc plan's intra-phase note suggested "sub-tasks 2 + 3 parallel" with up to 3 branches (FR-FBR-05, FR-FBR-06, FR-FBR-03 each potentially separate). Refining at P0 plan time: **FR-FBR-03 (submission API) and FR-FBR-05 (JWT) and FR-FBR-06 (anonymous mode) share a single hot code path** (the `POST /feedback` handler must decide auth-mode, call the right verifier, accept-or-reject, and the rate-limiter sits inline). Splitting these across three workers forces a synthetic seam at the handler boundary and risks three independent re-implementations of "what does an accepted submission look like." Better: one worker (B) owns the entire submission path including JWT verification and anonymous mode. The other worker (A) owns the orthogonal signup/onboarding surface.

This also lowers per-worker context pressure: each worker holds the repository contract + their own surface, not the repository contract + everyone else's surface.

---

## Context Budget Assessment

### Stage 1 (single agent, contract-defining)

| Item | Tokens (estimate) |
|---|---|
| Spec section (SPECIFICATION.md FR-FBR-01) + DEC-FBR-03 + DEC-FBR-04 | ~6k |
| Arc plan §P0 + this plan's Stage 1 brief | ~5k |
| GitCellar reference reads (`feedback/db.rs`, `feedback/routes.rs`, schema migrations) | ~15-25k |
| Implementation + tests + oracle build | ~35-50k |
| Reasoning reserve | ~25-30k |
| **Total** | **~85-115k → comfortably within 1M context (~10% utilization)** |

Pass.

### Stage 2 — Worker A (Signup/Onboarding)

| Item | Tokens |
|---|---|
| Spec section (FR-FBR-02) + DEC-FBR-04 + DEC-FBR-05 (license display) | ~3k |
| Stage 1 carry-state (frozen repository contract + schema) | ~8k |
| GitCellar reference reads (auth + signup patterns if any; or fresh design) | ~10-15k |
| Implementation + tests + email-verify integration | ~30-40k |
| Reasoning reserve | ~25k |
| **Total** | **~75-90k → ~8% utilization. Pass.** |

### Stage 2 — Worker B (Submission Path: API + JWT + Anonymous)

| Item | Tokens |
|---|---|
| Spec sections (FR-FBR-03 + FR-FBR-05 + FR-FBR-06) + DEC-FBR-04 | ~5k |
| Stage 1 carry-state (frozen repository contract) | ~6k |
| JWT fixture corpus (carry-state) | ~3k |
| GitCellar reference reads (`feedback/routes.rs`, rate-limit helpers) | ~12-18k |
| Implementation + tests | ~40-55k |
| Reasoning reserve | ~25k |
| **Total** | **~90-110k → ~10% utilization. Pass.** |

### Stage 3 (single agent, port pattern)

Health endpoint + structured logging — small surface. Estimate ~50-70k total. Pass.

**Per-agent budgets all pass with comfortable margin.** No decomposition needed beyond what Stage 2's two-worker split already provides.

---

## Oracle Pre-Build Plan

| Oracle | Question | Consumer(s) | Timing | Status |
|---|---|---|---|---|
| `multi-tenant-isolation-check` | "Does every domain-touching code path go through tenant-scoped repository methods? Are there any raw SQL strings, any methods accepting `Connection` outside the repository layer, or any methods that take `tenant_id` as a non-`TenantScope` argument?" | Stage 1 agent (builds + consumes); Stage 2 Workers A + B (consume on every commit); CI gate from commit 1 | **Task Zero of Stage 1 — built before any data-write code lands** | not yet built |

**Rationale**: this is the single highest-leverage oracle in the entire arc. DEC-FBR-03 declares raw SQL outside the repository layer a security incident. Without the oracle, the *first* tenant-leakage drift would be caught only at security review (months away) or production (catastrophic). One oracle, invoked on every commit during P0+ by every worker, is the canonical inversion of redundant investigation cost (one author writes the rules; N workers benefit indefinitely).

**Implementation sketch** (Stage 1 agent authors):
- `kind: "verification"`
- `freshness: trigger-invalidate on changes to `migrations/`, `crates/feedbackmonk-repository/`, or `crates/feedbackmonk-core/`
- Two probes:
  1. **AST/grep probe over the API + handler crates**: any `sqlx::query`, `sqlx::query_as`, `sqlx::query!`, `pool.acquire()`, or `&mut Connection` reference *outside* `crates/feedbackmonk-repository/` fails the oracle.
  2. **Repository-method audit**: every public function in `crates/feedbackmonk-repository/` must take `&TenantScope` or `&ProjectScope` as its first non-`&self` argument, or be marked `#[doc(hidden)] pub(crate)` admin-only with an explicit allow-listed call site.
- Output: machine-parseable PASS / FAIL with file:line offenders.

**Deferrals** (evaluated, not scheduled for P0):
- `pii-scrub-audit` (port from GitCellar) — **deferred to P1** per arc plan; P0 has no log-emission surface that ingests submission bodies.
- `widget-bundle-size` — deferred to P2.
- `tier-enforcement-status` — deferred to P3.
- A JWT-verification fixture **corpus** (valid / expired / wrong-aud / wrong-key / alg-none-attack / missing-required-claim) is **scheduled** (see Testability Gate below) but as a **fixture**, not an oracle — it's consumed by tests directly, not by a freshness-bounded probe.

---

## Component Decomposition (sub-task → worker)

```
Stage 1 — Foundation Contract (SEQUENTIAL, 1 agent)
└── Task Zero: build multi-tenant-isolation-check oracle skeleton + freshness contract
└── Sub-task 1: Data model + tenant-scoped repository layer (FR-FBR-01)
    ├── Postgres schema (migrations/00001_p0_schema.sql)
    ├── Domain types (crates/feedbackmonk-core/) — Tenant, Project, SigningKey, Feedback, AnonSubmission, RateLimitCounter
    ├── Repository traits + impls (crates/feedbackmonk-repository/) — TenantScope, ProjectScope, TenantRepo, ProjectRepo, SigningKeyRepo, FeedbackRepo
    └── Oracle wired to CI

Stage 2 — Parallel Surfaces (PODS, 2 workers, fan-out after Stage 1 contract freeze)
├── Worker A — Signup & Onboarding (FR-FBR-02)
│   ├── Signup endpoint: POST /api/v1/signup (email + password OR email-only magic-link)
│   ├── Email-verify endpoint: POST /api/v1/verify-email
│   ├── Create-project endpoint: POST /api/v1/projects (returns project_id + initial embed snippet)
│   ├── Signing-key registration: POST /api/v1/projects/{project_id}/signing-keys (Ed25519 public key registration with rotation slots)
│   └── Admin session: cookie-based, signed
└── Worker B — Submission Path (FR-FBR-03 + FR-FBR-05 + FR-FBR-06)
    ├── JWT EdDSA verifier (crates/feedbackmonk-jwt/)
    │   ├── alg-allowlist enforced (EdDSA only; reject `none`, reject HS256-with-public-key-as-secret confusion)
    │   ├── multiple-active-keys support (try each registered public key for the project; key id from kid header optional)
    │   ├── claims validation: sub, iat, exp (5-min sliding TTL), aud == project_id
    │   └── consumed by submission handler
    ├── Submission endpoint: POST /api/v1/projects/{project_id}/feedback
    │   ├── Auth-mode dispatch: Authorization header present → JWT path; absent → anonymous path
    │   ├── JWT path: verify → extract sub/email/name/external_metadata → repository write
    │   └── Anonymous path: hash(IP + cookie + project_id) → dedup → rate-limit → optional verified-email gate → repository write
    ├── Anonymous mode (crates/feedbackmonk-anon/)
    │   ├── Cookie-based dedup (signed, project-scoped, 30-day TTL)
    │   ├── IP+cookie-hash anti-spam counter (in-memory governor crate for P0; Redis deferred to v1.1)
    │   └── Optional verified-email gate per-project config flag
    └── Submission domain logic in handler (crates/feedbackmonk-api/handlers/submission.rs)

Stage 3 — Observability (SEQUENTIAL, 1 agent, runs in converging session)
└── Sub-task 4: Health + structured logging (FR-FBR-18)
    ├── GET /health → structured JSON {status, db_connected, version, uptime_seconds}
    ├── tracing crate with JSON formatter; correlation IDs per request
    └── Error-rate counter (Prometheus-exposition format optional, deferred to v1.1; counter visibility in logs sufficient for P0)
```

---

## Interface Contracts (MUST be authored in detail in Stage 1; carried as frozen state to Stage 2)

### Contract C1 — Tenant-Scoped Repository API

**Owner**: Stage 1 agent. **Consumers**: Stage 2 Workers A + B; all P1+ code.

```rust
// crates/feedbackmonk-repository/src/scope.rs

/// Carries tenant identity through every repository call. NEVER constructed
/// outside an authenticated session boundary. Construction is `pub(crate)`.
pub struct TenantScope { tenant_id: Uuid, /* private fields */ }

impl TenantScope {
    pub fn tenant_id(&self) -> Uuid { self.tenant_id }
}

/// Project-scoped operations require ProjectScope (which proves the project
/// belongs to a tenant via TenantScope).
pub struct ProjectScope {
    tenant: TenantScope,
    project_id: Uuid,
}

impl ProjectScope {
    pub fn tenant(&self) -> &TenantScope { &self.tenant }
    pub fn project_id(&self) -> Uuid { self.project_id }
}
```

```rust
// crates/feedbackmonk-repository/src/lib.rs

#[async_trait]
pub trait TenantRepo {
    async fn create(&self, email: &str, password_hash: &str) -> Result<Tenant>;
    async fn find_by_email(&self, email: &str) -> Result<Option<Tenant>>;
    async fn mark_verified(&self, scope: &TenantScope) -> Result<()>;
    // ... NO method takes a raw tenant_id Uuid — only TenantScope.
}

#[async_trait]
pub trait ProjectRepo {
    async fn create(&self, scope: &TenantScope, name: &str) -> Result<Project>;
    async fn list_for_tenant(&self, scope: &TenantScope) -> Result<Vec<Project>>;
    async fn open(&self, scope: &TenantScope, project_id: Uuid) -> Result<ProjectScope>;
    // open() is the SOLE constructor of ProjectScope. It enforces
    // tenant→project ownership at the type-system boundary.
}

#[async_trait]
pub trait SigningKeyRepo {
    async fn register(&self, scope: &ProjectScope, public_key: &[u8; 32], label: &str) -> Result<SigningKeyId>;
    async fn list_active(&self, scope: &ProjectScope) -> Result<Vec<SigningKey>>; // for JWT verifier
    async fn deactivate(&self, scope: &ProjectScope, id: SigningKeyId) -> Result<()>;
}

#[async_trait]
pub trait FeedbackRepo {
    async fn submit_authenticated(
        &self,
        scope: &ProjectScope,
        end_user_sub: &str,
        end_user_email: Option<&str>,
        end_user_name: Option<&str>,
        external_metadata: Option<&serde_json::Value>,
        body: &str,
    ) -> Result<FeedbackId>;

    async fn submit_anonymous(
        &self,
        scope: &ProjectScope,
        anon_token_hash: &[u8; 32],
        optional_email: Option<&str>,
        body: &str,
    ) -> Result<FeedbackId>;
}
```

**Invariant (oracle-enforced)**: every method on every repository trait takes `&TenantScope` or `&ProjectScope` as its first non-`&self` argument, OR is `pub(crate)` and called only from another repository method with a scope in hand.

### Contract C2 — JWT Verifier API

**Owner**: Stage 2 Worker B. **Consumers**: Submission handler in Worker B; documented for P2 widget integration.

```rust
// crates/feedbackmonk-jwt/src/lib.rs

pub struct VerifiedClaims {
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub external_metadata: Option<serde_json::Value>, // ≤ 4KB enforced
    pub iat: i64,
    pub exp: i64,
}

pub enum JwtError {
    BadSignature,
    Expired,
    NotYetValid,
    WrongAudience,
    AlgorithmNotAllowed,  // anything other than EdDSA fails here, including `none`
    MissingRequiredClaim(&'static str),
    ExternalMetadataTooLarge,
    MalformedToken,
}

pub fn verify(
    token: &str,
    expected_aud_project_id: Uuid,
    active_keys: &[SigningKey],
    now_unix: i64,  // injectable for testability — Stage 2 Worker B uses a Clock trait
) -> Result<VerifiedClaims, JwtError>;
```

**Hard invariants (must be unit-tested with named tests in the fixture corpus)**:
1. `alg: "none"` tokens fail with `AlgorithmNotAllowed` regardless of key state.
2. `alg: "HS256"` tokens with the Ed25519 public key as HMAC secret fail with `AlgorithmNotAllowed` (the algorithm-confusion attack).
3. Wrong-audience tokens fail with `WrongAudience` even if the signature would have been valid against a key in `active_keys` for a different project.
4. Missing `sub`, `iat`, `exp`, or `aud` fails with `MissingRequiredClaim`.
5. `now_unix > exp + 5*60` fails with `Expired` (5-min sliding TTL — exp encodes the absolute deadline; verifier enforces strict `now ≤ exp`).
6. `external_metadata` > 4096 bytes fails with `ExternalMetadataTooLarge`.

### Contract C3 — Submission Request / Response JSON

**Owner**: Stage 2 Worker B. **Consumers**: P2 widget; external customer integrations.

```http
POST /api/v1/projects/{project_id}/feedback
Content-Type: application/json
Authorization: Bearer <JWT>     (optional — absence triggers anonymous mode)
X-Feedbackmonk-Anon-Cookie: <opaque-base64>  (optional — anonymous mode dedup)

Request body:
{
  "body": "string, required, 1..16384 chars",
  "kind": "bug | feature | question | other",  // optional, default "other"
  "email": "string?, anonymous mode only — auth mode reads email from JWT"
}

200 OK:
{
  "feedback_id": "FB-XXXXXX",
  "accepted_at": "2026-05-13T21:00:00Z",
  "echo": { "body": "...", "kind": "..." }
}

401 (auth mode): JWT verification failed (one of JwtError variants, body says which)
429 (anon mode): rate-limit exceeded for this (project, anon_token_hash)
400: body validation failed
413: body too large
```

### Contract C4 — Signing-Key Registration

**Owner**: Stage 2 Worker A. **Consumers**: customer admin UI (P1); customer backend operators (P0 onboarding flow).

```http
POST /api/v1/projects/{project_id}/signing-keys
Authorization: Bearer <admin-session-cookie or admin-API-token>
Content-Type: application/json

Request:
{
  "public_key_base64": "32 raw Ed25519 public-key bytes, base64",
  "label": "human-readable name, e.g. 'production-2026-Q2'"
}

200 OK:
{
  "signing_key_id": "uuid",
  "registered_at": "...",
  "active": true
}

DELETE /api/v1/projects/{project_id}/signing-keys/{signing_key_id}
  → 200 OK, marks inactive but retains row for audit
```

**Invariant**: a project may have ≥1 active signing key. Verifier (Contract C2) tries each in order and returns the first success.

### Contract C5 — Health Endpoint JSON

**Owner**: Stage 3 agent. **Consumers**: monitoring; P4 self-host docs.

```http
GET /health → 200 OK
Content-Type: application/json

{
  "status": "ok" | "degraded",
  "db_connected": true | false,
  "version": "0.1.0",
  "uptime_seconds": 12345,
  "started_at": "2026-05-13T20:00:00Z"
}

When db_connected = false: status = "degraded", HTTP still 200 (so load-balancer
health checks distinguish "alive but degraded" from "dead").

GET /health/ready → 200 OK if all dependencies healthy; 503 otherwise.
  Liveness vs. readiness split is a 12-factor convention; readiness is used by
  Docker-compose deps and (later) Kubernetes-style orchestration.
```

---

## Testability Gate Findings

Per `claude-template/segments/-ldis/plan-phase4-testability-gate.md`. Five questions scored per P0 FR.

### Flagged

#### FR-FBR-01 — Multi-tenant data model + tenant-scoped repository

| Q | Score | Reasoning |
|---|---|---|
| Q1 | Iteration cost | 2 (sqlx + in-memory or container DB; cheap unit tests) |
| Q2 | Fidelity risk | **5** (cross-tenant leakage is silent — a passing unit test does NOT prove isolation under all future query paths; canonical reward-hacking surface) |
| Q3 | Critical path | **5** (every other FR in P0+ depends) |
| Q4 | Scaffolding leverage | yes — `multi-tenant-isolation-check` Verification Oracle |
| Q5 | Drift detection | schema-hash + repository-method enumeration; oracle re-runs on schema or repository crate change |

**Composite 12 with Q2=5 → flagged (re-affirming arc-plan finding at P0 granularity).**

**Recommendation**: `multi-tenant-isolation-check` is Task Zero. Oracle skeleton + freshness contract must exist BEFORE any data-write code lands. CI is wired to fail-the-build on oracle red. The `TenantScope` / `ProjectScope` newtype design (Contract C1) is the type-system half of the defense; the oracle is the runtime/AST half.

#### FR-FBR-05 — JWT EdDSA verification

| Q | Score | Reasoning |
|---|---|---|
| Q1 | Iteration cost | 3 (Ed25519 keypair generation + JWT round-trip is well-documented but not free; each fixture case requires real crypto in tests) |
| Q2 | Fidelity risk | 3 (JWT verification has well-known pitfalls — alg confusion, missing aud check, key-rotation race; not silent like FR-FBR-01 but easy to get wrong; SWE-Agent literature flags crypto-verifier work as high-Q2) |
| Q3 | Critical path | **4** (the entire P0 exit gate depends on a working JWT-verified submission) |
| Q4 | Scaffolding leverage | **yes — a JWT fixture corpus halves iteration cost and converts each invariant in Contract C2 into a named test** |
| Q5 | Drift detection | fixture corpus has named tests for each invariant; new failure mode → new fixture entry, not a silent regression |

**Composite 10 (borderline); flagged because Q3=4 and Q4=yes match "highest plan-wide leverage" criterion.**

**Recommendation**: build the JWT fixture corpus **before** the main JWT verifier code lands in Stage 2. Corpus is six categories: (a) valid signed-by-key-1, (b) valid signed-by-key-2 (rotation), (c) expired, (d) wrong-aud, (e) alg-none-attack, (f) HS256-confusion-attack, (g) missing-claim-each-of-{sub,iat,exp,aud}, (h) oversize-external_metadata. Each corpus entry is a named test against Contract C2's `verify()` signature. The Stage 1 agent **does NOT** build this corpus (out of Stage 1's scope); Worker B's Stage 2 brief includes it as Task Zero of Worker B.

### Items NOT flagged (composite <10 with no Q2=5 spike)

- **FR-FBR-02** (signup/onboarding): composite 7. Standard CRUD-with-email-verify; Mailpit suffices.
- **FR-FBR-03** (submission API): composite 9 (borderline). Surface-level work; Q2 mitigated by Contract C3 freezing the JSON shape and by the JWT fixture corpus (above) covering the auth half.
- **FR-FBR-06** (anonymous mode): composite 8. Rate-limit + dedup semantics testable with deterministic time + IP fixtures; in-memory governor crate has good unit-test ergonomics.
- **FR-FBR-18** (health + logging): composite 4. Trivial.

---

## Ripple Analysis

**This is a greenfield P0 in a NEW repository.** No existing consumers. Ripple Analysis on existing code is empty by design.

**Forward-looking ripples to be aware of** (consumers that will exist later — Stage 1 contracts must accommodate them):

| Future consumer | What it will need from P0 | Captured in |
|---|---|---|
| P1 admin UI (status workflow, drawer, reply composer) | Repository surface for status transitions + audit history; SigningKeyRepo + ProjectRepo for embed/key management UIs | C1 must allow extension without breaking change (add new repo traits, don't widen existing method signatures) |
| P1 status emails | Stable feedback-event hook surface; tenant brand parameters | Not in P0; P1 carry-state will define |
| P2 widget (P2 Worker A) | C3 submission JSON contract + C4 signing-key registration contract | C3 + C4 frozen at P0 |
| P2 public roadmap, voting | Anonymous voter_id strategy (hashed cookie+IP) | Anonymous-mode token hash strategy lands at P0 — Contract C2/C3 documents the cookie shape so P2 can reuse it |
| P3 tier enforcement | Tenant-row "tier" column + project count ceiling | Add `tenants.tier` column at P0 schema (defaulted to `"free"`); enforcement logic deferred to P3 |
| P4 self-host docker | Env-var configuration surface + `DATABASE_URL` + migration runner | P0 must commit to 12-factor env-var config from commit 1 |

**No source-level ripple into GitCellar.** Confirmed: P0 modifies zero lines in `gitcellar-*` working trees. GitCellar reference reads are read-only.

---

## Deferred Decisions

| Decision | Deferred Until | Default if Unresolved | Why Defer |
|---|---|---|---|
| Redis-backed rate-limiter | v1.1 | In-memory `governor` crate (single-process correct; multi-instance not until horizontal scale required) | YAGNI at P0 single-instance dogfood scale |
| Prometheus metrics exposition | v1.1 | tracing-emitted error-rate counters visible in logs only | P0 health visibility is sufficient via `/health` + structured logs |
| Email provider for P0 signup email-verify | Stage 2 Worker A plan-out | Mailpit in dev, SMTP env-var in prod (provider-agnostic) | Worker A picks a thin SMTP wrapper crate; not arc-level concern |
| Admin session mechanism (cookie vs. bearer) | Stage 2 Worker A | Signed cookie with HMAC secret env var (matches GitCellar conv) | Worker A finalizes |
| Backend dev-server port | Now (this plan) | **`14304`** in 14300-14399 range; claim in MACHINE_CONFIG.md at Stage 1 start | Decided here; Stage 1 agent updates registry |
| Web framework | Now (this plan) | **`axum`** (mirrors GitCellar reference) | Decided |
| Query layer | Now (this plan) | **`sqlx`** with compile-time checking + `sqlx::migrate!` (mirrors GitCellar reference); `cargo sqlx prepare` for offline CI builds | Decided |
| JWT crypto crate | Now (this plan) | **`jsonwebtoken`** v9+ (supports EdDSA) OR custom `ed25519-dalek` + `base64` if `jsonwebtoken` version pins prove painful. Stage 2 Worker B confirms at start. | Decided (with explicit fallback) |
| Password hashing | Now (this plan) | **`argon2`** crate (RFC 9106), default params | Decided |
| Repository-layer enforcement: lint vs. oracle vs. both | Now (this plan) | **Both** — `clippy::pedantic` + a custom `cargo deny` rule + the oracle. Multi-layered. | Decided |

---

## Risks and Mitigations

| Risk | Likelihood | Severity | Mitigation |
|---|---|---|---|
| Multi-tenant isolation silently leaks (drift past the type system) | Low (after oracle) | **Critical** | Three-leg defense: type-system (TenantScope newtype), oracle (AST+repo-method audit), and clippy/deny rules. Wired in Stage 1 commit 1. |
| JWT alg-confusion or aud-not-checked bug ships to P0 exit gate | Low (after fixture corpus) | High | Fixture corpus (a)-(h) named tests in Worker B Task Zero. Verifier signature in C2 enforces explicit `expected_aud_project_id` arg — no implicit-trust path. |
| Email-verify race (user tries to log in before clicking link, or clicks twice) | Medium | Medium | Worker A: `pending_verification` state → state machine → no other state can be reached except via verify token. Idempotent verify endpoint. |
| `sqlx::query!` macro requires `DATABASE_URL` at build time, broken offline CI | High (once) | Low | Use `cargo sqlx prepare` to generate `.sqlx/` query cache; commit it; CI builds offline. Stage 1 agent sets this up before Stage 2 fan-out. |
| Stage 2 worker A and worker B both need to write `axum` routes; route-registration collisions | Low (clean module boundary) | Low | Each worker registers their own `Router` sub-tree; main binary composes them. Convention: Worker A owns `/api/v1/signup`, `/api/v1/projects`, `/api/v1/projects/.../signing-keys`; Worker B owns `/api/v1/projects/{id}/feedback`. No overlap. |
| In-memory rate limiter loses state on restart, abusers exploit | Medium | Low | P0 single-instance dogfood scale — acceptable. Documented in `docs/operations/RATE_LIMITING.md` (Stage 3 task). Hard upgrade path to Redis at v1.1 captured in deferred decisions. |
| 12-factor env-var sprawl confuses self-hosters at P4 | Low | Medium | P0 names env vars from a coherent set (`FEEDBACKMONK_*` prefix) and emits a `.env.example` at end of Stage 3. P4 self-host docs port this verbatim. |
| Stage 1 agent over-elaborates the contract, eating Stage 2 calendar budget | Medium | Medium | Stage 1 has a hard checklist (six trait surfaces in C1; one oracle; one migration). If after ~3 work-days Stage 1 isn't converging, escalate via `/0-uldf-ltads-admin decision` rather than slipping silently. |

---

## Execution Commands

The recommended progression:

1. **Now**: review this plan with the user; accept or revise.
2. **Stage 1 trigger** (single sequential session in this repo):
   - `/0-uldf-ltads-admin init` to create the LTADS workspace if not yet present.
   - `/0-uldf-proceed` — context-budget-aware router picks topology. At collaborative autonomy, expect HERE (continue this session) for Stage 1 scaffolding work; at supervised+ the router may HANDOFF to a fresh session for full-context Stage 1 implementation.
   - Stage 1 exit criteria: contract C1 frozen + oracle green + Stage 2 carry-state docs written.
3. **Stage 2 trigger** (after Stage 1 commit lands):
   - `/0-uldf-pods-parallelize` (this plan as input) → become Lead Developer of 2 Stage 2 workers.
   - `/0-uldf-pods-spawn-collaborator --all` → spawn Worker A (signup) + Worker B (submission path) in separate Claude CLI sessions.
   - Each worker reads their contract section from this plan plus their FRs from `docs/specs/SPECIFICATION.md`.
4. **Stage 2 convergence**:
   - `/0-uldf-pods-converge` once both workers report exit-gate-met.
5. **Stage 3 trigger** (single session in converging Lead Developer's tree):
   - Sub-task 4 (health + logging) runs sequentially in the same session that ran convergence.
   - `/0-uldf-finalize` once P0 exit gate (curl-able end-to-end + oracle green + /health JSON) is reached. P0 then closes; P1 begins via a fresh `/0-uldf-ldis-plan "feedbackmonk P1 — Closes the Loop"` round.

---

## Notes for Downstream Consumers

- **`/0-uldf-pods-parallelize`**: consume this plan; the two Stage 2 worker briefs are encoded in the Component Decomposition + Interface Contracts sections. Worker A's brief is FR-FBR-02 + Contract C4. Worker B's brief is FR-FBR-03 + FR-FBR-05 + FR-FBR-06 + Contracts C2 + C3 + the JWT fixture corpus (Task Zero of Worker B).
- **`/0-uldf-ltads-start`**: P0 work begins here. LTADS state will live in this repo's `ltads/` directory. The arc-plan and this plan together are the cross-session carry-state through P0.
- **Stage 1 agent**: your single deliverable IS Contract C1 (the repository API surface) + the oracle. Resist scope creep into Stage 2 (JWT, signup, submission, anon-mode). The arc plan and this plan are explicit: contract-first.
- **Workers in Stage 2**: read Contract C1 as a frozen library surface. If you discover an inadequacy in C1 mid-Stage-2, **stop and escalate via `channels/messages.md`** — do not silently widen the contract. C1 widening requires Stage 1 agent + Lead Developer involvement.
- **P0 exit gate verification**: a single curl pipeline through tenant signup → project create → key register → JWT-signed POST + anon POST → health endpoint must be runnable from a clean `docker compose up` (Docker compose is P4, but the *script* is wired at P0 to prove the env-var surface). This is the durable witness that P0 closed.
