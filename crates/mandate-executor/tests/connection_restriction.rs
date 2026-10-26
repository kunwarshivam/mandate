//! E7-2z slice C (trading spec §7.3 row 5; connections spec CN-6, §9.1; journal spec §9.8
//! `ConnectionStateChanged` with rules 59 and 60, §9.5 rule 309; DEC-2520, DEC-2826): a connection
//! the platform cannot reach or use restricts the account to closing first, every agent to
//! `exits_only`, with the owner told by the kind `connection_unavailable`; only in a later batch
//! does the executor record the connection's state change. The oracles are the records each case
//! expects, written out from the specs, never read back from the executor.

mod common;

use common::{
    ACCOUNT_STREAM, FixedInstruments, FixedMandate, OTHER_AGENT, Shell, TestIds, agent, config,
    copied, ports, text, with_clock,
};
use mandate_canon::Value;
use mandate_executor::{
    AccountState, ConnectionTrouble, DegradeReason, EventId, Input, Mode, SuspendReason,
};
use proptest::prelude::*;

const CONNECTION: &str = "conn-1";

/// A started executor on an opened stream with its connection established (its copy of
/// `ConnectionEstablished`, rule 68), reconciled, as a production shell starts.
fn established(ports: &mandate_executor::Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell
        .fold_one(&common::stream_opened())
        .expect("the stream opens");
    let seq = shell.head().0.saturating_add(1);
    shell
        .fold_one(&copied(
            ACCOUNT_STREAM,
            seq,
            "ConnectionEstablished",
            with_clock(
                &[
                    ("account_ref", text("01J8Z2ACCT0000000000000001")),
                    ("broker", text("alpaca")),
                    ("connection_id", text(CONNECTION)),
                    ("environment", text("paper")),
                    ("margin_attestation", Value::Null),
                    ("scopes", Value::Array(vec![text("data"), text("trading")])),
                    ("step_up", Value::Null),
                    ("user", text("usr_1")),
                ],
                1,
            ),
            &EventId("ctl:ws1-7".to_owned()),
        ))
        .expect("the copy folds");
    shell.restart_ready(ports)
}

/// A started executor whose stream holds no copy of the establishment yet.
fn unestablished(ports: &mandate_executor::Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell
        .fold_one(&common::stream_opened())
        .expect("the stream opens");
    shell.restart_ready(ports)
}

const NETWORK: Input = Input::Connection(ConnectionTrouble::Degraded(DegradeReason::NetworkErrors));
const HEADROOM: Input = Input::Connection(ConnectionTrouble::Degraded(DegradeReason::RateHeadroom));
const EXPIRED: Input = Input::Connection(ConnectionTrouble::Suspended(
    SuspendReason::CredentialExpired,
));

fn member(draft: &mandate_executor::EventDraft, name: &str) -> String {
    draft
        .payload
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The restrictions a run journaled, as `(restriction, cause, reason_code)`.
fn restrictions(ran: &common::Ran) -> Vec<(String, String, String)> {
    ran.drafts
        .iter()
        .filter(|d| d.event_type == "AccountRestrictionChanged")
        .map(|d| {
            (
                member(d, "restriction"),
                member(d, "cause"),
                member(d, "reason_code"),
            )
        })
        .collect()
}

/// The state changes a run journaled, as `(connection_id, from, to, reason)`.
fn changes(ran: &common::Ran) -> Vec<(String, String, String, String)> {
    ran.drafts
        .iter()
        .filter(|d| d.event_type == "ConnectionStateChanged")
        .map(|d| {
            (
                member(d, "connection_id"),
                member(d, "from"),
                member(d, "to"),
                member(d, "reason"),
            )
        })
        .collect()
}

fn change(from: &str, to: &str, reason: &str) -> (String, String, String, String) {
    (
        CONNECTION.to_owned(),
        from.to_owned(),
        to.to_owned(),
        reason.to_owned(),
    )
}

fn connection_restriction() -> (String, String, String) {
    (
        "closing_only".to_owned(),
        "connection_unavailable".to_owned(),
        "account_restricted".to_owned(),
    )
}

/// CN-6 and journal §9.8's ordering: the restriction, every agent `exits_only`, and the owner's
/// alert first, with no state change in that batch; the state change in the next; then nothing.
#[test]
fn the_restriction_comes_first_and_the_state_change_in_a_later_batch() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = established(&ports);
    let first = shell.run(NETWORK, &ports);
    assert_eq!(
        first.draft_types(),
        [
            "AccountRestrictionChanged",
            "AgentModeApplied",
            "OwnerAlertSent"
        ],
        "the halt is never behind the state change (journal §9.8)"
    );
    assert_eq!(restrictions(&first), [connection_restriction()]);
    assert_eq!(first.notifications, ["connection_unavailable"]);
    assert_eq!(shell.state.account_state(), AccountState::ClosingOnly);
    assert_eq!(
        shell.state.effective_mode(&agent(OTHER_AGENT)),
        Mode::ExitsOnly
    );
    let second = shell.run(NETWORK, &ports);
    assert_eq!(second.draft_types(), ["ConnectionStateChanged"]);
    assert_eq!(
        changes(&second),
        [change("active", "degraded", "network_errors")]
    );
    assert!(second.notifications.is_empty());
    let third = shell.run(NETWORK, &ports);
    assert!(third.drafts.is_empty(), "{:?}", third.draft_types());
}

