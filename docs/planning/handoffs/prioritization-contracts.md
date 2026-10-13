# End-user prioritization — frozen contracts C43 (public) and C44 (owner)

**Plan**: `docs/planning/plans/20261013T145126-end-user-prioritization.md`
**Spec**: `docs/specs/end-user-prioritization.md` (FR-FBR-42..61, DEC-FBR-18..22)
**Frozen by**: the `contracts` piece (wave 1). Every later piece builds against this file. A change
to a shape here is a plan revision, not a lane decision.

What `contracts` already landed, so no later piece writes it:

| Layer | What exists |
|---|---|
| Migrations | `00033` `projects.board_access` · `00034` `board_sessions` · `00035` `feedback_not_needed` + `board_priority_entries` · `00036` `feedback_board_notes` |
| Core | `board_signals.rs`: `BoardAccess`, `in_ranking_pool`, `RANKING_POOL_EXCLUDED_STATUSES`, `BOARD_SESSION_MAX_AGE_SECS`, `ViewerItemState`. `priority.rs`: `fit_priority`, `preference_weight`, `PriorityFit*`, `RankedItem`, `UnrankedItem` — **signature frozen, stub body** (priority-engine fills it) |
| Repository | `BoardSignalRepo`, `BoardSessionRepo`, `BoardNoteRepo` (full implementations + tests); `ProjectRepo::set_board_access` and `BoardSettings::board_access`; JWT upvote clears the voter's not-needed mark; `erase_all_for_end_user` reaches the three new user-keyed tables |
| API | `handlers/board_session.rs` and `handlers/board_admin.rs`: wire types, helpers and 501 handler stubs, registered in `handlers/mod.rs` but routed nowhere yet |
| Admin UI | the types (`types.gen.ts`, `boardModerationApi.ts`), every client function (`ApiClient.ts`, `boardModerationApi.ts`), the routes in `App.tsx`, and four stub pages |

## Where the new repositories live (read this before touching `AppState`)

`AppState` (`state.rs`) and the `multi-tenant-isolation-check` allowlist were in no piece's write
set, so the three new repositories hang off the board's existing end-user store rather than new
state fields:

```rust
state.board_votes.signals()   // &dyn BoardSignalRepo  — not-needed, priority list, viewer state, kind filter, owner inputs
state.board_votes.sessions()  // &dyn BoardSessionRepo — board sessions
state.board_votes.notes()     // &dyn BoardNoteRepo    — board detail record + owner board note
```

`SqlxBoardVoteRepo::new(pool)` builds all three over the same pool, so every existing
`AppState` constructor (main, tests) gets them unchanged. Giving them their own `AppState` fields
later is a mechanical refactor that needs `state.rs`, `main.rs`, the test harnesses and three
allowlist entries in one piece.

## The rules every handler enforces, in this order

For every route under `/api/v1/projects/{project_id}/board…`:

1. `open_for_submission(project_id)` → scope (unknown project → 404).
2. `ensure_board_enabled` → **404** when the board is disabled (C29 inv. 2, unchanged).
3. `resolve_board_viewer(state, scope, headers)` (`board_session.rs`): a valid bearer JWT
   (audience = project) wins, else a live board-session cookie for this project, else anonymous. A
   bearer token that fails verification is the existing JWT error response, never a silent downgrade.
4. If `BoardSettings::board_access == SignedIn` and the viewer is anonymous → **401**
   `{"error":"SignInRequired"}` (`sign_in_required_response()`), on every read and write **except
   the three `/board/session` routes**, which are how an anonymous viewer becomes signed in (critic
   C-001, collab-20261013-145347). The UI renders a sign-in prompt for this body and "board not
   available" for the 404.
5. Writes that need a person (not-needed, priority list) refuse an anonymous viewer with the same
   **401 SignInRequired** on a `public` board too (DEC-FBR-20).
6. The moderation gate runs before every write (FR-FBR-61): votes resolve through
   `resolve_approved_board_feedback_id` as today; not-needed and priority-list writes resolve inside
   the repository write (`resolve_pool_board_items`, approved-only + ranking pool, same transaction).

A signed-in viewer's `voter_id` is their `sub` with `voter_mode = jwt`, whether the identity came
from a bearer token or a board session — so their vote, mark and list share one key, and erasure
of the `sub` reaches all of them.

## Contract C43 — public board (in `board::board_router`; public, CORS, rate floor)

### `GET /board?kind=&limit=&offset=` (FR-FBR-42/43/46)

