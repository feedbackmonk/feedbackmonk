# tier-enforcement-status

## Summary

Verification Oracle defending FR-FBR-14 revenue enforcement: no code path may create a
chargeable row — a project, or a feedback submission — without first consulting
`check_tier_quota` with the matching `ResourceKind` and acting on its verdict. Come here
when it is red, or after adding any path that creates a project or submits feedback.

## Probes

### Probe A — every chargeable-row creation is tier-guarded (static, default)

1. **Discover** the chargeable repository functions: every non-test fn in
   `crates/feedbackmonk-repository/src/` whose body runs `INSERT INTO projects (` or
   `INSERT INTO feedback (`, closed over same-trait methods calling them via `self.`.
   Today: `ProjectRepo::create`, `FeedbackRepo::submit_{authenticated,anonymous}[_full]`.
   An INSERT outside a trait impl, or no writer found for either table, is a FAIL —
   the probe refuses to go blind.
2. **Find** every call site in every other crate's `src/` (`#[cfg(test)]` items
   excluded, comments stripped, string contents blanked). A method name no other
   repository file defines matches by name (`.m(` / `::m(`); a shared name (`create`)
   matches only through a receiver bound to the owning trait — a `ProjectRepo`- or
   `SqlxProjectRepo`-typed field/param (`state.projects`, `project_repo`), a
   `Sqlx*Repo::new` local, a `let x = &state.projects;` alias, or UFCS
   `ProjectRepo::create(`. Zero call sites for either table is a FAIL.
3. **Guard**: earlier in the enclosing fn there must be
   `let v = … check_tier_quota(…, ResourceKind::<K>) …;` with `K` matching the table
   (`projects` → `Project`, `feedback` → `FeedbackInRollingMonth`), **and** `v.allowed`
   must be tested between that statement and the call. A fn that calls without the
   guard becomes chargeable itself and its callers must carry the guard — this is how
   `handlers/feedback.rs::submit` guards the private `submit_*_path` helpers. An
   unguarded fn with no caller (a route handler referenced only as `post(create)`) is a
   FAIL, reported with the whole chain.
4. No `INSERT INTO projects|feedback (` outside the repository crate.

`allowlist.toml` can exempt a `file::function` only with a written rationale; it is
empty, and an entry without a rationale is itself a FAIL.

### Probe C — integration smoke (`--full`)

`cargo test -p feedbackmonk-api --test tier_enforcement_smoke -- --include-ignored`
(dev Postgres on 5433) drives the cap-firing HTTP paths end-to-end. The test target
exists, so a missing target now FAILs instead of passing vacuously.

### Dropped (2026-09-30)

- **Old Probe A** (write-pattern scan over `pub async fn` handlers) was vacuous: its
  body finder started paren-counting *after* the fn's opening `(`, so depth went to −1
  at the closing `)` and the "body" became whatever brace next balanced — e.g. the
  `TierCapExceeded { … }` literal inside `projects::create`. It never saw a real
  handler body.
- **Old Probe B** (`tier_quotas()` C19 token check) duplicated the core unit tests
  `c19_{free,starter,pro,self_host}_tier_shape` and `only_free_tier_carries_footer` in
  `crates/feedbackmonk-core/src/tier.rs`, which assert every value, the free-tier
  footer included.

## Invocation

```bash
python .claude/project-oracles/tier-enforcement-status/oracle.py            # Probe A
python .claude/project-oracles/tier-enforcement-status/oracle.py --full     # + Probe C
python .claude/project-oracles/tier-enforcement-status/oracle.py --root <tree>  # scan another tree
python .claude/project-oracles/tier-enforcement-status/oracle.py -v         # discovery detail on FAIL
```

Exit `0` PASS, `1` FAIL, `2` environment error (e.g. `--root` without the crates).

## Adversarial self-test

Harness: `scratchpad/mut2/tier-enforcement-status/run_mutations.py` (session scratch,
not tracked) copies `crates/*/src` into a scratch tree, applies one mutation, runs
`oracle.py --root <tree>`, then rebuilds. Run 2026-09-30: baseline exit 0; every
mutation exit 1, naming the right fn; restored tree exit 0.

| # | Mutation | Result |
|---|---|---|
| M1 | remove the guard from `projects::create` | exit 1 — `projects.rs create … nothing calls it` |
| M2 | move the check after `state.projects.create(…)` | exit 1 |
| M3 | check only in a `//` comment | exit 1 |
| M4 | new handler calling `state.feedback.submit_anonymous(…)` unguarded | exit 1 — `me_feedback.rs sneaky_submit` |
| M5 | check kept, `if !status.allowed` replaced by `if false` (verdict ignored) | exit 1 |
| M6 | project path checks `ResourceKind::FeedbackInRollingMonth` | exit 1 |
| M7 | `submit`'s check removed (guard must flow through `submit_*_path`) | exit 1 — chain `submit_*_full <- submit_*_path <- submit` |
| M8 | UFCS `ProjectRepo::create(&*state.projects, …)` in a new fn | exit 1 |
| M9 | `let repo = &state.projects; repo.create(…)` in a new fn | exit 1 |
| M10 | `"INSERT INTO feedback (…)"` in the api crate | exit 1 |
| M11 | `check_tier_quota(` only inside a string literal | exit 1 |
| M12 | repository `INSERT INTO feedback` in a free fn outside any trait | exit 1 |
| M13 | allowlist entry for `projects.rs::create` without a rationale | exit 1 |

Known limits (static, name-based): a receiver bound in a way none of the forms in
step 2 cover (e.g. passed through a generic `R: ProjectRepo` param named in a
`where` clause) would be missed for the shared name `create`; the unique-name
feedback methods have no such gap. Probe C is the backstop.

## Lineage

FR-FBR-14 (caps + footer), Contract C17/C18/C19, DEC-FBR-03 (tier matrix). Three-leg
defense: type system (`Tier`, `TierQuotas`, `check_tier_quota`) → this oracle →
`tier_enforcement_smoke.rs`.
