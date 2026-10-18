//! What the executor reads from streams it does not write (DEC-2003 items 3 to 5, DEC-2033): which
//! agents trade on its account, from the control stream, and the kill switches and order proposals
//! each agent's stream holds.
//!
//! Every reader here is a pure function of journaled rows. A row the executor must act on and
//! cannot read is never skipped: it is [`Unreadable`], and the process stops rather than guess
//! (DEC-85, `AGENTS.md` rule 3).

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::{Value, parse};
use mandate_executor::{
    AccountScope, AgentId, EventId, FoldedEvent, Initiator, OwnerConfirmation, Purpose, Seq,
    TimeInForce,
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
/// no executor (DEC-1171 item 3), nor ends the following an earlier version-2 one began: an agent
/// followed one switch too many only has its own orders cancelled (`AGENTS.md` rule 13). An agent whose deployment stopped is still followed, since it may
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

/// One order the runtime proposed on an agent's stream: its `IntentProposed`, identified by its
/// event id (journal spec §2, §9.1), which the executor hands to its core as the handoff the
/// runtime would make in one process (DEC-2033 item 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub intent: EventId,
    pub instrument: InstrumentId,
    pub side: Side,
    pub qty: Qty,
    /// The limit: the proposal's own, or for an owner's exit at the market the floor the owner
    /// confirmed in the `OwnerExitRequested` it follows from, or none when no floor was confirmed
    /// (DEC-2033 item 4).
    pub limit: Option<Price>,
    pub tif: TimeInForce,
    pub purpose: Purpose,
}

/// The orders one agent's stream proposes, in stream order. An opening or an increase that a later
/// `KillSwitchActivated`, or a later `AgentModeChanged` to any mode but `normal`, follows is left
/// out: the agent stopped wanting it before the executor took it, and leaving it out adds no risk
/// (`AGENTS.md` rule 3; DEC-2033 item 2). An owner's exit at the market is priced at the floor the
/// owner confirmed, since the core takes only limit orders from a handoff (DEC-2033 item 4).
///
/// # Errors
/// [`Unreadable`] for an `IntentProposed` whose members do not read, or that is at the market and
/// not an owner's exit.
pub fn proposals(rows: &[FoldedEvent]) -> Result<Vec<Proposal>, Unreadable> {
    let mut proposed: Vec<Proposal> = Vec::new();
    for row in rows {
        let text = |member: &str| row.payload.get(member).and_then(Value::as_str);
        let stops = match row.event_type.as_str() {
            "IntentProposed" => {
                proposed.push(proposal(rows, row)?);
                false
            }
            "KillSwitchActivated" => true,
            "AgentModeChanged" => text("to") != Some("normal"),
            _ => false,
        };
        if stops {
            proposed.retain(|order| !order.purpose.adds_risk());
        }
    }
    Ok(proposed)
}

/// The floor the owner confirmed for an exit at the market: the `OwnerExitRequested` the proposal
/// follows from on its causation chain, when it confirmed one (journal spec §9.1 rule 12: the floor
/// is set exactly when the bid was confirmed).
fn floor(rows: &[FoldedEvent], proposed: &FoldedEvent) -> Result<Option<Price>, Unreadable> {
    let mut cause = proposed.causation_id.as_ref();
    for _ in 0..rows.len() {
        let Some(row) = cause.and_then(|id| rows.iter().find(|row| &row.event_id == id)) else {
            return Ok(None);
        };
        if row.event_type == "OwnerExitRequested" {
            return match row.payload.get("floor") {
                Some(Value::Str(floor)) => Price::parse(floor).map(Some).map_err(|_| Unreadable),
                Some(Value::Null) => Ok(None),
                _ => Err(Unreadable),
            };
        }
        cause = row.causation_id.as_ref();
    }
    Ok(None)
}