`kind` ∈ `bug|feature|question|other`, optional; anything else → 400. Repository:
`signals().list_board_for_viewer(scope, BoardFilter { kind }, viewer.sub(), limit, offset)`.

```json
{
  "items": [
    {
      "short_code": "FB-XXXXXX", "body": "…", "kind": "feature", "status": "triaged",
      "vote_count": 12, "accepted_at": "2026-10-13T…Z",
      "viewer": { "upvoted": true, "not_needed": false, "priority_position": 0 }
    }
  ],
  "total": 37, "limit": 20, "offset": 0,
  "access": "public",
  "signed_in": true
}
```

`viewer` is **absent** (not null) for an anonymous viewer (FR-FBR-46). `total` counts only the
filtered kind. Order is unchanged (newest first); it never depends on not-needed counts
(FR-FBR-49). No item carries a not-needed count, a priority, a submitter field, a reply, a
translated body or a locale (FR-FBR-49/56/59).

### `GET /board/items/{short_code}` (FR-FBR-57/58/59)

Repository: `notes().get_board_item_detail` + (signed in) `signals().viewer_state_for_board_item`.

```json
{
  "short_code": "FB-XXXXXX", "body": "verbatim, submitted language", "kind": "feature",
  "status": "in-progress", "vote_count": 12, "accepted_at": "…",
  "board_note": { "body": "We're starting this in November.", "updated_at": "…" },
  "timeline": [ { "status": "triaged", "at": "…" }, { "status": "in-progress", "at": "…" } ],
  "in_ranking_pool": true,
  "viewer": { "upvoted": false, "not_needed": false, "priority_position": null }
}
```

`board_note` is `null` when there is none. `timeline` is oldest first, status and date only.
`in_ranking_pool` is `feedbackmonk_core::in_ranking_pool(Approved, kind, status)`; the UI offers
"not needed" and "add to my priorities" only when it is true.

### `POST|DELETE /board/items/{short_code}/vote` (FR-FBR-43/47/48)

Shapes unchanged (C30). Changes: on a `signed_in` board an anonymous vote → 401 SignInRequired. A
signed-in voter's retract passes `Duration::MAX` as the window (retract any time); an anonymous
retract keeps `RETRACTION_WINDOW` and its 403. A signed-in cast clears that voter's not-needed
mark in the same transaction (the repository does it; the response is unchanged).

### `PUT /board/items/{short_code}/not-needed` · `DELETE …/not-needed` (FR-FBR-48/61)

Signed-in only (401 SignInRequired otherwise). Repository: `signals().set_not_needed` /
`clear_not_needed`. An item outside the ranking pool → 404.

```json
{ "short_code": "FB-XXXXXX", "not_needed": true,  "upvote_cleared": true }
{ "short_code": "FB-XXXXXX", "not_needed": false }
```

### `GET /board/priorities` · `PUT /board/priorities` (FR-FBR-50/51)

Signed-in only. `PUT` body `{ "items": ["FB-AAAAAA", "FB-BBBBBB"] }` — the whole order, top
first; `[]` clears. Repository: `signals().save_priority_list` / `load_priority_list`. An item not
in the pool → 404 `{"error":"NotInRankingPool"}`; a repeated item → 409 `{"error":"DuplicateItem"}`;
either way the stored list is unchanged. Both return:

```json
{
  "entries":  [ { "short_code": "…", "body": "…", "kind": "feature", "status": "triaged", "vote_count": 3, "accepted_at": "…" } ],
  "unranked": [ … pool items not in the list, same item shape … ],
  "removed":  [ … items that left the pool but are still public, same item shape … ],
  "removed_hidden": 1,
  "influences_priority": false
}
```

`influences_priority` is `entries.length >= 2` (FR-FBR-50: a shorter list is accepted, and the UI
says it does not yet influence priority). `removed` / `removed_hidden` are reported once by the
`GET`, which drops them (FR-FBR-51).

### `POST|GET|DELETE /board/session` (FR-FBR-44)

`POST` body `{ "token": "<end-user JWT from the URL fragment>" }`. Verified with
`feedbackmonk_jwt::verify_with_leeway`, audience = project; failure → the existing JWT error
response (401). On success: random 32-byte token, `hash_board_session_token` stored via
`sessions().create_board_session(scope, hash, sub, BOARD_SESSION_MAX_AGE_SECS)`, and
`Set-Cookie: fbm_board_<project uuid simple>=<token>; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=28800`
— no `Domain` (host-only). The session holds the `sub` and nothing else; it is never extended (a
new handoff mints a new one). All three return `{ "signed_in": bool, "expires_at": "…" | null }`.
`DELETE` ends the session and expires the cookie. The token is accepted **only** in this `POST`
body — never in a query string, path or header of a board page request.

