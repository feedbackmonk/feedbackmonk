//! Claim helper (C26 loop step 2) — `dispatched → claimed`, then fetch the
//! order detail (step 3) so the caller holds the full trusted + untrusted
//! context needed to drive the implementer.
//!
//! Thin by design: the transport (the claim POST + the detail GET) lives in
//! [`WorkOrderClient::claim`]; this wrapper exists so the loop reads as the C26
//! protocol (`poll → claim → implement → report`) and so claim-time logging has
//! one home.

use crate::client::WorkOrderClient;
use crate::types::ClaimedOrder;
use uuid::Uuid;

/// Claim a dispatched order and return its full [`ClaimedOrder`] detail.
///
/// A runner token can author `claim` but can NEVER author `approve` (C22 inv.
/// 2) — so this can only ever advance an order the owner already approved; it
/// can never create work.
///
/// # Errors
/// Transport/HTTP failure on the claim transition or the detail fetch (e.g. the
/// order was already claimed by another tick, or no longer `dispatched`).
pub async fn claim_order(
    client: &WorkOrderClient,
    work_order_id: Uuid,
) -> anyhow::Result<ClaimedOrder> {
    let claimed = client.claim(work_order_id).await?;
    tracing::info!(
        target: "runner",
        %work_order_id,
        action_type = ?claimed.action_type,
        "claimed dispatched work order"
    );
    Ok(claimed)
}
