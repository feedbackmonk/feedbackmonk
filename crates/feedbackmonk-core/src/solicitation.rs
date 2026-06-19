//! Durable per-user **feedback-solicitation state** (Capability 2 of the
//! GitCellar in-app solicitation request; FR-FBR-29).
//!
//! A consumer (e.g. GitCellar Desktop) shows an ambient "got a minute for
//! feedback?" nudge to engaged users. feedbackmonk owns the DURABLE record of
//! whether a given end-user may be asked — so the consumer can honor
//! "ask at most ~twice/year, honor dismissal, honor opt-out" WITHOUT that state
//! resetting when the user reinstalls the client. The record is keyed by the
//! end-user's stable JWT `sub` (per project), so it survives client reinstalls
//! (the whole point — local client state does not).
//!
//! ## State machine
//!
//! ```text
//!   eligible ──prompted──▶ prompted ──┬─ dismissed
//!                                      ├─ gave_feedback
//!                                      └─ opted_out (terminal)
//! ```
//!
//! - `eligible` — the implicit default for any sub with no record yet. May be
//!   prompted.
//! - `prompted` — the consumer displayed the nudge; awaiting the user's choice.
//! - `dismissed` / `gave_feedback` — the user resolved the nudge. The frequency
//!   cap (a cooldown computed from `prompted_at`, see `eligibility` at the API
//!   layer) governs when the sub becomes promptable again.
//! - `opted_out` — TERMINAL. The user asked never to be solicited again. No
//!   further transitions are legal except an idempotent re-`opted_out`. This is
//!   the load-bearing privacy promise — once opted out, never eligible again.
//!
//! `opted_out` is reachable from ANY non-terminal state (a user can opt out
//! before ever being prompted). `dismissed` / `gave_feedback` are reachable
//! ONLY from `prompted` (you cannot dismiss a nudge that was never shown).
//!
//! This module freezes the enum + the legal transition table + the transition
//! function. The cooldown/frequency-cap POLICY (how long after a prompt the sub
//! becomes eligible again) is an API-layer concern (configurable), not a
//! domain-machine concern — the machine only knows the discrete states.
//!
//! DB form (and JSON form) is `snake_case`, matching the `00017` CHECK
//! constraint byte-for-byte: `eligible | prompted | dismissed | gave_feedback |
//! opted_out`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Persisted solicitation state for one (project, end-user) pair. DB/JSON form
/// is `snake_case`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SolicitationStatus {
    /// The default for any sub with no record. Promptable.
    #[default]
    Eligible,
    /// A nudge was shown; awaiting the user's resolution.
    Prompted,
    /// The user dismissed the nudge without giving feedback.
    Dismissed,
    /// The user gave feedback in response to the nudge.
    GaveFeedback,
    /// TERMINAL: the user opted out of all future solicitation.
    OptedOut,
}

impl SolicitationStatus {
    /// DB-side string form. Must match the CHECK constraint in migration
    /// `00017` byte-for-byte.
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Eligible => "eligible",
            Self::Prompted => "prompted",
            Self::Dismissed => "dismissed",
            Self::GaveFeedback => "gave_feedback",
            Self::OptedOut => "opted_out",
        }
    }

    /// Lenient DB-string parser. Unknown values fall back to `Eligible` — the
    /// safe default (the CHECK constraint guarantees only canonical values are
    /// ever stored). Mirrors the other domain enums' `from_db_str`.
    #[must_use]
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "prompted" => Self::Prompted,
            "dismissed" => Self::Dismissed,
            "gave_feedback" => Self::GaveFeedback,
            "opted_out" => Self::OptedOut,
            _ => Self::Eligible,
        }
    }

    /// `true` iff this is the terminal opted-out state.
    #[must_use]
    pub fn is_opted_out(self) -> bool {
        matches!(self, Self::OptedOut)
    }
}

/// An event the consumer reports against a sub's solicitation record. DB/JSON
/// form is `snake_case`. Distinct from [`SolicitationStatus`] because the
/// `prompted` event drives bookkeeping (bump `prompt_count`, stamp
/// `prompted_at`) beyond the resulting status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SolicitationEvent {
    /// The consumer displayed the nudge to this user.
    Prompted,
    /// The user dismissed the nudge.
    Dismissed,
    /// The user gave feedback via the nudge.
    GaveFeedback,
    /// The user opted out of all future solicitation.
    OptedOut,
}

impl SolicitationEvent {
    #[must_use]
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Prompted => "prompted",
            Self::Dismissed => "dismissed",
            Self::GaveFeedback => "gave_feedback",
            Self::OptedOut => "opted_out",
        }
    }
}

