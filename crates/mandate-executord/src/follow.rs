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
    AccountScope, AgentId, EventId, FoldedEvent, HandedConfirmation, Initiator, MandateVersion,
    OwnerConfirmation, Purpose, RiskClock, Seq, TimeInForce,
};
use mandate_journal::{Environment, StoredEvent};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;

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
/// `environment` (journal spec §9.8, §9.24, DEC-1171, DEC-2343), with what a revocation changes
/// for them.
///
/// An agent is followed when any of its version-2 `AgentDeployed` named a connection that a
/// version-2 `ConnectionEstablished` in this environment ever named this `account_ref` for, and a
/// later deployment naming another connection never drops it (DEC-2343 item 2). A revocation never
/// ends the following either (DEC-2343 item 1): the agent's kill switches, exits and protective
/// re-placements are still read, and it is `revoked`, so none of its openings is handed, while any
/// of those connections' latest establishment or revocation is not a version-2 establishment
/// naming this account. A version-1 deployment names no connection, so it neither begins nor ends
/// any following (DEC-1171 item 3), and an agent whose deployment stopped is still followed, since
/// it may still hold orders or a position its kill switch must reach (`AGENTS.md` rule 13). Each
/// agent comes with the `mandate_version` of its latest version-2 deployment on this account, the
/// version it runs under (DEC-2034 item 1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Following {
    pub agents: BTreeMap<AgentId, MandateVersion>,
    pub revoked: BTreeSet<AgentId>,
    /// Each followed agent's deployments on this account's connections, by control-stream `seq`.
    deployments: BTreeMap<AgentId, Vec<(u64, String)>>,
    /// Each of this account's connections' revocations, by control-stream `seq`.
    revocations: BTreeMap<String, Vec<u64>>,
    /// The kill-switch commands of connection or workspace scope, in control-stream order.
    commands: Vec<KillCommand>,
}

/// An `OwnerCommandIssued` `kill_switch` the executor may read itself (DEC-2343 item 3): its
/// record, its `seq`, and the connection it names, or `None` for the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
struct KillCommand {
    id: EventId,
    seq: u64,
    connection: Option<String>,
}

/// Reads [`Following`] from the control stream.
///
/// # Errors
/// [`Unreadable`] for a deployment, connection, or kill-switch command record whose members do not
/// read: an agent the account may owe a kill switch is never dropped for it (DEC-85).
pub fn agents_of(
    control: &[StoredEvent],
    scope: &AccountScope,
    environment: Environment,
) -> Result<Following, Unreadable> {
    let mut ours: BTreeSet<String> = BTreeSet::new();
    let mut live: BTreeMap<String, bool> = BTreeMap::new();
    let mut deployed: BTreeMap<String, Vec<(u64, String, String)>> = BTreeMap::new();
    let mut revocations: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    let mut commands = Vec::new();
    for row in control
        .iter()
        .filter(|row| row.environment == environment.as_str())
    {
        if !matches!(
            row.event_type.as_str(),
            "ConnectionEstablished"
                | "ConnectionRevoked"
                | "AgentDeployed"
                | "AgentStopped"
                | "OwnerCommandIssued"
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
                let connection = text("connection_id")?;
                if mine {
                    ours.insert(connection.clone());
                }
                live.insert(connection, mine);
            }
            ("ConnectionEstablished", _) => {
                live.insert(text("connection_id")?, false);
            }
            ("ConnectionRevoked", _) => {
                let connection = text("connection_id")?;
                revocations
                    .entry(connection.clone())
                    .or_default()
                    .push(row.seq);
                live.insert(connection, false);
            }
            ("AgentDeployed", 2) => {
                deployed.entry(text("agent_id")?).or_default().push((
                    row.seq,
                    text("connection_id")?,
                    text("mandate_version")?,
                ));
            }
            ("OwnerCommandIssued", _) => {
                if text("command")? != "kill_switch" {
                    continue;
                }
                let connection = match text("scope")?.as_str() {
                    "connection" => Some(text("subject")?),
                    "workspace" if text("subject")? == scope.workspace.0 => None,
                    _ => continue,
                };
                commands.push(KillCommand {
                    id: event.event_id.clone(),
                    seq: row.seq,
                    connection,
                });
            }
            _ => {
                text("agent_id")?;
            }
        }
    }
    let mut following = Following {
        revocations: revocations
            .into_iter()
            .filter(|(connection, _)| ours.contains(connection))
            .collect(),
        commands,
        ..Following::default()
    };
    for (agent, deployments) in deployed {
        let here: Vec<(u64, String, String)> = deployments
            .into_iter()
            .filter(|(_, connection, _)| ours.contains(connection))
            .collect();
        let Some((_, _, version)) = here.last() else {
            continue;
        };
        let agent = AgentId(agent);
        following
            .agents
            .insert(agent.clone(), MandateVersion(version.clone()));
        if here
            .iter()
            .any(|(_, connection, _)| !live.get(connection).copied().unwrap_or(false))
        {
            following.revoked.insert(agent.clone());
        }
        following.deployments.insert(
            agent,
            here.into_iter()
                .map(|(seq, connection, _)| (seq, connection))
                .collect(),
        );
    }
    Ok(following)
}

