//! Shared runner types — the FROZEN data seam (Contract C26/C27, P5b).
//!
//! Stage-1 workers (implementer/A, runner-loop/B, analyst/C) build against these
//! verbatim. The trust boundary is encoded in the type layout itself:
//! [`ClaimedOrder`] separates the **trusted** layer (owner-approved
//! `instructions` + `owner_overrides`) from the **untrusted** layer
//! ([`RecommendationContext`] — ALL feedback-derived text), so prompt assembly
//! (Worker A) cannot accidentally cross the streams.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use feedbackmonk_core::ActionType;

/// A work order the runner has claimed, with everything needed to drive the
/// implementer — split into the trusted instruction inputs and the untrusted
/// feedback-derived context (C27 25b).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimedOrder {
    pub work_order_id: Uuid,
    pub project_id: Uuid,
    pub action_type: ActionType,
    /// Owner-authored title (trusted — survived the approval gate).
    pub title: String,
    /// **Trusted** instruction layer: owner-approved, ratified at the approval
    /// gate (FR-FBR-25a). Safe to place in the instruction/system layer.
    pub instructions: String,
    /// **Trusted** Q17 owner edits merged over the recommendation (overrides win).
    pub owner_overrides: Option<serde_json::Value>,
    /// **Untrusted** feedback-derived context. EVERYTHING in here is
    /// public-internet input and MUST enter the prompt only via
    /// `prompt::wrap_untrusted` (C27 25b single chokepoint).
    pub recommendation: RecommendationContext,
}

/// The untrusted, feedback-derived grounding for a work order. Every field is
/// public-reported context — treat as DATA, never as instructions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationContext {
    /// The analyst recommendation body (derived from feedback).
    pub body: String,
    /// The analyst's rationale (derived from feedback).
    pub rationale: Option<String>,
    /// The cluster summary/label (derived from feedback bodies).
    pub cluster_summary: String,
    /// Verbatim member feedback bodies (the rawest untrusted text).
    pub member_bodies: Vec<String>,
    /// Grounding references (file/line pointers) — references, NEVER dumps
    /// (the C27 25c references-not-dumps invariant; validated by the API
    /// ingestion gate `validate_source_refs` + the runner egress sanitizer).
    pub source_refs: serde_json::Value,
}

/// The assembled implementer prompt with its two layers kept SEPARATE so the
/// C24 corpus can assert that feedback-derived text lands only in the untrusted
/// envelope and the DEC-84 preamble is present in the trusted layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledPrompt {
    /// Trusted instruction layer: DEC-84 critical-action preamble + owner-approved
    /// `instructions` + `owner_overrides`. NO feedback-derived text.
    pub instructions: String,
    /// The SINGLE delimited untrusted-data envelope holding ALL feedback-derived
    /// text (built only by `prompt::wrap_untrusted`).
    pub untrusted_envelope: String,
}

impl AssembledPrompt {
    /// Render the two layers into the final prompt text handed to the agent.
    /// Trusted layer first, then the clearly-delimited untrusted envelope.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}\n\n{}", self.instructions, self.untrusted_envelope)
    }
}

/// Where the agent runs (the customer's checked-out repo).
#[derive(Debug, Clone)]
pub struct RepoContext {
    /// Absolute path to the repository working tree.
    pub repo_path: String,
}

/// The outcome of one implementation run — CONCLUSIONS ONLY (C27 25c). Carries
/// references + verification summary; the wire form is [`ResultRef`] after the
/// egress sanitizer. NO source, NO diffs-as-content, NO secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplementResult {
    pub result_ref: ResultRef,
}

/// The `result_ref jsonb` shape the runner POSTs on `reported` (C26). References
/// + conclusions only — the egress sanitizer (C27) enforces references-not-dumps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultRef {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    pub diff_stat: DiffStat,
    pub verification: Verification,
    /// A short, sanitized summary — conclusions, never a content dump.
    pub summary: String,
}

/// Diff magnitude (counts only — never the diff content; 25c).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffStat {
    pub files: u32,
    pub insertions: u32,
    pub deletions: u32,
}

/// Verification outcome conclusions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verification {
    pub tests_passed: bool,
    /// e.g. "passed" | "failed" | "skipped" — a status string, not a log dump.
    pub finalize_status: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_ref_serializes_references_only() {
        let r = ResultRef {
            pr_url: Some("https://git.example/pr/1".into()),
            branch: Some("fbm/wo-123".into()),
            commit: None,
            diff_stat: DiffStat { files: 2, insertions: 10, deletions: 3 },
            verification: Verification { tests_passed: true, finalize_status: "passed".into() },
            summary: "Fixed the null-check; 2 files".into(),
        };
        let v = serde_json::to_value(&r).unwrap();
        // Conclusions/references present; no content/source/secret fields.
        assert_eq!(v["branch"], "fbm/wo-123");
        assert_eq!(v["diff_stat"]["files"], 2);
        assert_eq!(v["verification"]["tests_passed"], true);
        assert!(v.get("commit").is_none(), "absent optionals are omitted");
    }

    #[test]
    fn assembled_prompt_renders_trusted_then_untrusted() {
        // NB: the real `<untrusted-feedback-data>` envelope literal is owned
        // solely by `prompt::wrap_untrusted` (the single chokepoint the
        // `feedback-as-data-audit` oracle enforces); this test uses a neutral
        // marker so the literal never appears outside that one place.
        let p = AssembledPrompt {
            instructions: "TRUSTED".into(),
            untrusted_envelope: "[ENVELOPE]".into(),
        };
        let rendered = p.render();
        assert!(rendered.starts_with("TRUSTED"));
        assert!(rendered.contains("[ENVELOPE]"));
    }
}