/// Errors from [`apply_event`]. The API layer maps these to HTTP `409`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SolicitationError {
    /// The event is not legal from the current state (e.g. `dismissed` when no
    /// nudge is outstanding).
    #[error("illegal solicitation event {event:?} from state {from:?}")]
    IllegalTransition {
        from: SolicitationStatus,
        event: SolicitationEvent,
    },
    /// The sub has opted out; no event other than `opted_out` is honored.
    #[error("end-user has opted out of solicitation")]
    OptedOut,
}

/// Apply a [`SolicitationEvent`] to the current [`SolicitationStatus`],
/// returning the resulting status or a [`SolicitationError`].
///
/// Legality (see the module-level diagram):
/// - `opted_out` is honored from ANY state (idempotent if already opted out)
///   and is terminal.
/// - any OTHER event against an already `opted_out` record is rejected with
///   [`SolicitationError::OptedOut`] (the privacy promise).
/// - `prompted` is legal from any non-terminal state.
/// - `dismissed` / `gave_feedback` are legal ONLY from `prompted`.
///
/// # Errors
/// Returns [`SolicitationError`] when the event is not legal from `current`.
pub fn apply_event(
    current: SolicitationStatus,
    event: SolicitationEvent,
) -> Result<SolicitationStatus, SolicitationError> {
    use SolicitationEvent as E;
    use SolicitationStatus as S;

    match event {
        // Opt-out is always honored, idempotent, and terminal.
        E::OptedOut => Ok(S::OptedOut),
        // Opted-out is terminal for every other event.
        _ if current.is_opted_out() => Err(SolicitationError::OptedOut),
        // A prompt may be shown from any non-terminal state.
        E::Prompted => Ok(S::Prompted),
        // You can only resolve a nudge that is currently outstanding.
        E::Dismissed if current == S::Prompted => Ok(S::Dismissed),
        E::GaveFeedback if current == S::Prompted => Ok(S::GaveFeedback),
        E::Dismissed | E::GaveFeedback => Err(SolicitationError::IllegalTransition {
            from: current,
            event,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SolicitationEvent as E;
    use SolicitationStatus as S;

    #[test]
    fn db_strings_round_trip() {
        for s in [S::Eligible, S::Prompted, S::Dismissed, S::GaveFeedback, S::OptedOut] {
            assert_eq!(S::from_db_str(s.as_db_str()), s);
        }
    }

    #[test]
    fn default_is_eligible() {
        assert_eq!(S::default(), S::Eligible);
    }

    #[test]
    fn unknown_db_str_falls_back_to_eligible() {
        assert_eq!(S::from_db_str("nope"), S::Eligible);
    }

    #[test]
    fn prompt_from_eligible() {
        assert_eq!(apply_event(S::Eligible, E::Prompted), Ok(S::Prompted));
    }

    #[test]
    fn prompt_from_resolved_states_is_a_new_cycle() {
        assert_eq!(apply_event(S::Dismissed, E::Prompted), Ok(S::Prompted));
        assert_eq!(apply_event(S::GaveFeedback, E::Prompted), Ok(S::Prompted));
    }

    #[test]
    fn dismiss_and_gave_feedback_require_a_prompt() {
        assert_eq!(apply_event(S::Prompted, E::Dismissed), Ok(S::Dismissed));
        assert_eq!(apply_event(S::Prompted, E::GaveFeedback), Ok(S::GaveFeedback));
        // From eligible (never prompted) these are illegal.
        assert!(matches!(
            apply_event(S::Eligible, E::Dismissed),
            Err(SolicitationError::IllegalTransition { .. })
        ));
        assert!(matches!(
            apply_event(S::Eligible, E::GaveFeedback),
            Err(SolicitationError::IllegalTransition { .. })
        ));
    }

    #[test]
    fn opt_out_from_any_state() {
        for s in [S::Eligible, S::Prompted, S::Dismissed, S::GaveFeedback, S::OptedOut] {
            assert_eq!(apply_event(s, E::OptedOut), Ok(S::OptedOut));
        }
    }

    #[test]
    fn opted_out_is_terminal_for_other_events() {
        for ev in [E::Prompted, E::Dismissed, E::GaveFeedback] {
            assert_eq!(apply_event(S::OptedOut, ev), Err(SolicitationError::OptedOut));
        }
    }

    #[test]
    fn event_json_is_snake_case() {
        let parsed: SolicitationEvent = serde_json::from_str(r#""gave_feedback""#).unwrap();
        assert_eq!(parsed, E::GaveFeedback);
        assert_eq!(serde_json::to_string(&S::OptedOut).unwrap(), r#""opted_out""#);
    }
}