/// An `IntentProposed`'s order (journal spec §9.1): a limit order, or an owner's exit at the
/// market at its confirmed floor.
fn proposal(rows: &[FoldedEvent], row: &FoldedEvent) -> Result<Proposal, Unreadable> {
    let text = |member: &str| row.payload.get(member).and_then(Value::as_str);
    let limit = match (text("type"), text("purpose")) {
        (Some("limit"), _) => {
            Some(Price::parse(text("limit_price").ok_or(Unreadable)?).map_err(|_| Unreadable)?)
        }
        (Some("market"), Some("owner_exit")) => floor(rows, row)?,
        _ => return Err(Unreadable),
    };
    let side = match text("side") {
        Some("buy") => Side::Buy,
        Some("sell") => Side::Sell,
        _ => return Err(Unreadable),
    };
    let tif = match text("tif") {
        Some("day") => TimeInForce::Day,
        Some("gtc") => TimeInForce::Gtc,
        Some("ioc") => TimeInForce::Ioc,
        _ => return Err(Unreadable),
    };
    let purpose = match text("purpose") {
        Some("open") => Purpose::Open,
        Some("increase") => Purpose::Increase,
        Some("discretionary_exit") => Purpose::DiscretionaryExit,
        Some("risk_exit") => Purpose::RiskExit,
        Some("owner_exit") => Purpose::OwnerExit,
        _ => return Err(Unreadable),
    };
    let member = |name: &str| text(name).ok_or(Unreadable);
    Ok(Proposal {
        intent: row.event_id.clone(),
        instrument: InstrumentId::new(member("instrument_id")?).map_err(|_| Unreadable)?,
        side,
        qty: Qty::parse(member("qty")?).map_err(|_| Unreadable)?,
        limit,
        tif,
        purpose,
    })
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

    fn proposed(id: &str, purpose: &str, kind: &str, tif: &str) -> FoldedEvent {
        row(
            id,
            "IntentProposed",
            Some("decision"),
            &[
                ("instrument_id", s("AAPL")),
                ("limit_price", s("150")),
                ("purpose", s(purpose)),
                ("qty", s("10")),
                ("side", s("sell")),
                ("tif", s(tif)),
                ("type", s(kind)),
            ],
        )
    }

    fn mode(to: &str) -> FoldedEvent {
        row(
            "m1",
            "AgentModeChanged",
            None,
            &[("from", s("normal")), ("to", s(to)), ("reason", s("owner"))],
        )
    }

    fn ids(rows: &[FoldedEvent]) -> Result<Vec<String>, Unreadable> {
        Ok(proposals(rows)?
            .into_iter()
            .map(|proposal| proposal.intent.0)
            .collect())
    }

    #[test]
    fn every_purpose_and_time_in_force_reads() -> Result<(), Unreadable> {
        for (text, purpose) in [
            ("open", Purpose::Open),
            ("increase", Purpose::Increase),
            ("discretionary_exit", Purpose::DiscretionaryExit),
            ("risk_exit", Purpose::RiskExit),
            ("owner_exit", Purpose::OwnerExit),
        ] {
            let read = proposals(&[proposed("i1", text, "limit", "day")])?;
            assert_eq!(
                read.iter().map(|p| p.purpose).collect::<Vec<_>>(),
                [purpose]
            );
        }
        for (text, tif) in [
            ("day", TimeInForce::Day),
            ("gtc", TimeInForce::Gtc),
            ("ioc", TimeInForce::Ioc),
        ] {
            let read = proposals(&[proposed("i1", "risk_exit", "limit", text)])?;
            assert_eq!(read.iter().map(|p| p.tif).collect::<Vec<_>>(), [tif]);
        }
        Ok(())
    }

    #[test]
    fn a_later_switch_or_stopping_mode_drops_openings_and_increases_only() -> Result<(), Unreadable>
    {
        let waiting = [
            proposed("open", "open", "limit", "day"),
            proposed("increase", "increase", "limit", "day"),
            proposed("exit", "discretionary_exit", "limit", "day"),
        ];
        for stop in [switch("k1", "owner", "c1"), mode("paused")] {
            let mut rows = waiting.to_vec();
            rows.push(stop);
            rows.push(proposed("after", "open", "limit", "day"));
            assert_eq!(ids(&rows)?, ["exit", "after"]);
        }
        let mut resumed = waiting.to_vec();
        resumed.push(mode("normal"));
        assert_eq!(ids(&resumed)?, ["open", "increase", "exit"]);
        Ok(())
    }

    fn requested(id: &str, floor: Option<&str>) -> FoldedEvent {
        let confirmed = |value: &str| floor.map_or(Value::Null, |_| s(value));
        row(
            id,
            "OwnerExitRequested",
            Some("command"),
            &[
                ("bid", confirmed("150")),
                ("bid_size", confirmed("100")),
                ("confirmed", Value::Bool(floor.is_some())),
                ("floor", floor.map_or(Value::Null, s)),
                ("scope", s("instrument")),
                ("step_up", Value::Null),
                ("step_up_status", s("absent")),
                ("subject", s("AAPL")),
                ("user", s("u1")),
            ],
        )
    }

    fn at_market(cause: &str) -> FoldedEvent {
        let mut proposed = proposed("i1", "owner_exit", "market", "day");
        proposed.causation_id = Some(EventId(cause.to_owned()));
        proposed
    }

    fn limits(rows: &[FoldedEvent]) -> Result<Vec<Option<String>>, Unreadable> {
        Ok(proposals(rows)?
            .into_iter()
            .map(|proposal| proposal.limit.map(|limit| limit.to_string()))
            .collect())
    }

    #[test]
    fn an_owners_exit_at_the_market_is_priced_at_its_confirmed_floor() -> Result<(), Unreadable> {
        let floored = [requested("r1", Some("142.5")), at_market("r1")];
        assert_eq!(limits(&floored)?, [Some("142.5".to_owned())]);
        let decided = row("d1", "DecisionMade", Some("r1"), &[]);
        let chained = [requested("r1", Some("142.5")), decided, at_market("d1")];
        assert_eq!(limits(&chained)?, [Some("142.5".to_owned())]);
        let unconfirmed = [requested("r1", None), at_market("r1")];
        assert_eq!(limits(&unconfirmed)?, [None]);
        assert_eq!(limits(&[at_market("r1")])?, [None]);
        let other = [requested("r2", Some("142.5")), at_market("r1")];
        assert_eq!(limits(&other)?, [None]);
        Ok(())
    }

    #[test]
    fn only_an_owners_exit_may_be_at_the_market() {
        assert_eq!(
            ids(&[proposed("i1", "risk_exit", "market", "day")]),
            Err(Unreadable)
        );
        assert_eq!(
            ids(&[proposed("i1", "owner_exit", "stop", "day")]),
            Err(Unreadable)
        );
        let mut unreadable = requested("r1", Some("142.5"));
        unreadable.payload = payload(&[("floor", Value::Bool(true))]);
        assert_eq!(ids(&[unreadable, at_market("r1")]), Err(Unreadable));
    }
}