/// Into `suspended` from `degraded`: the cause is already in force, so only the state change.
/// A degrading cause while `suspended` is held (DEC-687 item 1): nothing is journaled, since
/// `degraded` never follows `suspended` (rule 60).
#[test]
fn suspension_after_degradation_journals_only_its_state_change_and_a_later_degradation_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = established(&ports);
    shell.run(NETWORK, &ports);
    shell.run(NETWORK, &ports);
    let suspended = shell.run(EXPIRED, &ports);
    assert_eq!(
        changes(&suspended),
        [change("degraded", "suspended", "credential_expired")]
    );
    assert!(restrictions(&suspended).is_empty());
    assert!(suspended.notifications.is_empty());
    let held = shell.run(HEADROOM, &ports);
    assert!(held.drafts.is_empty(), "{:?}", held.draft_types());
}

/// Each new cause while already `degraded` is journaled once (connections spec §8.2, DEC-687):
/// a state change that stays `degraded` with the new reason, never twice.
#[test]
fn a_new_degrading_cause_is_journaled_once_and_stays_degraded() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = established(&ports);
    shell.run(NETWORK, &ports);
    shell.run(NETWORK, &ports);
    let another = shell.run(HEADROOM, &ports);
    assert_eq!(
        changes(&another),
        [change("degraded", "degraded", "rate_headroom")]
    );
    assert!(shell.run(HEADROOM, &ports).drafts.is_empty());
    assert!(shell.run(NETWORK, &ports).drafts.is_empty());
}

/// A `condition_cleared` ends the causes before it (DEC-687 item 1, connections spec §8.2): a cause
/// that recurs after it is journaled again, so the acknowledgment it must refuse has a record to
/// read, while the restriction stays in force.
#[test]
fn a_cause_after_the_condition_cleared_is_journaled_again() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = established(&ports);
    shell.run(NETWORK, &ports);
    shell.run(NETWORK, &ports);
    let seq = shell.head().0.saturating_add(1);
    let clock = shell.state.risk_clock().map_or(1, |clock| clock.secs());
    shell
        .fold_one(&common::event(
            ACCOUNT_STREAM,
            seq,
            "ConnectionStateChanged",
            with_clock(
                &[
                    ("connection_id", text(CONNECTION)),
                    ("from", text("degraded")),
                    ("reason", text("condition_cleared")),
                    ("to", text("degraded")),
                ],
                clock,
            ),
        ))
        .expect("the clearing folds");
    let again = shell.run(NETWORK, &ports);
    assert_eq!(
        changes(&again),
        [change("degraded", "degraded", "network_errors")]
    );
    assert!(restrictions(&again).is_empty());
    assert!(shell.run(NETWORK, &ports).drafts.is_empty());
}

/// A replay from the stream alone reaches the same place: the restriction in force and the state
/// it recorded, so a restart journals nothing again.
#[test]
fn a_replay_journals_nothing_twice() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = established(&ports);
    shell.run(EXPIRED, &ports);
    let (mut replayed, _) = shell.restart(&ports);
    let after = replayed.run(EXPIRED, &ports);
    assert_eq!(
        changes(&after),
        [change("active", "suspended", "credential_expired")],
        "the restriction replayed, so the state change is what is left"
    );
    assert!(restrictions(&after).is_empty());
    let (mut again, _) = replayed.restart(&ports);
    assert!(again.run(EXPIRED, &ports).drafts.is_empty());
    assert_eq!(again.state.account_state(), AccountState::ClosingOnly);
}

/// Before its copy of the establishment the stream may hold no state change (rule 68), so only
/// the restriction is journaled: openings halt all the same.
#[test]
fn before_the_establishment_only_the_restriction_is_journaled() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = unestablished(&ports);
    let first = shell.run(EXPIRED, &ports);
    assert_eq!(restrictions(&first), [connection_restriction()]);
    for _ in 0..3 {
        let next = shell.run(EXPIRED, &ports);
        assert!(next.drafts.is_empty(), "{:?}", next.draft_types());
    }
}

