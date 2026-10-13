# End-user prioritization — voting, "not needed", personal ranking, detail view

Extends the public feedback board (FR-FBR-26/27, Contracts C29/C30) so signed-in end users can
see every feature request, signal support or "not needed", keep their own priority order, and read
an item in full. Decisions: [DEC-FBR-18](DECISIONS.md#dec-fbr-18) … [DEC-FBR-22](DECISIONS.md#dec-fbr-22).

**Terms.** *Signed-in viewer*: a board visitor whose identity is the verified `sub` of a
customer-signed JWT (DEC-FBR-04), held either as a bearer token on the request or as a board
session (FR-FBR-44). *Ranking pool*: defined in FR-FBR-51. *Owner*: an authenticated admin session.

**Assumptions** (minor gaps, not forks): the public roadmap's access and voting are unchanged;
`board_access` governs the board only; on a `public` board anonymous upvoting continues exactly as
today; there is no limit on how many items a user may rank.

#### FR-FBR-42: The board can be filtered by kind [PLANNED]

**Priority**: Should
The board list accepts an optional kind filter and, when given, returns only approved items of
that kind, with `total` counting only those items.

#### FR-FBR-43: A project can require sign-in to see its board [PLANNED]

**Priority**: Must
Each project has a board access setting, `public` (default, today's behaviour) or `signed_in`,
which only the owner can change. On a `signed_in` board, every board read and write by a viewer who
is not signed in is refused with a response that the UI renders as a sign-in prompt, which is
different from the response for a board-disabled project. On a `signed_in` board, no anonymous vote
is accepted.

#### FR-FBR-44: A signed-in identity reaches the hosted board only through a one-time token handoff [PLANNED]

**Priority**: Must
The hosted board receives a customer-signed end-user JWT only in the URL fragment (never the
query string or path), and the board client posts it once to an exchange endpoint and discards it. That endpoint verifies the
token with the existing verifier (audience = project) and returns a board session. The session is
an HttpOnly, Secure, host-only cookie scoped to that one project. It lasts at most 8 hours from the
exchange and cannot be extended without a fresh handoff. The board session carries the JWT's `sub`
and nothing else from the token. After the exchange, the board removes the fragment from the
address bar and history.

#### FR-FBR-45: The widget links a signed-in user to the board as themselves [PLANNED]

**Priority**: Should
When the board is enabled and the widget holds a user token (`data-jwt` or `data-token-url`), the
widget shows a "See all requests" link. The link opens the hosted board and performs the
FR-FBR-44 handoff with a freshly obtained token. Without a user token, the link opens the board
unsigned, and the link is not shown on a `signed_in` board.

#### FR-FBR-46: A signed-in viewer sees their own state, and only their own [PLANNED]

**Priority**: Must
For a signed-in viewer, board list and detail responses carry that viewer's own state for each item:
whether they upvoted it, whether they marked it not needed, and its position in their priority list.
No response carries any other user's state. An anonymous viewer receives no per-viewer state.

#### FR-FBR-47: A signed-in voter can retract an upvote at any time [PLANNED]

**Priority**: Must
A board upvote cast by a signed-in voter can be retracted at any time. Anonymous upvotes keep the
existing retraction window (Contract C30).

#### FR-FBR-48: A signed-in viewer can mark a feature request "not needed" [PLANNED]

**Priority**: Must
A signed-in viewer can mark any item in the ranking pool as not needed, at most once per (item,
user), and can clear the mark at any time. A user's upvote and their not-needed mark on the same
item are mutually exclusive: setting one clears the other in the same transaction. Anonymous
viewers cannot set it.

#### FR-FBR-49: "Not needed" counts are visible to the owner only [PLANNED]

**Priority**: Must
The owner's console shows each item's not-needed count beside its vote count. No public surface,
response or aggregate exposes a not-needed count, and the public board's ordering does not depend on
not-needed counts.

#### FR-FBR-50: A signed-in viewer keeps one ordered priority list per project [PLANNED]

**Priority**: Must
A signed-in viewer can add items from the ranking pool to their priority list, remove items, and
reorder the list at any time. Every change is saved as the user's whole current order. The pool
items not in the list are shown separately as "not yet ranked". Reordering works by keyboard and by
pointer, and the page passes the project's axe accessibility check. A list of fewer than two items
is accepted, and the UI tells the user it does not yet influence priority.

#### FR-FBR-51: The ranking pool is the board's open feature requests [PLANNED]

**Priority**: Must
The ranking pool is the project's items that are approved, of kind `feature`, and whose status is
not `shipped`, `wontfix` or `duplicate`. Promotion to the roadmap moves an item to `duplicate` and
so removes it from the pool. An item that leaves the pool, for any reason, including un-approval or
erasure, no longer contributes to any aggregate. It is shown to the user as removed from their list
and then dropped from it.

#### FR-FBR-52: Aggregation counts only stated preferences [PLANNED]

**Priority**: Must
Each user's list contributes exactly one preference, "a over b", for every pair of pool items in the
list where a is placed above b. An item absent from a user's list contributes no preference from that
user, either for or against. A list of fewer than two items contributes nothing.

#### FR-FBR-53: A user's influence grows linearly with what they ranked [PLANNED]

**Priority**: Must
For a user who has placed k pool items, each of their preferences carries weight 2/k, so their total
weight is k − 1. No user's weight depends on how many items other users ranked.

#### FR-FBR-54: The aggregate order is a regularized Bradley–Terry fit [PLANNED]

**Priority**: Must
The aggregate priority of the ranking pool is the strength order of a Bradley–Terry model fitted to
the weighted preferences. The fit includes a prior that pulls items with few comparisons toward
neutral strength, so one comparison cannot put an item at either extreme. For a given set of lists,
the fit gives the same order every time. An item that no preference touches is reported as
unranked, not given a strength.

#### FR-FBR-55: The owner sees priority beside votes, never merged with them [PLANNED]

**Priority**: Must
The owner's console shows the aggregate priority order. For each item it shows the number of users
who placed it, an uncertainty indicator from the fit, its vote count and its not-needed count. It
lists unranked items separately and shows when the aggregate was computed. No single score combines
votes, not-needed marks and priority.

#### FR-FBR-56: Priority is never public, and never per person [PLANNED]

**Priority**: Must
Neither the aggregate priority nor any individual user's list appears on any public surface. The
owner sees only aggregates. No owner surface or endpoint shows an individual user's list or which
users marked an item not needed.

#### FR-FBR-57: The board detail view shows an item's public record [PLANNED]

**Priority**: Must
Opening a board item shows: its body verbatim in the submitted language, kind, status, vote count,
the date it was accepted, its board note (FR-FBR-58) if any, and its status timeline. The timeline
lists each submitter-visible status transition with its date only, never its note or who made it.

#### FR-FBR-58: The owner can write a public board note on an item [PLANNED]

**Priority**: Should
The owner can set, edit and clear one board note per item. It is plain text written for every board
viewer, and the board detail view shows it. Replies to the submitter, public or internal, never
appear on any board surface.

#### FR-FBR-59: The board never shows submitter or internal data [PLANNED]

**Priority**: Must
No board response, list or detail, signed in or not, contains any submitter identity field (C29
invariant 3's set), any reply, any internal note, any status-change note, any attachment or
attachment reference, the translated body, or the submitter's locale.

#### FR-FBR-60: Erasure reaches every new end-user-keyed record [PLANNED]

**Priority**: Must
Erasing an end user removes their upvotes, not-needed marks, priority list and board sessions.
Erasing a feedback item removes every not-needed mark, priority-list entry and board note for it.
The `feedback-erasure-completeness` oracle covers the new tables.

#### FR-FBR-61: Every new write passes the moderation gate first [PLANNED]

**Priority**: Must
A not-needed mark or a priority-list entry can be written only for an item resolved through the
approved-only board query, before the write. The `public-board-moderation-gate` oracle covers both
write paths.
