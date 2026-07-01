# Intake Assessment
**Source**: /0-uldf-ldis-intake
**Generated**: 2026-07-01T16:07:35
**Task**: FeedbackMonk "Phase A" contract build-out — add the end-user capabilities GitCellar's feedback consolidation depends on (per `.claude/handoff/handoff-20260701-gitcellar-feedback-phaseA.md` + GitCellar plan `20260701-feedback-consolidation-onto-feedbackmonk.md` §3/§4.1/§7).

---

═══════════════════════════════════════════════════════════════
       LEAD DEVELOPER INTELLIGENCE ASSESSMENT
═══════════════════════════════════════════════════════════════

TASK: Add Phase-A contract surfaces to FeedbackMonk so GitCellar (tenant #1) can
delete its internal feedback backend. All additive to the frozen contract; each
new capability advertised via `GET /api/v1/capabilities`. Deploy + verify to
`feedback.gitcellar.com` is the exit gate.

───────────────────────────────────────────────────────────────
PERCEPTION
───────────────────────────────────────────────────────────────

Type: Enhancement → Feature Addition (multiple additive contract surfaces)
Scope: MEDIUM (5 work items; each = migration + repository + handler + capability flag + tests + contract-doc update)
Risk: MEDIUM (P0 erasure item is privacy/GDPR-sensitive and touches object-store bytes; the rest is additive and low-risk. It is a cross-repo GATE — GitCellar Phases B/C block on deploy+verify.)

Professional Assessment:
The starting point is far richer than the GitCellar plan assumed. **Attachments
upload (Gap #2)** and the **end-user read API (`me_feedback`, Gap #4)** are
ALREADY BUILT and frozen. The genuine Phase-A deltas are narrower than "§3's
four gaps": one net-new P0 (delete/erasure), two additive extensions to existing
surfaces (attachment list/download; reply-state fields), two submit-field
additions (severity, idempotency-key), one proposed new read (export), plus
capability flags for each and a deploy/verify gate.

───────────────────────────────────────────────────────────────
CURRENT-STATE MAP (perceived from code, 2026-07-01)
───────────────────────────────────────────────────────────────

| Surface | Built today | Phase-A delta |
|---|---|---|
| `POST …/feedback` (submit) | body/sentiment/kind/email/crash_event_id. NO severity, NO idempotency. | **A4**: add first-class `severity` + `Idempotency-Key` dedupe. |
| `POST …/feedback/{id}/attachments` (upload) | BUILT — ≤4 images ≤5MB, service/console log w/ PII-scrub, magic-byte sniff, size/type limits. | **A2**: add **list** + **download** routes (upload already done) + capability flag. |
| `GET …/me/feedback` + `…/thread` (`me_feedback`) | BUILT — JWT-sub-scoped list + thread; fields: feedback_id, kind, status, body, sentiment, submitted_at. | **A1**: add `DELETE …/me/feedback/{id}`. **A3**: add `updated_at` + `reply_count` (+ optional `?since=`). **A5**: add `GET …/me/feedback/export`. |
| `GET /api/v1/capabilities` | advertises feedback.sentiment / body-optional / sentiment-trend / solicitation.v1 / feedback.my-feedback. | add `feedback.delete`, `feedback.attachments`, `feedback.reply_state`, `feedback.severity`, `feedback.idempotency`, `feedback.export` (final names TBD in plan). |

Load-bearing facts discovered:
- **`feedback(id)` FK graph** — `ON DELETE CASCADE`: feedback_replies (00004),
  feedback_status_history (00003), attachments (00009), feedback_moderation
  (00016), feedback_board_votes (00018). `ON DELETE SET NULL`:
  roadmap_items.origin_feedback_id (00006), status_history.duplicate_of_feedback_id
  (00003). ⇒ a hard row-delete cascades cleanly with **no blocking FK**; roadmap
  items promoted from feedback survive (origin nulled — Q24-consistent, they
  already carry the body verbatim with no FB ref). `feedback.cluster_id` (00013)
  is a column on feedback, not an inbound FK — delete just drops cluster
  membership (a sweep re-clusters; best-effort).
- **Attachment BYTES are NOT purged by DB cascade** — cascade removes only the
  `attachments` metadata rows. A1 erasure MUST explicitly delete each
  `storage_key` object from the object store (`ObjectStore::delete`) — this is
  the crux of "purge body + its attachments." (Check `ObjectStore` exposes a
  delete verb; add if missing.)
- **`feedback` has no `updated_at`** (only `accepted_at`). A3 must add/derive an
  update timestamp (candidate: `greatest(accepted_at, latest public-reply
  created_at, latest status-history changed_at)` computed in the list query, or a
  materialized `updated_at` column bumped on reply/status change).
- **Severity value set (GitCellar tenant #1)**: `low | medium | high | blocker`
  (`FeedbackSeverity`, `gitcellar-cloud/admin-ui/src/utils/feedbackApi.ts`);
  today smuggled in `external_metadata.severity`, required for bug_report only.
- **Migration numbering**: next free is `00020` (last is `00019_feedback_translation`).
  New migrations must be coordinated/sequential (A1..A5 may each add one).

───────────────────────────────────────────────────────────────
SPECIFICATION ANALYSIS
───────────────────────────────────────────────────────────────

Coverage: 8/10 dimensions specified (Purpose, Scope, Behavior, Data, Integration,
Constraints, Success all clear; Users = GitCellar Desktop end-users, clear).
Gaps: 0 blocking-after-perception, 3 confirmable sub-decisions, 1 low.

The GitCellar plan §6 already pre-decided the open questions with proposed
defaults ("confirm at execution"). After code perception, the genuinely
load-bearing (contract-freezing) confirmations are:

Confirmable sub-decisions (each has a strong recommended default):
• **Erasure semantics** (A1): hard-delete row + cascade + explicit object-store
  purge  vs.  tombstone (redact body + purge attachments, keep row). Affects the
  list API (row vanishes vs. shows "deleted"), analytics/clusters, and tests.
• **Severity enum** (A4): adopt `low|medium|high|blocker` (matches tenant #1) as
  the tenant-generic first-class set  vs.  a different generic set. Contract-freezing.
• **Export inclusion** (A5 / OQ-4): include `GET …/me/feedback/export` now  vs.
  defer. Small; completes the GDPR erasure+portability story.

Low (assume): reply-detection mechanism (OQ-5) = `updated_at`+`reply_count`
polling (plan's proposed default; webhooks a later enhancement). Assumed, not asked.

───────────────────────────────────────────────────────────────
CALIBRATION
───────────────────────────────────────────────────────────────

Required Spec Level: Thorough (production, public-facing, privacy-sensitive, frozen contract)
Current Spec Level: Thorough-minus (plan + handoff + this code-perception cover it; 3 confirmations outstanding)

VERDICT: SUFFICIENT (route to /0-uldf-ldis-plan) — pending confirmation of the 3
sub-decisions above, all of which have recommended defaults and none of which
block planning.

───────────────────────────────────────────────────────────────
ENGAGEMENT STRATEGY
───────────────────────────────────────────────────────────────

Questions: 3 asked (erasure semantics, severity enum, export include/defer),
1 assumed (reply-detection = updated_at+reply_count), remainder deferred to plan.

Recommended defaults (lead-with-recommendation):
1. Erasure: **hard-delete row + cascade + explicit object-store byte purge.**
   Strongest right-to-erasure, simplest, matches "delete → gone from both screens."
2. Severity: **`low|medium|high|blocker`** — matches tenant #1, generic enough
   for any tenant; optional field (not required, unlike GitCellar's bug_report rule).
3. Export: **include now** — small, on-brand, completes erasure+portability.

DECISION STATUS (2026-07-01): the 3 questions were put to the user via
AskUserQuestion; user was AFK (no response in 60s). Per intake guidance I
**adopted all three recommended defaults** to keep momentum and re-confirm on
their return. These are ASSUMPTIONS pending user confirmation — they are
contract-freezing, so the plan flags them as a confirmation gate before any
migration/handler is written:
- **D-A1** Erasure = hard-delete row + full FK cascade + explicit object-store
  byte purge (via `ObjectStore::delete` over each attachment `storage_key`).
- **D-A4** `severity` = optional field, value set `low|medium|high|blocker`,
  accepted on submit in BOTH auth + anon modes, stored as a first-class column.
- **D-A5** `GET …/me/feedback/export` = IN scope for Phase A + `feedback.export`
  capability flag.
Reply "Assumption D-A1/D-A4/D-A5 is wrong: …" to override.

───────────────────────────────────────────────────────────────
ORACLE CANDIDATES (Proactive Oraculurgy)
───────────────────────────────────────────────────────────────

Candidates:
• **feedback-erasure-completeness** (Verification Oracle): after a
  `DELETE …/me/feedback/{id}`, assert NO residual PII — the body is gone AND no
  object-store object remains under the feedback's attachment prefix AND all
  cascade children are gone. Signal: A1 is privacy-critical, the "purge bytes not
  just rows" invariant is exactly the kind of thing that silently regresses when
  someone later refactors the delete path. Qualification: deterministic ✓ |
  recurrent ✓ (runs every finalize + before deploy gate) | freshness-contractable
  ✓ (keyed to delete-handler + storage source) | gracefully-absent ✓. Timing:
  Task Zero of A1 (mirrors the project's established Verification-Oracle-per-trust-
  boundary pattern — approval-gate, moderation-gate, translation-egress).
• **capability-advertisement-parity** (lighter): assert every capability the code
  actually implements has a matching string in `CAPABILITIES` (and vice-versa) —
  the deploy/verify gate (A6) depends on this being truthful. Signal: 6 new flags
  land across 5 streams; drift between "route exists" and "flag advertised" breaks
  GitCellar's feature-detection. Qualification: deterministic ✓ | recurrent ✓ |
  freshness-contractable ✓ | gracefully-absent ✓. Timing: deferred → build during
  A6 (or fold into an existing capabilities test).

───────────────────────────────────────────────────────────────
COLLABORATION ASSESSMENT
───────────────────────────────────────────────────────────────

Scope: MEDIUM
Subdivisible: YES

Natural decomposition (3 cohesive streams by touched surface):
- **Stream 1 — me_feedback surface** (A1 delete + A3 reply-state + A5 export): all
  touch `me_feedback.rs` + the repository's end-user reads. Cohesive; keep together.
- **Stream 2 — submit surface** (A4 severity + idempotency): touches `feedback.rs`
  + submit repository + a new migration.
- **Stream 3 — attachments** (A2 list + download): touches `attachments.rs` +
  `AttachmentRepo` + `ObjectStore`.
- LD-owned shared seams: `capabilities.rs` (all flags), the contract doc
  `docs/integrations/gitcellar-adoption.md`, migration numbering (00020+), and A6
  deploy/verify.

Collaboration Value:
- Specialization Benefit: 3/5 — surfaces are distinct but small.
- Quality Benefit: 3/5 — the P0 erasure item benefits from focused attention + a critic.
- Discovery Potential: 2/5 — well-specified, little to discover.
- Speed Benefit: 3/5 — streams are independent post-contract.

Friction:
- Coupling: 3/5 — `capabilities.rs` + contract doc + migration numbering are shared;
  Stream 1 internally couples 3 items on one file (fine — one owner).
- Boundary Clarity: 4/5 — clean by touched file.

Net Score: ~6 → STAGED / small PODS (rung 1 flat PODS at most; HDT not needed).
Recommendation: this is comfortably a single-session HERE job OR a 3-worker flat
PODS. Topology decision belongs to `/0-uldf-proceed` at the plan boundary (context
budget + work shape). Intake flags: subdivisible, moderate parallel value, rung ≤1.

───────────────────────────────────────────────────────────────
RECOMMENDED NEXT STEPS
───────────────────────────────────────────────────────────────

1. Confirm the 3 sub-decisions (erasure semantics, severity enum, export inclusion).
2. `/0-uldf-ldis-plan "feedbackmonk Phase A — GitCellar contract build-out"` —
   design intra-phase execution (stream ordering, migration numbering, the two
   Verification Oracles as Task Zero, exit gate A6 deploy+verify) and let
   `/0-uldf-proceed` pick HERE vs. flat-PODS by context budget.
3. Execute A1 (P0) first (privacy) with its erasure-completeness oracle; then
   A2/A3/A4/A5 (independent); capability flags + contract-doc updates folded in
   per stream; A6 deploy+verify closes the gate.

═══════════════════════════════════════════════════════════════