/// The connection's cause beside a broker's block is a cause of its own (rule 309): journaled and
/// told to the owner as `connection_unavailable`, moving no agent, the account staying blocked.
#[test]
fn beside_a_broker_block_the_connection_cause_is_told_and_moves_no_agent() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = established(&ports);
    shell.run(
        Input::BrokerUpdate(mandate_executor::BrokerUpdate::Account(
            mandate_executor::BrokerAccount {
                trading_blocked: true,
                ..common::broker_account()
            },
        )),
        &ports,
    );
    let ran = shell.run(NETWORK, &ports);
    assert_eq!(restrictions(&ran), [connection_restriction()]);
    assert!(!ran.draft_types().contains(&"AgentModeApplied"));
    assert_eq!(ran.notifications, ["connection_unavailable"]);
    assert_eq!(shell.state.account_state(), AccountState::Blocked);
}

/// One trouble the generator reports.
fn trouble() -> impl Strategy<Value = ConnectionTrouble> {
    prop_oneof![
        Just(ConnectionTrouble::Degraded(DegradeReason::NetworkErrors)),
        Just(ConnectionTrouble::Degraded(DegradeReason::RateHeadroom)),
        Just(ConnectionTrouble::Degraded(DegradeReason::ContractDrift)),
        Just(ConnectionTrouble::Suspended(
            SuspendReason::AuthorizationFailed
        )),
        Just(ConnectionTrouble::Suspended(
            SuspendReason::CredentialExpired
        )),
        Just(ConnectionTrouble::Suspended(SuspendReason::RefreshFailed)),
        Just(ConnectionTrouble::Suspended(SuspendReason::CheckFailed)),
        Just(ConnectionTrouble::Suspended(SuspendReason::LeaseExpired)),
    ]
}

/// The independent model: `(to, reason)` of a trouble, from journal §9.8's reason table.
fn modelled(trouble: ConnectionTrouble) -> (&'static str, &'static str) {
    match trouble {
        ConnectionTrouble::Degraded(DegradeReason::NetworkErrors) => ("degraded", "network_errors"),
        ConnectionTrouble::Degraded(DegradeReason::RateHeadroom) => ("degraded", "rate_headroom"),
        ConnectionTrouble::Degraded(DegradeReason::ContractDrift) => ("degraded", "contract_drift"),
        ConnectionTrouble::Suspended(SuspendReason::AuthorizationFailed) => {
            ("suspended", "authorization_failed")
        }
        ConnectionTrouble::Suspended(SuspendReason::CredentialExpired) => {
            ("suspended", "credential_expired")
        }
        ConnectionTrouble::Suspended(SuspendReason::RefreshFailed) => {
            ("suspended", "refresh_failed")
        }
        ConnectionTrouble::Suspended(SuspendReason::CheckFailed) => ("suspended", "check_failed"),
        ConnectionTrouble::Suspended(SuspendReason::LeaseExpired) => ("suspended", "lease_expired"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Whatever troubles arrive, in any order and repeated, possibly across restarts: the first
    /// run journals exactly the restriction and its alert; every state change obeys rules 59 and
    /// 60 and the model's next state; each `(state, reason)` is journaled at most once per stay in
    /// that state; nothing degrades after a suspension; and the account stays closing only. The
    /// oracle is a separate model over the reported troubles, never the executor's own state.
    #[test]
    fn every_sequence_of_troubles_restricts_once_then_records_the_model_s_changes(
        reported in proptest::collection::vec((trouble(), any::<bool>()), 1..14),
    ) {
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
        let instruments = FixedInstruments;
        let config = config();
        let ports = ports(&ids, &mandates, &instruments, &config);
        let mut shell = established(&ports);
        let mut model_state = "active";
        let mut model_reasons: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        let mut restricted = false;
        for (step, (reported, restart)) in reported.iter().enumerate() {
            if *restart {
                let (fresh, _) = shell.restart(&ports);
                shell = fresh;
            }
            let ran = shell.run(Input::Connection(*reported), &ports);
            let (to, reason) = modelled(*reported);
            if !restricted {
                prop_assert_eq!(restrictions(&ran), vec![connection_restriction()], "step {}", step);
                prop_assert!(changes(&ran).is_empty(), "the halt comes first");
                prop_assert_eq!(ran.notifications.clone(), vec!["connection_unavailable"]);
                restricted = true;
                continue;
            }
            prop_assert!(restrictions(&ran).is_empty(), "step {}", step);
            let held = model_state == "suspended" && to == "degraded";
            let repeat = model_state == to && model_reasons.contains(reason);
            if held || repeat {
                prop_assert!(ran.drafts.is_empty(), "step {}: {:?}", step, ran.draft_types());
                continue;
            }
            prop_assert_eq!(changes(&ran), vec![change(model_state, to, reason)], "step {}", step);
            if model_state != to {
                model_reasons.clear();
            }
            model_state = to;
            model_reasons.insert(reason);
        }
        prop_assert_eq!(shell.state.account_state(), AccountState::ClosingOnly);
    }
}
