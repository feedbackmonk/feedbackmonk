---
id: DEFER-006
title: Install the staged host-tenant-binding oracle + 2 allow-list entries — the CI-parity gate is red until then, so nothing from FR-FBR-32/33 can be pushed
status: PROPOSED
origin: defer-local
source-project: feedbackmonk
source-session-id: session-20260830-113228-909
injected-at: 2026-08-30T16:15:00Z
autonomy-hint: autopilot
suggested-entry-point: implement
scope-estimate: single-session
content-hash: fbm-host-tenant-binding-oracle-install-v1
---

# DEFER-006: Install the staged `host-tenant-binding` oracle + 2 allow-list entries — the CI-parity gate is red until then, so nothing from FR-FBR-32/33 can be pushed

> **Measured 2026-08-30 by the commands quoted below — re-measure before acting.** This is not a
> hypothesis: the red gate was reproduced, and the fix was authored and self-tested in the same
> session. What is unknown is only whether the receiving session has the grant to apply it.

## Idea

`bash scripts/ci-local.sh` is **RED**, on exactly one of fifteen oracles, and CI runs the same suite —
so the FR-FBR-32/33 work sitting in commit `90ca288` **cannot be pushed** until this is applied.

Everything needed is finished and staged. One command from a session that may write under `.claude/`:

```bash
bash scripts/oracles-pending/host-tenant-binding/install.sh
```

It copies the `host-tenant-binding` Verification Oracle into `.claude/oracles/`, appends the two
`multi-tenant-isolation-check` allow-list entries FR-FBR-32 requires, and re-verifies both. Then
delete `scripts/oracles-pending/host-tenant-binding/` — it is a staging area, not a second home for
the oracle — and re-run `bash scripts/ci-local.sh` before pushing.

## Originating Context

FR-FBR-32/33 (the DEC-FBR-13/14 hosting shape, from `DEFER-005`) added `DomainRepo` — the host→tenant
resolution layer. `multi-tenant-isolation-check` correctly flags two of its methods, and the fix is
two allow-list entries. Both the entries and the new oracle live under `.claude/`, and **DEC-84
(CSI-08) hard-defers every `.claude/` write for a session whose role is `orchestrated-worker`, at any
autonomy level.** The building session hit that block, respected it, and staged the artefacts rather
than working around it. Grant requests are recorded in `.claude/session-state/grant-requests.jsonl`.

**Reproduced measurement:**

```
$ bash .claude/oracles/multi-tenant-isolation-check/oracle.sh
FAIL multi-tenant-isolation-check (3 offender(s))

Probe B offenders (public repository fn missing &TenantScope/&ProjectScope):
  crates/feedbackmonk-repository/src/domains.rs:110  DomainRepo::resolve_host  ...
  crates/feedbackmonk-repository/src/domains.rs:176  DomainRepo::resolve_host  ...
  crates/feedbackmonk-repository/src/domains.rs:169  SqlxDomainRepo::new       ...

$ bash scripts/ci-local.sh
... 14 oracles PASS ...
❌ verification-oracle suite FAILED: multi-tenant-isolation-check
❌ CI-parity checks FAILED — fix before pushing (this is what CI would report).
```

Three offenders, two allow-list entries: `resolve_host` appears twice (trait declaration + impl) and
a single `[[methods]]` entry covers both, exactly as `ProjectRepo::open_for_submission` does today.

### Do not "fix" this by loosening the oracle

The two remaining offenders are irreducible, and that was established by trying: the first draft of
the repository surface produced **seven**, and five were removed by **reshaping the code, not the
allow-list** —

- `DomainKind` / `DomainStatus` moved into `feedbackmonk-core::hosting`, beside `Tier` and
  `ModerationStatus`, so their parse helpers are no longer repository-crate free functions;
- `DomainRepo::mark_active` was made `&TenantScope`-first (the TLS-authorisation handler already
  resolves the owning tenant, so it can mint the scope) — which also stopped a stray `domain_id`
  from being able to flip a row belonging to someone else.

What is left is one genuine pre-authentication boundary (`resolve_host` takes an attacker-controlled
`Host` header and *mints* a binding — structurally identical to `open_for_submission`) and one pool
constructor. Deleting the entries, weakening Probe B, or skipping the oracle in `ci-local.sh` would
each turn a real invariant off to make a message go away.

## Success Criterion

- `bash .claude/oracles/multi-tenant-isolation-check/oracle.sh` → **PASS**.
- `bash .claude/oracles/host-tenant-binding/oracle.sh --full` → **PASS** (Probes A/B/C/D). The `--full`
  leg needs `DATABASE_URL` + Postgres.
- `bash scripts/ci-local.sh` → green, and commit `90ca288` pushes cleanly (`git -c
  credential.helper=store push`, per this repo's CLAUDE.md).
- `scripts/oracles-pending/host-tenant-binding/` is **deleted** — leaving a second copy of an oracle
  in the tree is how the two drift apart.
- The `host-tenant-binding` row in `CLAUDE.md`'s oracle table loses its ⚠️ staging note, and
  `PF-HOSTING-ORACLE-01` is removed from Pending Follow-Ups.

## Dependencies

- **A session that may write under `.claude/`.** That is the whole blocker. Either an owner/LD
  session, or a worker after `bash ~/.claude/scripts/grant-deferral.sh --from-request latest`.
- Nothing else. No code change, no migration, no decision.

## Related Artifacts

- `scripts/oracles-pending/host-tenant-binding/` — `install.sh`, `oracle.py` (379 lines, 4 probes),
  `manifest.json`, `README.md`, `allowlist-additions.toml`, and the two shims.
- `CLAUDE.md` — `PF-HOSTING-ORACLE-01`, and the oracle table row carrying the ⚠️ staging note.
- `.claude/session-state/grant-requests.jsonl` — the recorded DEC-84 deferrals.
- `docs/specs/DECISIONS.md` — DEC-FBR-IMPL-28, the binding invariant the oracle defends.
- `crates/feedbackmonk-api/tests/host_tenant_binding.rs` — Probe D's behavioural leg.

## Note on why an oracle, not just tests

The oracle is not ceremony duplicating the test file. `multi-tenant-isolation-check` polices the
*repository scope* axis; host binding is a second, orthogonal axis, and **a public router merged
without the host guard passes that oracle unchanged**. The failure is invisible at runtime — a
missing guard renders a correct-looking page of another tenant's feedback on your origin, with
nothing logged and no test failing. Probe A is the anti-treadmill leg (modelled on
`public-route-ceiling`): a *new* public route added later that forgets the guard fails there.

It was adversarially self-tested during authoring: dropping `bind_public_routes` from
`widget_config_router` and `bind_admin_routes` from `moderation_router` both went red and named the
offender; flipping the cross-tenant refusal from 404 to 403 went red on Probe B. All restored and
re-verified PASS.