impl Following {
    /// The kill-switch commands the executor applies itself to `agent` (DEC-2343 item 3), in
    /// control-stream order: each command of its connection's or the workspace's scope issued
    /// after the agent was deployed on one of this account's connections it names, when one of
    /// those connections is revoked at or after the command. Every other covered agent waits for
    /// the runtime's copy, which carries the owner's confirmation.
    pub fn commands_for(&self, agent: &AgentId) -> Vec<EventId> {
        let deployments = self
            .deployments
            .get(agent)
            .map(Vec::as_slice)
            .unwrap_or_default();
        self.commands
            .iter()
            .filter(|command| {
                deployments.iter().any(|(deployed, connection)| {
                    *deployed < command.seq
                        && command
                            .connection
                            .as_ref()
                            .is_none_or(|named| named == connection)
                        && self
                            .revocations
                            .get(connection)
                            .is_some_and(|seqs| seqs.iter().any(|seq| *seq >= command.seq))
                })
            })
            .map(|command| command.id.clone())
            .collect()
    }
}

/// A `KillSwitchActivated` on an agent's stream: the runtime's copy of a kill switch that covers
/// the agent, which the executor applies as its own agent-scoped switch over its own ledger
/// (trading spec §5.5; DEC-2003 item 4). Whatever scope the copy records, the executor's switch
/// touches only this agent, so it never reaches an account-wide endpoint (`AGENTS.md` rule 13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switch {
    pub switch: EventId,
    /// The record the copy was made from, its `causation_id`: for a connection's or the workspace's
    /// switch, the control stream's command (DEC-2343 item 4).
    pub cause: Option<EventId>,
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
                cause: row.causation_id.clone(),
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
    /// The limit: the proposal's own; for an owner's exit at the market the floor the owner
    /// confirmed without valid step-up in the `OwnerExitRequested` it follows from (DEC-2033 item
    /// 4); none for one confirmed with valid step-up, which carries its confirmation instead, or
    /// for any other exit at the market (DEC-2040 item 4).
    pub limit: Option<Price>,
    pub tif: TimeInForce,
    pub purpose: Purpose,
    /// The owner's confirmation an owner's exit at the market is handed with: the bid, bid size and
    /// floor of the `OwnerExitRequested` it follows from, when that request confirmed them with
    /// valid step-up, expiring 300 s after the step-up (DEC-2040 items 1, 4 and 7).
    pub confirmation: Option<HandedConfirmation>,
}

