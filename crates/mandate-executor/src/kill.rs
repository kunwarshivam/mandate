//! Kill switches (trading-domain spec §5.5): the final mode first, then the cancels, each step
//! journaled.

use mandate_canon::Value;

use crate::batch::Batch;
use crate::codec::mode_name;
use crate::error::ExecutorError;
use crate::orders::transition;
use crate::payload::text;
use crate::types::{BrokerRequest, Initiator, KillScope, OrderState};

/// An agent-scoped kill switch. The final mode is journaled before anything else, so an intent
/// handled after it is gated under that mode; then every working order **of that agent** is
/// cancelled by its client order id — never through the broker's cancel-all, which would reach
/// other agents' orders and the owner's (`AGENTS.md` rule 13, interpretation 18). The sells of the
/// agent's sub-ledger are the protective sequence's (E7-4) and are recorded as deferred here.
pub(crate) fn switch(
    batch: &mut Batch<'_, '_>,
    scope: &KillScope,
    initiator: Initiator,
) -> Result<(), ExecutorError> {
    let KillScope::Agent(agent) = scope else {
        return Err(ExecutorError::NotInterpreted {
            what: "an account or workspace kill switch".to_owned(),
            story: "E7-4",
        });
    };
    let applied = batch.journal(
        "AgentModeApplied",
        None,
        vec![
            ("agent", text(agent.0.clone())),
            ("to", text(mode_name(initiator.final_mode()))),
            ("restriction", text("kill_switch")),
            ("originated", Value::Bool(true)),
        ],
    )?;
    let initiated = match initiator {
        Initiator::Owner => "owner",
        Initiator::RiskLimit => "risk_limit",
        Initiator::PlatformOperator => "platform_operator",
    };
    batch.journal(
        "KillSwitchActivated",
        Some(applied),
        vec![
            ("scope", text("agent")),
            ("agent", text(agent.0.clone())),
            ("initiator", text(initiated)),
            ("deferred", Value::Bool(true)),
        ],
    )?;
    let working: Vec<_> = batch
        .view
        .orders
        .values()
        .filter(|order| {
            &order.agent == agent
                && matches!(
                    order.state,
                    OrderState::Accepted | OrderState::PartiallyFilled | OrderState::PendingReplace
                )
        })
        .map(|order| order.client_order_id.clone())
        .collect();
    for id in working {
        transition(batch, &id, OrderState::PendingCancel, Vec::new())?;
        batch.broker(BrokerRequest::Cancel {
            client_order_id: id,
        });
    }
    Ok(())
}
