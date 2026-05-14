//! P4 Stage 2 marketing-site pricing SSOT export (DEC-FBR-IMPL-05).
//!
//! Emits the canonical four-tier quota matrix from `feedbackmonk_core::tier::tier_quotas()`
//! as pretty JSON on stdout. The Astro `/pricing` page consumes this via the
//! `marketing/scripts/export-tier-quotas.{sh,ps1}` build-step shim, which writes
//! the JSON into `marketing/src/data/tier_quotas.json` (gitignored).
//!
//! Build-step (not source-of-truth) by design — Contract C19 (`tier_quotas()`)
//! is the only place tier shapes are authored. This binary is a one-way mirror.
//!
//! Invocation: `cargo run --quiet -p feedbackmonk-core --example export_tier_quotas`

use feedbackmonk_core::tier::{tier_quotas, Tier, TierQuotas};
use serde::Serialize;

/// On-wire export shape: one object per tier, `tier` discriminator field
/// followed by the verbatim `TierQuotas` fields (via serde flatten).
///
/// Self-describing — round-trips back to (Tier, TierQuotas) without external
/// schema. Field shape MATCHES `TierQuotas` exactly per task spec; the only
/// added field is the `tier` discriminator, taken verbatim from
/// `Tier::as_db_str` (DEC-FBR-03 wire form: free|starter|pro|self_host).
#[derive(Serialize)]
struct TierExport {
    tier: &'static str,
    #[serde(flatten)]
    quotas: TierQuotas,
}

fn main() {
    let entries: Vec<TierExport> = [Tier::Free, Tier::Starter, Tier::Pro, Tier::SelfHost]
        .into_iter()
        .map(|t| TierExport {
            tier: t.as_db_str(),
            quotas: tier_quotas(t),
        })
        .collect();

    let json = serde_json::to_string_pretty(&entries)
        .expect("TierQuotas + &'static str fields always serialize");
    println!("{json}");
}