/// The orders one agent's stream proposes, in stream order. An opening or an increase that a later
/// `KillSwitchActivated`, or a later `AgentModeChanged` to any mode but `normal`, follows is left
/// out: the agent stopped wanting it before the executor took it, and leaving it out adds no risk
/// (`AGENTS.md` rule 3; DEC-2033 item 2). An owner's exit at the market is priced at the floor the
/// owner confirmed, since the core takes only limit orders from a handoff (DEC-2033 item 4). One
/// `OwnerExitRequested` confirms one exit: a later proposal that follows from the same request is
/// handed its floor as its limit and no confirmation, so it waits for the regular session.
///
/// # Errors
/// [`Unreadable`] for an `IntentProposed` whose members do not read, or that is an opening or an
/// increase at the market, which the journal never holds (journal spec §9.1 rule 3).
pub fn proposals(rows: &[FoldedEvent]) -> Result<Vec<Proposal>, Unreadable> {
    let mut proposed: Vec<Proposal> = Vec::new();
    let mut handed: Vec<EventId> = Vec::new();
    for row in rows {
        let text = |member: &str| row.payload.get(member).and_then(Value::as_str);
        let stops = match row.event_type.as_str() {
            "IntentProposed" => {
                let mut order = proposal(rows, row)?;
                if let Some(confirmation) = order.confirmation.take() {
                    if handed.contains(&confirmation.copied_from) {
                        order.limit = Some(confirmation.floor);
                    } else {
                        handed.push(confirmation.copied_from.clone());
                        order.confirmation = Some(confirmation);
                    }
                }
                proposed.push(order);
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

/// How long a confirmation lasts after the step-up that gave it: mandate spec §6.1's step-up
/// freshness window (DEC-2040 item 7).
const CONFIRMATION_S: i64 = 300;

/// `handed`, expiring no later than [`CONFIRMATION_S`] after `now`, the clock of the batch that
/// hands it: a step-up instant later than that batch cannot stretch the window past DEC-2040 item
/// 7.
#[must_use]
pub fn bounded(handed: HandedConfirmation, now: UtcNanos) -> HandedConfirmation {
    let latest = RiskClock::from_secs(now.secs().saturating_add(CONFIRMATION_S));
    HandedConfirmation {
        expires_at: handed.expires_at.min(latest),
        ..handed
    }
}

/// What the owner confirmed for an exit at the market, as `(limit, confirmation)`: the
/// `OwnerExitRequested` the proposal follows from on its causation chain, for the exit's own
/// `instrument` (a request of another scope or instrument confirms nothing for it). With valid
/// step-up its bid, bid size and floor are handed as the confirmation, expiring
/// [`CONFIRMATION_S`] after the step-up on the whole second, and the exit goes as proposed with no
/// limit (DEC-2040 items 1, 4 and 7). Confirmed without valid step-up, the floor is its limit, as
/// before, and nothing is handed that would let it sell outside the session (mandate spec §6.1,
/// DEC-2033 item 4). Unconfirmed, it has neither (journal spec §9.1 rule 12).
fn confirmed(
    rows: &[FoldedEvent],
    proposed: &FoldedEvent,
    instrument: Option<&str>,
) -> Result<(Option<Price>, Option<HandedConfirmation>), Unreadable> {
    let mut cause = proposed.causation_id.as_ref();
    for _ in 0..rows.len() {
        let Some(row) = cause.and_then(|id| rows.iter().find(|row| &row.event_id == id)) else {
            return Ok((None, None));
        };
        if row.event_type == "OwnerExitRequested" {
            let floor = match row.payload.get("floor") {
                Some(Value::Str(floor)) => Some(Price::parse(floor).map_err(|_| Unreadable)?),
                Some(Value::Null) => None,
                _ => return Err(Unreadable),
            };
            let text = |member: &str| row.payload.get(member).and_then(Value::as_str);
            let own = text("scope") == Some("instrument") && text("subject") == instrument;
            let Some(floor) = floor.filter(|_| own) else {
                return Ok((None, None));
            };
            if text("step_up_status") != Some("valid") {
                return Ok((Some(floor), None));
            }
            let authenticated_at = row
                .payload
                .get("step_up")
                .and_then(|step_up| step_up.get("authenticated_at"))
                .and_then(Value::as_str)
                .and_then(|at| UtcNanos::parse(at).ok())
                .ok_or(Unreadable)?;
            let price = |member: &str| {
                text(member)
                    .and_then(|value| Price::parse(value).ok())
                    .ok_or(Unreadable)
            };
            let handed = HandedConfirmation {
                bid: price("bid")?,
                bid_size: text("bid_size")
                    .and_then(|value| Qty::parse(value).ok())
                    .ok_or(Unreadable)?,
                floor,
                expires_at: RiskClock::from_secs(
                    authenticated_at.secs().saturating_add(CONFIRMATION_S),
                ),
                copied_from: row.event_id.clone(),
            };
            return Ok((None, Some(handed)));
        }
        cause = row.causation_id.as_ref();
    }
    Ok((None, None))
}

/// An `IntentProposed`'s order (journal spec §9.1): a limit order, an owner's exit at the market at
/// its confirmed floor, or any other exit at the market with no limit.
fn proposal(rows: &[FoldedEvent], row: &FoldedEvent) -> Result<Proposal, Unreadable> {
    let text = |member: &str| row.payload.get(member).and_then(Value::as_str);
    let (limit, confirmation) = match (text("type"), text("purpose")) {
        (Some("limit"), _) => (
            Some(Price::parse(text("limit_price").ok_or(Unreadable)?).map_err(|_| Unreadable)?),
            None,
        ),
        (Some("market"), Some("owner_exit")) => confirmed(rows, row, text("instrument_id"))?,
        (Some("market"), Some("risk_exit" | "discretionary_exit")) => (None, None),
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
        confirmation,
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
        requested_for(id, floor, "instrument", "AAPL")
    }

    fn requested_for(id: &str, floor: Option<&str>, scope: &str, subject: &str) -> FoldedEvent {
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
                ("scope", s(scope)),
                ("step_up", Value::Null),
                ("step_up_status", s("absent")),
                ("subject", s(subject)),
                ("user", s("u1")),
            ],
        )
    }

    /// A request of `AAPL` whose bid the owner confirmed with valid step-up at `authenticated_at`.
    fn stepped_up(id: &str, floor: &str, authenticated_at: &str) -> FoldedEvent {
        let mut request = requested(id, Some(floor));
        request.payload = payload(&[
            ("bid", s("150")),
            ("bid_size", s("100")),
            ("confirmed", Value::Bool(true)),
            ("floor", s(floor)),
            ("scope", s("instrument")),
            (
                "step_up",
                payload(&[
                    ("assertion_id", s("a1")),
                    ("authenticated_at", s(authenticated_at)),
                    ("method", s("passkey")),
                ]),
            ),
            ("step_up_status", s("valid")),
            ("subject", s("AAPL")),
            ("user", s("u1")),
        ]);
        request
    }

    /// DEC-2040 items 1, 4 and 7: an owner's exit at the market whose request confirmed the bid
    /// with valid step-up is handed as proposed, with no limit, and with the confirmation, which
    /// expires 300 s after the step-up, on the whole second. One confirmed without valid step-up
    /// keeps its floor as its limit and carries no confirmation (mandate spec §6.1).
    #[test]
    fn a_stepped_up_owner_exit_at_the_market_is_handed_its_confirmation() -> Result<(), Unreadable>
    {
        let rows = [
            stepped_up("r1", "142.5", "2026-10-19T21:00:00.500000000Z"),
            at_market("r1"),
        ];
        let read = proposals(&rows)?;
        let expires_at = UtcNanos::parse("2026-10-19T21:05:00.000000000Z")
            .map_err(|_| Unreadable)?
            .secs();
        assert_eq!(
            read.iter()
                .map(|proposal| (proposal.limit, proposal.confirmation.clone()))
                .collect::<Vec<_>>(),
            [(
                None,
                Some(HandedConfirmation {
                    bid: Price::parse("150").map_err(|_| Unreadable)?,
                    bid_size: Qty::parse("100").map_err(|_| Unreadable)?,
                    floor: Price::parse("142.5").map_err(|_| Unreadable)?,
                    expires_at: RiskClock::from_secs(expires_at),
                    copied_from: EventId("r1".to_owned()),
                })
            )]
        );
        let absent = proposals(&[requested("r1", Some("142.5")), at_market("r1")])?;
        assert_eq!(
            absent
                .iter()
                .map(|proposal| (proposal.limit, proposal.confirmation.is_some()))
                .collect::<Vec<_>>(),
            [(Some(Price::parse("142.5").map_err(|_| Unreadable)?), false)],
            "without valid step-up: the floor as its limit, no confirmation"
        );
        let mut broken = stepped_up("r1", "142.5", "not a time");
        broken.event_id = EventId("r1".to_owned());
        assert_eq!(
            ids(&[broken, at_market("r1")]),
            Err(Unreadable),
            "a valid step-up whose instant does not read"
        );
        Ok(())
    }

    /// DEC-2040 item 7: a confirmation expires no later than 300 s after the batch that hands it,
    /// whatever instant the step-up names; an earlier step-up keeps its own, earlier expiry.
    #[test]
    fn a_confirmation_never_outlasts_the_batch_that_hands_it() -> Result<(), Unreadable> {
        let at = |text: &str| UtcNanos::parse(text).map_err(|_| Unreadable);
        let future = stepped_up("r1", "142.5", "2026-10-19T22:00:00.000000000Z");
        let read = proposals(&[future, at_market("r1")])?;
        let handed = read
            .into_iter()
            .find_map(|proposal| proposal.confirmation)
            .ok_or(Unreadable)?;
        let now = at("2026-10-19T21:00:00.700000000Z")?;
        assert_eq!(
            bounded(handed.clone(), now).expires_at,
            RiskClock::from_secs(at("2026-10-19T21:05:00.000000000Z")?.secs()),
            "a step-up an hour after the batch"
        );
        let later = at("2026-10-19T22:30:00.000000000Z")?;
        assert_eq!(
            bounded(handed.clone(), later).expires_at,
            handed.expires_at,
            "a step-up before the batch keeps its own expiry"
        );
        Ok(())
    }

    /// One `OwnerExitRequested` confirms one exit: a second proposal from the same request takes
    /// its floor as its limit and no confirmation.
    #[test]
    fn one_request_confirms_one_exit() -> Result<(), Unreadable> {
        let mut second = at_market("r1");
        second.event_id = EventId("i2".to_owned());
        let rows = [
            stepped_up("r1", "142.5", "2026-10-19T21:00:00.500000000Z"),
            at_market("r1"),
            second,
        ];
        assert_eq!(
            proposals(&rows)?
                .iter()
                .map(|proposal| (
                    proposal.limit.map(|limit| limit.to_string()),
                    proposal.confirmation.is_some()
                ))
                .collect::<Vec<_>>(),
            [(None, true), (Some("142.5".to_owned()), false)]
        );
        Ok(())
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

    /// A floor is the owner's only for the instrument the exit sells: an `OwnerExitRequested` of
    /// another scope or another instrument confirms none for it.
    #[test]
    fn a_floor_is_taken_only_from_a_request_for_the_exits_instrument() -> Result<(), Unreadable> {
        for (scope, subject) in [("agent", "AAPL"), ("instrument", "MSFT"), ("account", "a1")] {
            let other = [
                requested_for("r1", Some("142.5"), scope, subject),
                at_market("r1"),
            ];
            assert_eq!(limits(&other)?, [None], "{scope} {subject}");
        }
        Ok(())
    }

    /// Any exit may be at the market (journal spec §9.1 rule 3): it reads, with no limit, so the
    /// process alerts on it rather than stop; an opening or an increase at the market, or an order
    /// of another type, does not read.
    #[test]
    fn an_exit_at_the_market_reads_with_no_limit() -> Result<(), Unreadable> {
        for purpose in ["risk_exit", "discretionary_exit"] {
            let read = proposals(&[proposed("i1", purpose, "market", "day")])?;
            assert_eq!(read.iter().map(|p| p.limit).collect::<Vec<_>>(), [None]);
        }
        for purpose in ["open", "increase"] {
            assert_eq!(
                ids(&[proposed("i1", purpose, "market", "day")]),
                Err(Unreadable)
            );
        }
        assert_eq!(
            ids(&[proposed("i1", "owner_exit", "stop", "day")]),
            Err(Unreadable)
        );
        let mut unreadable = requested("r1", Some("142.5"));
        unreadable.payload = payload(&[("floor", Value::Bool(true))]);
        assert_eq!(ids(&[unreadable, at_market("r1")]), Err(Unreadable));
        Ok(())
    }
}
