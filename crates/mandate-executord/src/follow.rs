//! What the executor reads from streams it does not write (DEC-2003 items 3 to 5): which agents
//! trade on its account, from the control stream, and the kill switches each agent's stream holds.
//!
//! Every reader here is a pure function of journaled rows. A row the executor must act on and
//! cannot read is never skipped: it is [`Unreadable`], and the process stops rather than guess
//! (DEC-85, `AGENTS.md` rule 3).

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Value, parse};
use mandate_executor::{
    AccountScope, AgentId, EventId, FoldedEvent, Initiator, OwnerConfirmation, Seq,
};
use mandate_journal::{Environment, StoredEvent};
use mandate_num::{Price, Qty};

/// A row the executor must act on that it could not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreadable;

/// One journaled row as the executor's fold and readers take it (journal spec §3).
///
/// # Errors
/// [`Unreadable`] for a body that is not canonical JSON or has no payload.
pub fn folded(row: &StoredEvent) -> Result<FoldedEvent, Unreadable> {
    let body = parse(&row.body).map_err(|_| Unreadable)?;
    let payload = body.get("payload").cloned().ok_or(Unreadable)?;
    let causation_id = body
        .get("causation_id")
        .and_then(Value::as_str)
        .map(|id| EventId(id.to_owned()));
    Ok(FoldedEvent {
        stream: row.stream_id.clone(),
        seq: Seq(row.seq),
        event_id: EventId(row.event_id.clone()),
        event_type: row.event_type.clone(),
        causation_id,
        payload,
    })
}

/// The agents whose deployments trade on the account in `scope`, read from the control stream in
/// `environment`: an agent whose latest version-2 `AgentDeployed` names a connection whose latest
/// establishment or revocation is a version-2 `ConnectionEstablished` naming this `account_ref`
/// (journal spec §9.8, §9.24, DEC-1171). A version-1 deployment names no connection, so it reaches
/// no executor (DEC-1171 item 3). An agent whose deployment stopped is still followed, since it may
/// still hold orders or a position its kill switch must reach (`AGENTS.md` rule 13).
///
/// # Errors
/// [`Unreadable`] for a deployment or connection record whose members do not read: an agent the
/// account may owe a kill switch is never dropped for it (DEC-85).
pub fn agents_of(
    control: &[StoredEvent],
    scope: &AccountScope,
    environment: Environment,
) -> Result<BTreeSet<AgentId>, Unreadable> {
    let mut ours: BTreeMap<String, bool> = BTreeMap::new();
    let mut deployed: BTreeMap<String, Option<String>> = BTreeMap::new();
    for row in control
        .iter()
        .filter(|row| row.environment == environment.as_str())
    {
        if !matches!(
            row.event_type.as_str(),
            "ConnectionEstablished" | "ConnectionRevoked" | "AgentDeployed" | "AgentStopped"
        ) {
            continue;
        }
        let event = folded(row)?;
        let text = |member: &str| {
            event
                .payload
                .get(member)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(Unreadable)
        };
        match (row.event_type.as_str(), row.schema_version) {
            ("ConnectionEstablished", 2) => {
                let mine = text("account_ref")? == scope.account.0
                    && text("environment")? == environment.as_str();
                ours.insert(text("connection_id")?, mine);
            }
            ("ConnectionEstablished" | "ConnectionRevoked", _) => {
                ours.insert(text("connection_id")?, false);
            }
            ("AgentDeployed", 2) => {
                deployed.insert(text("agent_id")?, Some(text("connection_id")?));
            }
            ("AgentDeployed", _) => {
                deployed.insert(text("agent_id")?, None);
            }
            _ => {
                text("agent_id")?;
            }
        }
    }
    Ok(deployed
        .into_iter()
        .filter(|(_, connection)| {
            connection
                .as_ref()
                .is_some_and(|c| ours.get(c).copied().unwrap_or(false))
        })
        .map(|(agent, _)| AgentId(agent))
        .collect())
}

/// A `KillSwitchActivated` on an agent's stream: the runtime's copy of a kill switch that covers
/// the agent, which the executor applies as its own agent-scoped switch over its own ledger
/// (trading spec §5.5; DEC-2003 item 4). Whatever scope the copy records, the executor's switch
/// touches only this agent, so it never reaches an account-wide endpoint (`AGENTS.md` rule 13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switch {
    pub switch: EventId,
    pub initiator: Initiator,
    pub confirmation: Option<OwnerConfirmation>,
}

/// The kill switches one agent's stream holds, in stream order.
///
/// # Errors
/// [`Unreadable`] for a `KillSwitchActivated` whose members do not read.
pub fn switches(rows: &[FoldedEvent]) -> Result<Vec<Switch>, Unreadable> {
    rows.iter()
        .filter(|row| row.event_type == "KillSwitchActivated")
        .map(|row| {
            let initiator = row
                .payload
                .get("initiator")
                .and_then(Value::as_str)
                .ok_or(Unreadable)?;
            Ok(Switch {
                switch: row.event_id.clone(),
                initiator: initiator_of(initiator)?,
                confirmation: confirmation_for(rows, row)?,
            })
        })
        .collect()
}

fn initiator_of(name: &str) -> Result<Initiator, Unreadable> {
    match name {
        "owner" => Ok(Initiator::Owner),
        "risk_limit" => Ok(Initiator::RiskLimit),
        "platform_operator" => Ok(Initiator::PlatformOperator),
        _ => Err(Unreadable),
    }
}

