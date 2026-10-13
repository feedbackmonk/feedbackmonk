# end-user-prioritization — plan

**Spec**: `docs/specs/end-user-prioritization.md`, which covers FR-FBR-42 … FR-FBR-61 (decisions DEC-FBR-18 … DEC-FBR-22).
**Topology**: one contracts piece first, then parallel workers. `contracts` freezes everything that
more than one piece would otherwise write: migrations, repository, wire types, the client, route
registration and module stubs. After that, the remaining pieces' path sets are disjoint. Five of
them can run together, and two wait on what they consume.
**Nodes**: — (every piece fits one window).

## Pieces

| Piece | Writes (globs) | Depends on | Proof — the command or check that says it is done | Status |
|---|---|---|---|---|
| `contracts` | `migrations/00033_*.sql`, `migrations/00034_*.sql`, `migrations/00035_*.sql`, `migrations/00036_*.sql`, `migrations/README.md`, `crates/feedbackmonk-core/src/{lib.rs,board_signals.rs,priority.rs}`, `crates/feedbackmonk-repository/src/**`, `crates/feedbackmonk-repository/README.md`, `crates/feedbackmonk-api/src/handlers/{mod.rs,board_session.rs,board_admin.rs}`, `.sqlx/**`, `admin-ui/src/shared/{types.gen.ts,ApiClient.ts,boardModerationApi.ts}`, `admin-ui/src/App.tsx`, `admin-ui/src/pages/board/{BoardItemDetail.tsx,MyPriorities.tsx,SignInHandoff.tsx}`, `admin-ui/src/pages/priority/OwnerPriority.tsx`, `docs/planning/handoffs/prioritization-contracts.md` | — | `unit`: `cargo test -p feedbackmonk-repository board_signals board_sessions board_notes erasure` green, each new test red first against an empty body. It must cover: kind filter (FR-FBR-42); approved-only resolution before every new write (FR-FBR-61); upvote and not-needed exclusive in one transaction (FR-FBR-48); list saved whole (FR-FBR-50); pool membership and drop-out (FR-FBR-51); erasure reaching every new table (FR-FBR-60). Also `bash scripts/ci-local.sh` green (build, clippy, sqlx offline) with the stubs in place | — |
| `priority-engine` | `crates/feedbackmonk-core/src/priority.rs`, `crates/feedbackmonk-core/tests/priority*.rs` | `contracts` | `unit`: `cargo test -p feedbackmonk-core priority` green, red first. It must show: an unranked item gets no strength (FR-FBR-52); a one-item list moves nothing (FR-FBR-52); total per-user weight = k − 1 and invariant to other users' list lengths (FR-FBR-53); one comparison does not reach an extreme; a fixed input gives an identical order on repeated runs (FR-FBR-54); a k=20 ranker does not outweigh twenty k=2 rankers on a shared pair | — |
| `public-api` | `crates/feedbackmonk-api/src/handlers/{board.rs,voting_common.rs,board_session.rs,README.md}`, `crates/feedbackmonk-api/tests/board_{access,session,signals,priority_list,detail,retract}.rs` | `contracts` | `unit`: `cargo test -p feedbackmonk-api --test board_access --test board_session --test board_signals --test board_priority_list --test board_detail --test board_retract` green, red first. It must show: `signed_in` refuses anonymous reads and votes with the sign-in response, distinct from 404 (FR-FBR-43); the token is accepted only by the POST exchange, the cookie flags are as specified, and an expired session is refused (FR-FBR-44); per-viewer state is the caller's only (FR-FBR-46); signed-in retract after 60 s succeeds and anonymous still gets 403 (FR-FBR-47); a not-needed count appears in no public response (FR-FBR-49); the detail response carries none of FR-FBR-59's fields; plus the existing `board_*` tests | — |
| `admin-api` | `crates/feedbackmonk-api/src/handlers/{board_admin.rs,moderation.rs}`, `crates/feedbackmonk-api/src/main.rs`, `crates/feedbackmonk-api/tests/board_admin*.rs` | `contracts`, `priority-engine`, `public-api` | `unit`: `cargo test -p feedbackmonk-api --test board_admin --test board_admin_privacy` green, red first. It must show: board-access setting owner-only (FR-FBR-43); board-note set, edit and clear (FR-FBR-58); the priority view returns order, ranker count, uncertainty, unranked set and computed-at, and no merged score (FR-FBR-55); no admin response names any user's list or who marked not needed (FR-FBR-56) | — |
| `public-ui` | `admin-ui/src/pages/board/**`, `i18n/locales/en/public.json`, `admin-ui/e2e/public-board*.spec.ts` | `contracts` | `unit`: `cd admin-ui && npx vitest run src/pages/board` green, and `npx playwright test e2e/public-board-a11y.spec.ts e2e/public-board-priorities.spec.ts` green with 0 axe violations. The specs must reorder a priority list by keyboard alone (FR-FBR-50), show the fragment cleared after the handoff (FR-FBR-44), render the sign-in prompt on a `signed_in` board (FR-FBR-43), and show the one-item note (FR-FBR-50) | — |
| `admin-ui` | `admin-ui/src/pages/priority/**`, `admin-ui/src/pages/settings/BoardSettings.tsx`, `admin-ui/src/pages/settings/__tests__/BoardSettings*.test.tsx`, `i18n/locales/en/admin.json`, `admin-ui/e2e/owner-priority-a11y.spec.ts` | `contracts` | `unit`: `cd admin-ui && npx vitest run src/pages/priority src/pages/settings` green, and `npx playwright test e2e/owner-priority-a11y.spec.ts` green with 0 axe violations. It must show votes, not-needed and priority as separate columns with no combined score (FR-FBR-55), and the access setting and the board-note editor round-trip through the mocked client | — |
| `widget-link` | `widget/src/**`, `widget/dist/**`, `i18n/locales/en/widget.json`, `widget/README.md` | `contracts` | `unit`: `cd widget && npx vitest run` green, red first. It must show: the link is present only with a token and an enabled board; it is absent on `signed_in` without a token; the token travels in the fragment, never the query (FR-FBR-45). Plus `python .claude/project-oracles/widget-bundle-size/oracle.py` green (size reported, no tracker) | — |
| `oracles` | `.claude/project-oracles/feedback-erasure-completeness/**`, `.claude/project-oracles/public-board-moderation-gate/**`, `.claude/project-oracles/host-tenant-binding/**` | `public-api`, `admin-api` | `oracle`: each of the three runs green on the tree (`python .claude/project-oracles/<name>/oracle.py`), and red against a recorded mutation. The mutations are: drop the new tables from erasure (FR-FBR-60); remove approved-only resolution from the not-needed write and from the priority-list write (FR-FBR-61); add an unclassified board-session route. Each mutation recorded in that oracle's `README.md` | — |