**Single use is not a server guarantee** (critic C-002). The schema keeps no consumed-token record,
and none is wanted: a customer-signed JWT is already a bearer credential for its own lifetime (the
board accepts it as `Authorization: Bearer` on every route), so refusing a second exchange would
deny a token holder nothing it lacks. What is frozen is that the token travels only in the fragment
and this `POST` body, and that the board client exchanges it once and discards it.

The board client (`SignInHandoff`) reads `#token=` from the fragment, `POST`s it, then
`history.replaceState`s the URL without the fragment before navigating on (FR-FBR-44).

## Contract C44 — owner (AdminSession, no CORS; routes from `moderation::routes`)

### Board settings — existing `GET|PATCH /api/v1/admin/projects/{project_id}/board-settings`

Adds `board_access` (FR-FBR-43):

```json
{ "public_board_enabled": true, "board_requires_moderation": true, "board_access": "public" }
```

`PATCH` accepts `"board_access": "public" | "signed_in"` (anything else → 400) and applies it with
`ProjectRepo::set_board_access`; the other legs keep using `set_board_settings`.

### `GET /api/v1/admin/projects/{project_id}/priority` (FR-FBR-55/56)

Built from `signals().board_priority_inputs(scope)` and `fit_priority(pool, lists, &Default)`.
Shapes in `handlers/board_admin.rs`:

```json
{
  "computed_at": "…",
  "contributing_rankers": 14,
  "ranked": [
    { "short_code": "…", "body_excerpt": "…", "status": "triaged", "rank": 1,
      "rankers": 9, "uncertainty": 0.41, "vote_count": 40, "not_needed_count": 3 }
  ],
  "unranked": [
    { "short_code": "…", "body_excerpt": "…", "status": "submitted",
      "rankers": 0, "vote_count": 2, "not_needed_count": 0 }
  ]
}
```

No field combines votes, not-needed and priority (FR-FBR-55). Nothing names a user or a list
(FR-FBR-56). `contributing_rankers` counts lists of two or more pool items.

### `GET|PUT|DELETE /api/v1/admin/feedback/{feedback_id}/board-note` (FR-FBR-58)

Sole-project scope as the other `/admin/feedback/…` routes. `PUT` body `{ "body": "…" }` (trimmed,
1..=4000 chars, else 400). Repository: `notes().get_board_note` / `set_board_note` /
`clear_board_note`. A board note exists only on a board item: an item that is not approved → 404.
Response `{ "short_code": "…", "note": { "body": "…", "updated_at": "…" } | null }`; `DELETE`
returns the same with `note: null`.

## Pages (admin-ui `App.tsx`, frozen routes)

| Route | Page | Owner piece |
|---|---|---|
| `/public/projects/:projectId/board/items/:shortCode` | `pages/board/BoardItemDetail.tsx` | public-ui |
| `/public/projects/:projectId/board/priorities` | `pages/board/MyPriorities.tsx` | public-ui |
| `/public/projects/:projectId/board/sign-in` (`#token=…`) | `pages/board/SignInHandoff.tsx` | public-ui |
| `/admin/priority` | `pages/priority/OwnerPriority.tsx` | admin-ui |

The widget's "See all requests" link (FR-FBR-45) opens
`{board origin}/public/projects/{id}/board/sign-in#token={fresh JWT}` when it holds a token, and
`/public/projects/{id}/board` otherwise. Host-rooted variants (`/board/…` on a tenant host) are not
routed yet: `TenantHostLanding` (pages/public) is in no piece's write set.

## For the `oracles` piece

- `public-board-moderation-gate` check (0b) allowlists only `.feedback.<method>` calls in `board.rs`.
  The new board reads arrive through `.board_votes.signals()` / `.notes()`; extend (0b) to those
  handles, and add the two write paths (FR-FBR-61). Every repository fn named `*board*` that reads
  `feedback` already carries the approved literal in each SQL statement, so check (1) is green.
- `feedback-erasure-completeness` Probe E already sees the three `user_sub` tables erased in
  `erase_all_for_end_user`; `feedback_board_notes` hangs off `feedback` by cascade.