/// The owner's confirmation a kill switch carries: its agent-scope `OwnerExitRequested`, copied
/// from the same command (journal spec §9.1 rule 16), when that copy confirmed a bid.
fn confirmation_for(
    rows: &[FoldedEvent],
    switch: &FoldedEvent,
) -> Result<Option<OwnerConfirmation>, Unreadable> {
    let Some(command) = switch.causation_id.as_ref() else {
        return Ok(None);
    };
    let copy = rows.iter().find(|row| {
        row.event_type == "OwnerExitRequested"
            && row.causation_id.as_ref() == Some(command)
            && row.payload.get("scope").and_then(Value::as_str) == Some("agent")
    });
    copy.map_or(Ok(None), |row| confirmation(&row.payload))
}

/// A confirmed bid's members (journal spec §9.1's `OwnerExitRequested`), or none when the owner
/// confirmed none: then an equity sell waits for the regular session (trading spec §5.5).
fn confirmation(payload: &Value) -> Result<Option<OwnerConfirmation>, Unreadable> {
    if payload.get("confirmed") != Some(&Value::Bool(true)) {
        return Ok(None);
    }
    let text = |member: &str| {
        payload
            .get(member)
            .and_then(Value::as_str)
            .ok_or(Unreadable)
    };
    let step_up = payload
        .get("step_up")
        .and_then(|s| s.get("assertion_id"))
        .and_then(Value::as_str)
        .ok_or(Unreadable)?;
    Ok(Some(OwnerConfirmation {
        bid: Price::parse(text("bid")?).map_err(|_| Unreadable)?,
        bid_size: Qty::parse(text("bid_size")?).map_err(|_| Unreadable)?,
        floor: Price::parse(text("floor")?).map_err(|_| Unreadable)?,
        user: text("user")?.to_owned(),
        step_up: step_up.to_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use mandate_canon::{Key, Object};

    use super::*;

    fn payload(members: &[(&str, Value)]) -> Value {
        let mut object = Object::new();
        for (name, value) in members {
            if let Ok(key) = Key::new(name) {
                object.insert(key, value.clone());
            }
        }
        Value::Object(object)
    }

    fn s(text: &str) -> Value {
        Value::Str(text.to_owned())
    }

    fn row(
        id: &str,
        event_type: &str,
        cause: Option<&str>,
        members: &[(&str, Value)],
    ) -> FoldedEvent {
        FoldedEvent {
            stream: "agent:ws1:agenta".to_owned(),
            seq: Seq(1),
            event_id: EventId(id.to_owned()),
            event_type: event_type.to_owned(),
            causation_id: cause.map(|c| EventId(c.to_owned())),
            payload: payload(members),
        }
    }

    fn switch(id: &str, initiator: &str, cause: &str) -> FoldedEvent {
        row(
            id,
            "KillSwitchActivated",
            Some(cause),
            &[
                ("initiator", s(initiator)),
                ("scope", s("agent")),
                ("subject", s("agenta")),
            ],
        )
    }

    fn owner_exit(id: &str, scope: &str, cause: &str, confirmed: bool) -> FoldedEvent {
        row(
            id,
            "OwnerExitRequested",
            Some(cause),
            &[
                ("scope", s(scope)),
                ("confirmed", Value::Bool(confirmed)),
                ("bid", s("151")),
                ("bid_size", s("300")),
                ("floor", s("146.47")),
                ("user", s("user1")),
                ("step_up", payload(&[("assertion_id", s("stepup1"))])),
            ],
        )
    }

    #[test]
    fn each_initiator_reads_as_its_own() -> Result<(), Unreadable> {
        for (name, want) in [
            ("owner", Initiator::Owner),
            ("risk_limit", Initiator::RiskLimit),
            ("platform_operator", Initiator::PlatformOperator),
        ] {
            let read = switches(&[switch("k1", name, "c1")])?;
            assert!(matches!(read.as_slice(), [Switch { initiator, .. }] if *initiator == want));
        }
        assert_eq!(switches(&[switch("k1", "someone", "c1")]), Err(Unreadable));
        Ok(())
    }

    #[test]
    fn a_switch_carries_the_bid_its_own_commands_agent_exit_confirmed() -> Result<(), Unreadable> {
        let confirmation = |rows: &[FoldedEvent]| -> Result<Option<OwnerConfirmation>, Unreadable> {
            match switches(rows)?.last() {
                Some(Switch { confirmation, .. }) => Ok(confirmation.clone()),
                None => Err(Unreadable),
            }
        };
        let confirmed = confirmation(&[
            owner_exit("e1", "agent", "c1", true),
            switch("k1", "owner", "c1"),
        ])?
        .ok_or(Unreadable)?;
        assert_eq!(
            (
                confirmed.bid.to_string(),
                confirmed.bid_size.to_string(),
                confirmed.floor.to_string(),
                confirmed.user.as_str(),
                confirmed.step_up.as_str()
            ),
            (
                "151".to_owned(),
                "300".to_owned(),
                "146.47".to_owned(),
                "user1",
                "stepup1"
            )
        );
        for others in [
            vec![
                owner_exit("e1", "agent", "c1", false),
                switch("k1", "owner", "c1"),
            ],
            vec![
                owner_exit("e1", "agent", "c2", true),
                switch("k1", "owner", "c1"),
            ],
            vec![
                owner_exit("e1", "instrument", "c1", true),
                switch("k1", "owner", "c1"),
            ],
            vec![
                row(
                    "e1",
                    "AgentModeChanged",
                    Some("c1"),
                    &[("scope", s("agent")), ("to", s("normal"))],
                ),
                switch("k1", "owner", "c1"),
            ],
            vec![
                owner_exit("e1", "agent", "c1", true),
                row(
                    "k1",
                    "KillSwitchActivated",
                    None,
                    &[("initiator", s("owner"))],
                ),
            ],
        ] {
            assert_eq!(confirmation(&others)?, None);
        }
        Ok(())
    }
}