## Order and partition

`contracts` runs alone. It writes every file that two later pieces would both need:
- the migrations and the whole repository crate, including the `feedback.rs` erasure path and the
  kind-filtered board query;
- the `.sqlx` cache, because handlers never carry raw SQL (DEC-FBR-03);
- `types.gen.ts`, `ApiClient.ts` and `boardModerationApi.ts`, with every new client function;
- `App.tsx`, with the new routes pointing at stub pages;
- `handlers/mod.rs` and the two new handler stubs;
- the core stubs: `board_signals.rs`, and `priority.rs` with its signature frozen and an empty body.

The wire contract it writes (`prioritization-contracts.md`, Contracts C43 public and C44 admin) is
what every later piece builds against.

Then `priority-engine`, `public-api`, `public-ui`, `admin-ui` and `widget-link` run in parallel.
Their write sets are disjoint:
- each English catalog namespace is its own file;
- the public routes stay inside `board_router` in `board.rs`, so only `admin-api` touches `main.rs`;
- `BoardItemDetail.tsx`, `MyPriorities.tsx`, `SignInHandoff.tsx` and `OwnerPriority.tsx` pass from
  `contracts` to their UI piece by the ordering above.

`admin-api` follows `priority-engine`, because its proof calls the real fit. It also follows `public-api`, which hands it the handlers `README.md`. `oracles` runs last,
because it needs the real handlers to grade.

The convergence alone writes:
- the requirement statuses in `docs/specs/end-user-prioritization.md`;
- the `SPECIFICATION.md` index line;
- the generated non-English locale tables (`i18n-catalog-integrity` regenerates them; translation
  itself waits for `/1-translate`, per DEC-FBR-17).

## Budget

`contracts` is the heaviest: four migrations, about four repository modules and an edit to the
3.4k-line `feedback.rs`. It fits one window if it reads `feedback.rs` by its erasure and board
sections rather than whole. Every other piece is small to medium. The engine is one pure module
of a few hundred lines.

## Risks that would change this plan

- **The hosted board is not same-origin with the API on some host shape.** The session cookie
  would then need credentialed CORS on `board_router`, and `public-api` would split into exchange
  and signals. Observed when `public-api`'s session test cannot set the cookie from the board
  origin.
- **The `host-tenant-binding` oracle refuses the new routers before `oracles` runs.** In that
  case its classification edit moves into `contracts`. Observed when `contracts`' `ci-local.sh`
  run goes red on that oracle.
- **A fit over a large pool is slow enough to need a cache**, like the roadmap's 60-second one.
  That adds an aggregator task to `admin-api`. Observed when `admin-api`'s priority test over
  several hundred items exceeds an interactive response.
