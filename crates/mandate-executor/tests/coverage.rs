//! Cases that pin behavior the first suite left open, found by running the mutation gate against
//! the implementation ([task brief](../../../docs/project/tasks/M6-K-executor-and-connector.md),
//! backlog E7-2, E7-3).
//!
//! Each case states one clause of the trading-domain or journal spec and computes its expectation
//! by hand, like `hand.rs`; it exists because a mutant of the implementation survived every case
//! that suite already had, which means the clause was not yet evidence-backed.

mod common;

use common::{
    ACCOUNT_STREAM, AGENT_STREAM, CLOCK_STREAM, CONTROL_STREAM, FixedInstruments, FixedMandate,
    OTHER_AGENT, Shell, TestIds, agent, broker_account, broker_fill, broker_order, broker_position,
    broker_reject, clock, config, copied, event, handoff, instrument, object, opening, ports, qty,
    risk_exit, signed_qty, snapshot, stream_opened, text, usd, with_clock,
};
use mandate_accounting::Side;
use mandate_canon::Value;
use mandate_executor::{
    AccountState, BrokerOutcome, BrokerRequest, BrokerUnknown, BrokerUpdate, ClientOrderId,
    Command, DifferenceKind, EventId, ExecutorError, ExecutorState, Initiator, Input, IntentId,
    IntentOutcome, KillScope, Mode, OrderState, Ports, ReconcileReason, ReconciliationVerdict, Seq,
    TimeInForce, fold, reconcile,
};

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const CPHC: &str = FixedInstruments::THIN_EQUITY;
const BTC: &str = FixedInstruments::CRYPTO;
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";
const OTHER_INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ1";

/// A stream opened and a process started on it.
fn fresh(ports: &Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    shell.restart_ready(ports)
}

/// The same, with a position of ten `AAPL` bought at 150 folded before the start.
fn holding(ports: &Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            2,
            "FillApplied",
            with_clock(
                &[
                    ("fill_id", text("f-0")),
                    ("instrument", text(AAPL)),
                    ("side", text("buy")),
                    ("qty_gross", text("10")),
                    ("price", text("150")),
                ],
                0,
            ),
        ))
        .expect("the position folds");
    shell.restart_ready(ports)
}

/// Submits one intent and answers the client order id it went out under.
fn submitted(shell: &mut Shell, ports: &Ports<'_>, intent: &str, who: &str, name: &str) -> String {
    let ran = shell.run(handoff(intent, who, opening(name, "10", "150")), ports);
    ran.submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("the opening is sent")
}

/// Submits one intent and acknowledges it, answering its client order id.
fn accepted(shell: &mut Shell, ports: &Ports<'_>, intent: &str, who: &str, name: &str) -> String {
    let id = submitted(shell, ports, intent, who, name);
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Submitted(broker_order(
            "b-1",
            Some(&id),
            name,
            Side::Buy,
            "10",
            "0",
            "new",
        )))),
        ports,
    );
    id
}

fn key(id: &str) -> ClientOrderId {
    ClientOrderId::parse(id).expect("an id this platform derived")
}

fn gate_field(ran: &common::Ran, field: &str) -> Option<String> {
    ran.draft("GateDecided")
        .and_then(|draft| draft.payload.get(field))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn next_seq(shell: &Shell) -> u64 {
    shell.head().0.saturating_add(1)
}

#[test]
fn the_workspaces_agent_control_and_clock_streams_are_followed() {
    let mut state = ExecutorState::new(common::scope());
    for stream in [AGENT_STREAM, CONTROL_STREAM, CLOCK_STREAM] {
        fold(&mut state, &event(stream, 1, "IntentProposed", object(&[])))
            .unwrap_or_else(|e| panic!("{stream} is followed (journal §2): {e}"));
        assert_eq!(
            state.head(stream),
            Some(Seq(1)),
            "{stream}'s position is folded"
        );
    }
    let error = fold(
        &mut state,
        &event("agent:ws2:agent-a", 1, "IntentProposed", object(&[])),
    )
    .expect_err("another workspace's agent stream is not this account's to follow");
    assert_eq!(error.code(), "foreign_stream");
}

#[test]
fn every_uninterpreted_event_names_the_story_that_owns_it() {
    for (kind, story) in [
        ("RelatedAccountsCoordination", "E7-5"),
        ("UniverseChanged", "E17-3"),
        ("RiskLimitTriggered", "E6-4"),
        ("SomethingNobodyWrote", "E7-2"),
    ] {
        let mut state = ExecutorState::new(common::scope());
        fold(&mut state, &stream_opened()).expect("folds");
        let error = fold(
            &mut state,
            &event(ACCOUNT_STREAM, 2, kind, with_clock(&[], 1)),
        )
        .expect_err("not interpreted here");
        assert!(
            format!("{error}").contains(story),
            "{kind} names {story} (DEC-85): {error}"
        );
    }
}

#[test]
#[ignore = "pending E7-3"]
fn a_split_multiplies_the_position_and_ends_the_pending_action() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports);
    let prepared = with_clock(&[("instrument", text(AAPL)), ("action", text("split"))], 5);
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            next_seq(&shell),
            "CorporateActionPrepared",
            prepared,
        ))
        .expect("the prepared split folds");
    let applied = with_clock(&[("instrument", text(AAPL)), ("ratio", text("4"))], 6);
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            next_seq(&shell),
            "CorporateActionApplied",
            applied,
        ))
        .expect("the applied split folds");

    assert_eq!(
        shell.state.positions().get(&instrument(AAPL)).copied(),
        Some(signed_qty("40")),
        "ten shares split four for one are forty (§8.5)"
    );
    let mut taken = snapshot(shell.head().0, ReconcileReason::Scheduled);
    taken.positions = vec![broker_position(AAPL, "41")];
    let run = reconcile(&shell.state, &taken, &ports).expect("runs");
    assert_eq!(
        run.verdict,
        ReconciliationVerdict::Mismatch,
        "once applied, the action is no longer pending and the position is exact again (§11)"
    );
}

#[test]
fn a_reconciliation_run_advances_the_checkpoint_and_the_covered_head() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    shell.run(
        Input::BrokerSnapshot(snapshot(shell.head().0, ReconcileReason::Scheduled)),
        &ports,
    );
    assert_eq!(shell.state.reconciled_through(), Some(shell.head()));
    assert_eq!(
        shell.state.checkpoint().map(|cursor| cursor.0.as_str()),
        Some("cursor-1"),
        "the run advances the activities checkpoint to the snapshot's cursor (§11)"
    );
}

#[test]
fn a_reject_that_is_not_a_403_breaks_the_run_of_403s() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let reject =
        |status| Input::BrokerUpdate(BrokerUpdate::Reject(broker_reject(None, status, "x")));
    shell.run(reject(403), &ports);
    shell.run(reject(403), &ports);
    shell.run(reject(422), &ports);
    assert_eq!(
        shell.state.consecutive_403s(),
        0,
        "§7.3 counts consecutive 403s"
    );
    shell.run(reject(403), &ports);
    assert_eq!(shell.state.consecutive_403s(), 1);
    assert_eq!(shell.state.account_state(), AccountState::Active);
}

#[test]
fn a_restricted_message_alone_sets_closing_only() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Reject(broker_reject(
            None,
            422,
            "trading is restricted on this account",
        ))),
        &ports,
    );
    assert_eq!(shell.state.account_state(), AccountState::ClosingOnly);
}

#[test]
fn each_account_flag_alone_blocks_the_account() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let cases = [
        mandate_executor::BrokerAccount {
            status: "ACCOUNT_CLOSED".to_owned(),
            ..broker_account()
        },
        mandate_executor::BrokerAccount {
            trading_blocked: true,
            ..broker_account()
        },
        mandate_executor::BrokerAccount {
            account_blocked: true,
            ..broker_account()
        },
        mandate_executor::BrokerAccount {
            trade_suspended_by_user: true,
            ..broker_account()
        },
    ];
    for account in cases {
        let mut shell = fresh(&ports);
        shell.run(
            Input::BrokerUpdate(BrokerUpdate::Account(account.clone())),
            &ports,
        );
        assert_eq!(
            shell.state.account_state(),
            AccountState::Blocked,
            "§7.3's first row, any one signal: {account:?}"
        );
    }
    let mut shell = fresh(&ports);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Account(broker_account())),
        &ports,
    );
    assert_eq!(shell.state.account_state(), AccountState::Active);
}

#[test]
fn an_observed_account_is_carried_field_for_field() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Account(mandate_executor::BrokerAccount {
            status: "ACCOUNT_CLOSED".to_owned(),
            multiplier: 4,
            non_marginable_buying_power: usd("123"),
            accrued_fees: usd("4"),
            ..broker_account()
        })),
        &ports,
    );
    let observed = shell.state.observed_account().expect("observed");
    assert_eq!(observed.state, AccountState::Blocked);
    assert_eq!(observed.multiplier, 4);
    assert_eq!(observed.non_marginable_buying_power, usd("123"));
    assert_eq!(observed.accrued_fees, usd("4"));
    let mut shell = fresh(&ports);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Account(broker_account())),
        &ports,
    );
    assert_eq!(
        shell.state.observed_account().map(|account| account.state),
        Some(AccountState::Active)
    );
}

#[test]
fn a_first_pass_deny_is_recorded_with_its_reason_and_ends_the_intent() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            next_seq(&shell),
            "AccountRestrictionChanged",
            with_clock(&[("restriction", text("blocked"))], 1),
        ))
        .expect("folds");
    let ran = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
        &ports,
    );
    assert_eq!(gate_field(&ran, "verdict").as_deref(), Some("deny"));
    assert_eq!(
        gate_field(&ran, "reason_code").as_deref(),
        Some("account_trading_blocked"),
        "the first failing check's reason is journaled (journal §9)"
    );
    assert_eq!(
        shell
            .state
            .intent(&IntentId(EventId(INTENT.to_owned())))
            .map(|record| record.outcome),
        Some(IntentOutcome::Denied)
    );
}

#[test]
fn an_opening_outside_the_working_universe_is_denied() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let ran = shell.run(
        handoff(INTENT, common::AGENT, opening(CPHC, "1", "20")),
        &ports,
    );
    assert_eq!(
        gate_field(&ran, "reason_code").as_deref(),
        Some("instrument_not_in_universe")
    );
    assert!(ran.submissions().is_empty());
}

#[test]
fn a_sell_may_take_the_position_and_no_more() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports);
    let over = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "11", "150")),
        &ports,
    );
    assert_eq!(
        gate_field(&over, "reason_code").as_deref(),
        Some("sell_exceeds_available"),
        "eleven from ten crosses zero (§5.3 rules 3 and 4)"
    );
    let first = shell.run(
        handoff(OTHER_INTENT, common::AGENT, risk_exit(AAPL, "6", "150")),
        &ports,
    );
    assert_eq!(first.submissions().len(), 1, "six of ten is available");
    let second = shell.run(
        handoff(
            "01JABCDEFGHJKMNPQRSTVWXYZ2",
            common::AGENT,
            risk_exit(AAPL, "6", "150"),
        ),
        &ports,
    );
    assert_eq!(
        gate_field(&second, "reason_code").as_deref(),
        Some("sell_exceeds_available"),
        "six more is not: the open sell already takes six of the ten"
    );
}

#[test]
fn a_filled_sell_no_longer_holds_quantity_back() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports);
    let first = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "4", "150")),
        &ports,
    );
    let id = first
        .submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("sent");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Sell,
            "4",
            "0",
            "canceled",
        ))),
        &ports,
    );
    let second = shell.run(
        handoff(OTHER_INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );
    assert_eq!(
        second.submissions().len(),
        1,
        "a cancelled sell is terminal and takes nothing from the ten"
    );
}

#[test]
fn a_confirmed_absent_sell_is_resubmitted_against_its_own_quantity() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports);
    let sent = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );
    let id = sent
        .submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("sent");
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    let absent = || {
        Input::Broker(Ok(BrokerOutcome::Absent {
            client_order_id: id.clone(),
        }))
    };
    shell.run(absent(), &ports);
    shell.run(Input::Tick(clock(8)), &ports);
    shell.run(absent(), &ports);
    shell.run(Input::Tick(clock(16)), &ports);
    let third = shell.run(absent(), &ports);
    assert_eq!(
        third.submissions().len(),
        1,
        "the re-check does not count the order against itself: all ten are still available"
    );
}

#[test]
fn closing_only_denies_an_opening_and_lets_an_exit_through() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports);
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            next_seq(&shell),
            "AccountRestrictionChanged",
            with_clock(&[("restriction", text("closing_only"))], 1),
        ))
        .expect("folds");
    let opening_ran = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "1", "150")),
        &ports,
    );
    assert_eq!(
        gate_field(&opening_ran, "reason_code").as_deref(),
        Some("account_restricted")
    );
    let exit = shell.run(
        handoff(OTHER_INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );
    assert_eq!(
        exit.submissions().len(),
        1,
        "reducing risk is never denied (rule 13)"
    );
}

#[test]
fn exits_only_denies_an_opening_and_holds_no_exit() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports);
    shell
        .fold_one(&copied(
            ACCOUNT_STREAM,
            next_seq(&shell),
            "AgentModeApplied",
            with_clock(
                &[("agent", text(common::AGENT)), ("to", text("exits_only"))],
                1,
            ),
            &EventId(format!("{AGENT_STREAM}-1")),
        ))
        .expect("folds");
    let opening_ran = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, "1", "150")),
        &ports,
    );
    assert_eq!(
        gate_field(&opening_ran, "reason_code").as_deref(),
        Some("agent_exits_only")
    );
    let exit = shell.run(
        handoff(OTHER_INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );
    assert_eq!(
        exit.submissions().len(),
        1,
        "exits_only allows exits (§7.4)"
    );
}

#[test]
#[ignore = "pending E7-2"]
fn a_paused_agents_exit_is_held_not_denied() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports);
    shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        }),
        &ports,
    );
    let ran = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );
    assert_eq!(gate_field(&ran, "verdict").as_deref(), Some("hold"));
    assert_eq!(
        gate_field(&ran, "reason_code").as_deref(),
        Some("agent_paused")
    );
    assert!(ran.submissions().is_empty());
}

#[test]
fn a_held_exit_is_released_at_the_first_tick_its_hold_has_cleared() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports);
    let unknown = submitted(&mut shell, &ports, OTHER_INTENT, common::AGENT, AAPL);
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    let held = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );
    assert_eq!(
        gate_field(&held, "reason_code").as_deref(),
        Some("unknown_order_in_flight")
    );
    let quiet = shell.run(Input::Tick(clock(3)), &ports);
    assert!(
        quiet.submissions().is_empty(),
        "still held while the Unknown lasts"
    );
    assert!(
        quiet.draft("GateDecided").is_none(),
        "and a hold is not journaled again at every tick"
    );
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Order(broker_order(
            "b-1",
            Some(&unknown),
            AAPL,
            Side::Buy,
            "10",
            "0",
            "accepted",
        )))),
        &ports,
    );
    let released = shell.run(Input::Tick(clock(4)), &ports);
    assert_eq!(
        released.submissions().len(),
        1,
        "the Unknown resolved, so the exit goes"
    );
    assert_eq!(gate_field(&released, "verdict").as_deref(), Some("allow"));
}

#[test]
fn a_held_intent_past_its_age_is_abandoned_at_a_tick() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports);
    submitted(&mut shell, &ports, OTHER_INTENT, common::AGENT, AAPL);
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );
    let ran = shell.run(Input::Tick(clock(121)), &ports);
    assert!(
        ran.draft_types().contains(&"OrderAbandoned"),
        "121 seconds is past max_intent_age: {:?}",
        ran.draft_types()
    );
}

#[test]
fn the_age_bound_is_inclusive() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    for (last, resubmits) in [(120_i64, true), (121, false)] {
        let mut shell = fresh(&ports);
        let id = submitted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
        shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
        let absent = || {
            Input::Broker(Ok(BrokerOutcome::Absent {
                client_order_id: id.clone(),
            }))
        };
        shell.run(absent(), &ports);
        shell.run(Input::Tick(clock(8)), &ports);
        shell.run(absent(), &ports);
        shell.run(Input::Tick(clock(last)), &ports);
        let third = shell.run(absent(), &ports);
        assert_eq!(
            third.submissions().len() == 1,
            resubmits,
            "an intent exactly max_intent_age old may still be sent; one second more may not \
             (at {last})"
        );
    }
}

#[test]
fn absences_are_counted_afresh_each_time_an_order_goes_unknown() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = submitted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Absent {
            client_order_id: id.clone(),
        })),
        &ports,
    );
    assert_eq!(
        shell.state.order(&key(&id)).map(|o| o.absent_lookups),
        Some(1)
    );
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "0",
            "accepted",
        )))),
        &ports,
    );
    shell.run(
        Input::BrokerSnapshot(snapshot(shell.head().0, ReconcileReason::Scheduled)),
        &ports,
    );
    let order = shell.state.order(&key(&id)).expect("folded");
    assert_eq!(
        order.state,
        OrderState::Unknown,
        "missing from the open orders again"
    );
    assert_eq!(
        (order.absent_lookups, order.first_absence_at),
        (0, None),
        "an absence from an earlier episode never counts toward confirming this one (§5.7)"
    );
}

#[test]
fn an_unknown_order_is_looked_up_again_only_after_the_spacing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = submitted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    let lookups = |ran: &common::Ran| {
        ran.requests
            .iter()
            .filter(|request| matches!(request, BrokerRequest::GetOrderByClientId(_)))
            .count()
    };
    assert_eq!(
        lookups(&shell.run(Input::Tick(clock(7)), &ports)),
        0,
        "three lookups over fifteen seconds are eight seconds apart, rounded up"
    );
    assert_eq!(lookups(&shell.run(Input::Tick(clock(8)), &ports)), 1);
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Absent {
            client_order_id: id,
        })),
        &ports,
    );
    assert_eq!(lookups(&shell.run(Input::Tick(clock(15)), &ports)), 0);
    assert_eq!(lookups(&shell.run(Input::Tick(clock(16)), &ports)), 1);
}

#[test]
fn a_replacement_is_a_new_order_under_its_own_id() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = accepted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-1",
            Some(&id),
            "4",
            "150",
        ))),
        &ports,
    );
    let mut replaced = broker_order("b-1", Some(&id), AAPL, Side::Buy, "10", "4", "replaced");
    replaced.replaced_by_broker_order_id = Some("b-2".to_owned());
    shell.run(Input::BrokerUpdate(BrokerUpdate::Order(replaced)), &ports);
    let linked = shell
        .state
        .order(&key(&id))
        .and_then(|order| order.replaced_by.clone())
        .expect("the old order names the new one");
    let new = shell.state.order(&linked).expect("the new order is folded");
    assert_eq!(new.client_order_id, linked, "under its own derived id");
    assert_eq!(
        new.qty,
        qty("6"),
        "for what the original had left, 10 less the 4 filled, so the pair never absorbs more \
         than the gate approved (§5.7, AGENTS.md rule 1)"
    );
    assert_eq!(new.filled_qty, qty("0"), "a new order has filled nothing");
    assert_eq!(new.replaced_by, None, "and has not itself been replaced");
    assert_eq!(new.state, OrderState::Accepted);

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-2",
            Some(linked.as_str()),
            "10",
            "150",
        ))),
        &ports,
    );
    assert!(
        ran.draft_types().contains(&"ExternalActivityIngested")
            && ran
                .draft("FillApplied")
                .is_some_and(|draft| draft.payload.get("client_order_id").is_none()),
        "ten more shares on the new id would take the pair to 14 of an approved 10, so the fill \
         cannot be the new order's and is ingested as external activity (§7.1): {:?}",
        ran.draft_types()
    );
    assert_eq!(
        shell.state.order(&linked).map(|order| order.filled_qty),
        Some(qty("0"))
    );
    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-3",
            Some(linked.as_str()),
            "6",
            "150",
        ))),
        &ports,
    );
    assert!(
        !ran.draft_types().contains(&"ExternalActivityIngested"),
        "while the six left are the new order's own: {:?}",
        ran.draft_types()
    );
    assert_eq!(
        shell.state.order(&linked).map(|order| order.state),
        Some(OrderState::Filled)
    );
}

#[test]
#[ignore = "pending E7-4"]
fn a_kill_switch_cancel_is_unconfirmed_until_the_broker_confirms_it() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = accepted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        }),
        &ports,
    );
    let pending = shell.state.order(&key(&id)).expect("folded");
    assert_eq!(pending.state, OrderState::PendingCancel);
    assert!(
        pending.cancel_unconfirmed,
        "an accepted cancel request is not a confirmation"
    );
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-1",
            Some(&id),
            "4",
            "150",
        ))),
        &ports,
    );
    assert_eq!(
        shell.state.order(&key(&id)).map(|order| order.state),
        Some(OrderState::PendingCancel),
        "a fill during a pending cancel updates the filled quantity without leaving it (§5.7)"
    );
    shell.run(
        Input::Broker(Ok(BrokerOutcome::CancelAccepted {
            client_order_id: id.clone(),
        })),
        &ports,
    );
    let cancelled = shell.state.order(&key(&id)).expect("folded");
    assert_eq!(cancelled.state, OrderState::Canceled);
    assert!(!cancelled.cancel_unconfirmed);
    assert!(!shell.state.reservations().contains_key(&key(&id)));
}

#[test]
#[ignore = "pending E7-4"]
fn a_kill_switch_leaves_another_agents_working_order_alone() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let theirs = accepted(&mut shell, &ports, OTHER_INTENT, OTHER_AGENT, AAPL);
    let ran = shell.run(
        Input::Command(Command::KillSwitch {
            scope: KillScope::Agent(agent(common::AGENT)),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        }),
        &ports,
    );
    assert!(
        !ran.requests.iter().any(|request| matches!(
            request,
            BrokerRequest::Cancel { client_order_id } if client_order_id.as_str() == theirs
        )),
        "an agent's kill switch reaches only its own orders (§5.5): {:?}",
        ran.requests
    );
    assert_eq!(
        shell.state.order(&key(&theirs)).map(|order| order.state),
        Some(OrderState::Accepted)
    );
}

#[test]
fn fills_move_an_order_through_partially_filled_to_filled() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = accepted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-1",
            Some(&id),
            "4",
            "150",
        ))),
        &ports,
    );
    assert_eq!(
        shell.state.order(&key(&id)).map(|order| order.state),
        Some(OrderState::PartiallyFilled)
    );
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-2",
            Some(&id),
            "6",
            "150",
        ))),
        &ports,
    );
    let order = shell.state.order(&key(&id)).expect("folded");
    assert_eq!(order.state, OrderState::Filled);
    assert_eq!(order.filled_qty, qty("10"));
    assert!(!shell.state.reservations().contains_key(&key(&id)));
}

#[test]
fn a_fill_that_cannot_be_the_orders_is_applied_unattributed() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = accepted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    let unattributed = |ran: &common::Ran| {
        ran.draft("FillApplied")
            .is_some_and(|draft| draft.payload.get("client_order_id").is_none())
            && ran.draft_types().contains(&"ExternalActivityIngested")
    };
    let other_instrument = mandate_executor::BrokerFill {
        instrument: instrument(CPHC),
        ..broker_fill("f-1", Some(&id), "1", "20")
    };
    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(other_instrument)),
        &ports,
    );
    assert!(
        unattributed(&ran),
        "another instrument: {:?}",
        ran.draft_types()
    );
    let restricted = ran
        .draft("AgentModeApplied")
        .expect("external activity restricts the agents (§7.1)");
    for (field, expected) in [
        ("agent", "*"),
        ("to", "exits_only"),
        ("restriction", "reconciliation:external_activity"),
    ] {
        assert_eq!(
            restricted.payload.get(field).and_then(Value::as_str),
            Some(expected),
            "every agent on the account goes `exits_only` under the restriction only an owner \
             acknowledgment of the external activity lifts ({field})"
        );
    }
    assert_eq!(
        shell.state.effective_mode(&agent(OTHER_AGENT)),
        Mode::ExitsOnly,
        "including an agent that placed no order"
    );
    let ingested = ran
        .draft("ExternalActivityIngested")
        .map(|draft| draft.event_id.clone());
    let alerts: Vec<_> = ran
        .effects
        .iter()
        .filter_map(|effect| match effect {
            mandate_executor::Effect::Notify(reference) => Some(reference),
            _ => None,
        })
        .collect();
    assert_eq!(alerts.len(), 1, "the owner is alerted once");
    assert_eq!(
        (Some(alerts[0].subject_event.clone()), alerts[0].message_key),
        (ingested, "external_activity"),
        "by the ingested event's opaque id and a message key, nothing more (AGENTS.md rule 6)"
    );
    let other_side = mandate_executor::BrokerFill {
        side: Side::Sell,
        ..broker_fill("f-2", Some(&id), "1", "150")
    };
    let ran = shell.run(Input::BrokerUpdate(BrokerUpdate::Fill(other_side)), &ports);
    assert!(
        unattributed(&ran),
        "the other side: {:?}",
        ran.draft_types()
    );
    let too_much = broker_fill("f-3", Some(&id), "11", "150");
    let ran = shell.run(Input::BrokerUpdate(BrokerUpdate::Fill(too_much)), &ports);
    assert!(
        unattributed(&ran),
        "more than the order: {:?}",
        ran.draft_types()
    );
    let exact = broker_fill("f-4", Some(&id), "10", "150");
    let ran = shell.run(Input::BrokerUpdate(BrokerUpdate::Fill(exact)), &ports);
    assert!(
        !unattributed(&ran),
        "exactly what is left is the order's own fill"
    );
    assert_eq!(
        shell.state.order(&key(&id)).map(|order| order.filled_qty),
        Some(qty("10"))
    );
}

#[test]
fn an_update_reporting_more_filled_than_applied_asks_for_a_reconciliation() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = accepted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    let reconciles = |ran: &common::Ran| {
        ran.requests
            .iter()
            .any(|request| matches!(request, BrokerRequest::ListOpenOrders))
    };
    let same = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "0",
            "accepted",
        ))),
        &ports,
    );
    assert!(!reconciles(&same), "nothing filled that the journal lacks");
    let ahead = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "4",
            "partially_filled",
        ))),
        &ports,
    );
    assert!(
        reconciles(&ahead),
        "a cumulative quantity ahead of the fills applied triggers reconciliation (§5.7)"
    );
}

#[test]
fn a_crypto_opening_is_gtc_and_an_equity_opening_is_day() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let crypto = shell.run(
        handoff(INTENT, common::AGENT, opening(BTC, "0.5", "60000")),
        &ports,
    );
    assert_eq!(
        crypto.submissions().first().map(|order| order.tif),
        Some(TimeInForce::Gtc),
        "crypto takes gtc or ioc, never day (§5.2)"
    );
    let equity = shell.run(
        handoff(OTHER_INTENT, common::AGENT, opening(AAPL, "1", "150")),
        &ports,
    );
    assert_eq!(
        equity.submissions().first().map(|order| order.tif),
        Some(TimeInForce::Day)
    );
}

#[test]
fn a_received_intent_resumes_after_the_startup_reconciliation_and_not_before() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    shell
        .step_crashing(
            handoff(INTENT, common::AGENT, opening(AAPL, "10", "150")),
            &ports,
            common::CrashPoint::IntentReceivedBeforeGate,
        )
        .expect("the step does not refuse");
    let (mut shell, started) = shell.restart(&ports);
    assert!(started.submissions().is_empty(), "Started never submits");
    let scheduled = shell.run(
        Input::BrokerSnapshot(snapshot(shell.head().0, ReconcileReason::Scheduled)),
        &ports,
    );
    assert!(
        scheduled.submissions().is_empty(),
        "only the startup reconciliation releases a resumed intent"
    );
    let startup = shell.run(
        Input::BrokerSnapshot(snapshot(shell.head().0, ReconcileReason::Startup)),
        &ports,
    );
    assert_eq!(startup.submissions().len(), 1);
}

#[test]
fn a_reconciliation_leaves_an_agreeing_order_and_an_unknown_one_alone() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = accepted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    let mut taken = snapshot(shell.head().0, ReconcileReason::Scheduled);
    taken.open_orders = vec![broker_order(
        "b-1",
        Some(&id),
        AAPL,
        Side::Buy,
        "10",
        "0",
        "new",
    )];
    let run = reconcile(&shell.state, &taken, &ports).expect("runs");
    assert!(run.differences.is_empty(), "{:?}", run.differences);
    assert_eq!(run.verdict, ReconciliationVerdict::Clean);

    let mut shell = fresh(&ports);
    submitted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    let taken = snapshot(shell.head().0, ReconcileReason::Scheduled);
    let run = reconcile(&shell.state, &taken, &ports).expect("runs");
    assert!(
        run.differences.is_empty(),
        "an Unknown order is already being resolved by query: {:?}",
        run.differences
    );
}

#[test]
fn cash_within_the_band_agrees_and_beyond_it_differs() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Account(broker_account())),
        &ports,
    );
    let id = accepted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-1",
            Some(&id),
            "10",
            "150",
        ))),
        &ports,
    );
    for (cash, differs) in [
        ("18500", false),
        ("18485", false),
        ("18484.99", true),
        ("18516", true),
    ] {
        let mut taken = snapshot(shell.head().0, ReconcileReason::Scheduled);
        taken.account.cash = usd(cash);
        taken.positions = vec![broker_position(AAPL, "10")];
        let run = reconcile(&shell.state, &taken, &ports).expect("runs");
        assert_eq!(
            run.differences
                .iter()
                .any(|d| d.kind == DifferenceKind::Cash),
            differs,
            "the model is 20000 − 1500 = 18500 and the band 0.01 × 1500 = 15 (§11): {cash}"
        );
    }
}

#[test]
fn a_position_mismatch_pauses_only_the_agents_holding_the_instrument() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    accepted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    accepted(&mut shell, &ports, OTHER_INTENT, OTHER_AGENT, CPHC);
    let mut taken = snapshot(shell.head().0, ReconcileReason::Scheduled);
    taken.open_orders = shell
        .state
        .orders()
        .values()
        .map(|order| {
            broker_order(
                "b-1",
                Some(order.client_order_id.as_str()),
                order.instrument.as_str(),
                Side::Buy,
                "10",
                "0",
                "new",
            )
        })
        .collect();
    taken.positions = vec![broker_position(AAPL, "7")];
    let run = reconcile(&shell.state, &taken, &ports).expect("runs");
    let paused: Vec<String> = run
        .effects
        .iter()
        .filter_map(|effect| match effect {
            mandate_executor::Effect::Journal(draft) if draft.event_type == "AgentModeApplied" => {
                draft
                    .payload
                    .get("agent")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        paused,
        vec![common::AGENT.to_owned()],
        "§11 pauses every agent holding that instrument, and no other"
    );
}

#[test]
fn an_acknowledgment_lifts_its_own_subject_and_no_other() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let mut bad = snapshot(shell.head().0, ReconcileReason::Startup);
    bad.positions = vec![broker_position(AAPL, "7"), broker_position(CPHC, "3")];
    shell.run(Input::BrokerSnapshot(bad), &ports);
    let ack = |shell: &mut Shell, subject: &str, n: u64| {
        let event = copied(
            ACCOUNT_STREAM,
            next_seq(shell),
            "OwnerAcknowledged",
            with_clock(
                &[
                    ("subject", text(subject)),
                    ("user", text("user-1")),
                    ("step_up", text("assertion-1")),
                ],
                100,
            ),
            &EventId(format!("{CONTROL_STREAM}-{n}")),
        );
        shell.fold_one(&event).expect("the acknowledgment folds");
    };
    ack(&mut shell, AAPL, 1);
    assert!(!shell.state.mismatched().contains(&instrument(AAPL)));
    assert!(shell.state.mismatched().contains(&instrument(CPHC)));
    assert_eq!(
        shell.state.effective_mode(&agent(common::AGENT)),
        Mode::Paused,
        "CPHC's mismatch still pauses"
    );
    ack(&mut shell, CPHC, 2);
    assert_eq!(
        shell.state.effective_mode(&agent(common::AGENT)),
        Mode::Normal,
        "with both acknowledged, nothing pauses"
    );
}

#[test]
fn the_state_answers_what_the_fold_carries() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = submitted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    assert!(
        shell
            .state
            .intent(&IntentId(EventId(INTENT.to_owned())))
            .is_some()
    );
    let unacknowledged: Vec<&str> = shell
        .state
        .unacknowledged()
        .iter()
        .map(|order| order.client_order_id.as_str())
        .collect();
    assert_eq!(unacknowledged, vec![id.as_str()]);
    let placed = with_clock(
        &[
            ("instrument", text(AAPL)),
            ("action", text("placed")),
            ("orders", text("md-oco-1")),
            ("qty", text("10")),
        ],
        1,
    );
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            next_seq(&shell),
            "ProtectionChanged",
            placed,
        ))
        .expect("folds");
    let protection = shell
        .state
        .protection(&instrument(AAPL))
        .expect("the protection accessor answers")
        .expect("placed");
    assert_eq!(protection.covered_qty, qty("10"));
    let origin = EventId(format!("{AGENT_STREAM}-3"));
    let mode = copied(
        ACCOUNT_STREAM,
        next_seq(&shell),
        "AgentModeApplied",
        with_clock(&[("agent", text(common::AGENT)), ("to", text("normal"))], 2),
        &origin,
    );
    let copied_id = mode.event_id.clone();
    shell.fold_one(&mode).expect("folds");
    assert_eq!(
        shell
            .state
            .copied_origin(&copied_id)
            .expect("the copied-origin accessor answers"),
        Some(&origin)
    );
    assert_eq!(
        shell
            .state
            .copied_origin(&EventId("nothing".to_owned()))
            .expect("the copied-origin accessor answers"),
        None
    );
}

#[test]
fn an_unprotected_interval_ends_for_its_own_instrument() {
    let mut state = ExecutorState::new(common::scope());
    fold(&mut state, &stream_opened()).expect("folds");
    let action = |seq: u64, name: &str, what: &str, at: i64| {
        event(
            ACCOUNT_STREAM,
            seq,
            "ProtectionChanged",
            with_clock(&[("instrument", text(name)), ("action", text(what))], at),
        )
    };
    fold(&mut state, &action(2, AAPL, "unprotected_start", 1)).expect("folds");
    fold(&mut state, &action(3, CPHC, "unprotected_start", 2)).expect("folds");
    fold(&mut state, &action(4, AAPL, "unprotected_end", 3)).expect("folds");
    let open: Vec<&str> = state
        .unprotected_intervals()
        .iter()
        .filter(|interval| interval.ended_at.is_none())
        .map(|interval| interval.instrument.as_str())
        .collect();
    assert_eq!(
        open,
        vec![CPHC],
        "ending AAPL's interval leaves CPHC's open, though CPHC's opened later"
    );
}

#[test]
fn an_order_adopted_as_unknown_is_queried_and_one_adopted_as_known_is_not() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = accepted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    let queried = |run: &mandate_executor::Reconciliation| {
        run.effects.iter().any(|effect| {
            matches!(
                effect,
                mandate_executor::Effect::Broker(BrokerRequest::GetOrderByClientId(queried))
                    if queried.as_str() == id
            )
        })
    };
    let missing = snapshot(shell.head().0, ReconcileReason::Scheduled);
    let run = reconcile(&shell.state, &missing, &ports).expect("runs");
    assert!(
        queried(&run),
        "an order missing from the open orders is resolved by query on its id (§5.7)"
    );
    let mut partly = snapshot(shell.head().0, ReconcileReason::Scheduled);
    partly.open_orders = vec![broker_order(
        "b-1",
        Some(&id),
        AAPL,
        Side::Buy,
        "10",
        "4",
        "partially_filled",
    )];
    let run = reconcile(&shell.state, &partly, &ports).expect("runs");
    assert!(
        !queried(&run),
        "an order the broker described needs no query"
    );
}

#[test]
fn external_activity_reaches_an_agent_the_executor_has_not_yet_seen() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = accepted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    let mut taken = snapshot(shell.head().0, ReconcileReason::Scheduled);
    taken.open_orders = vec![
        broker_order("b-1", Some(&id), AAPL, Side::Buy, "10", "0", "new"),
        broker_order(
            "b-9",
            Some("manual-web-1"),
            AAPL,
            Side::Buy,
            "5",
            "0",
            "new",
        ),
    ];
    shell.run(Input::BrokerSnapshot(taken), &ports);
    for who in [common::AGENT, "agent-deployed-later"] {
        assert_eq!(
            shell.state.effective_mode(&agent(who)),
            Mode::ExitsOnly,
            "every agent on the account, until the owner acknowledges (§7.1): {who}"
        );
    }
}

#[test]
fn the_client_order_id_grammar_is_the_derivations_and_nothing_else() {
    for accepted in ["md-01JABC", "md-p-e1-h2-o3"] {
        if let Err(error) = ClientOrderId::parse(accepted) {
            panic!("{accepted} is this platform's grammar and parses: {error:?}");
        }
    }
    for refused in ["md-", "xx-01JABC", "md-a b", "md-a_b"] {
        let error = ClientOrderId::parse(refused).expect_err("not this platform's");
        assert_eq!(error.code(), "malformed_client_order_id", "{refused}");
    }
    let longest = format!("md-{}", "a".repeat(125));
    assert!(
        ClientOrderId::parse(&longest).is_ok(),
        "128 bytes is Alpaca's bound"
    );
    let over = format!("md-{}", "a".repeat(126));
    assert!(ClientOrderId::parse(&over).is_err(), "129 bytes is over it");
    let intent = |raw: &str| IntentId(EventId(raw.to_owned()));
    assert_eq!(
        ClientOrderId::for_intent(&intent("01JABC")).map(|id| id.as_str().to_owned()),
        Ok("md-01JABC".to_owned())
    );
    assert!(matches!(
        ClientOrderId::for_intent(&intent("01J-ABC")),
        Err(ExecutorError::MalformedClientOrderId { .. })
    ));
}

#[test]
fn a_blocked_account_holds_an_exit_for_the_broker_and_never_denies_it() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports);
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            next_seq(&shell),
            "AccountRestrictionChanged",
            with_clock(&[("restriction", text("blocked"))], 1),
        ))
        .expect("folds");
    let ran = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );
    assert_eq!(
        gate_field(&ran, "verdict").as_deref(),
        Some("hold"),
        "the broker is one of the four holds rule 13 names; the executor denies no exit on it"
    );
    assert_eq!(gate_field(&ran, "reason_code").as_deref(), Some("broker"));
}

#[test]
fn a_resubmission_the_recheck_holds_waits_in_intent_and_goes_once_released() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = holding(&ports);
    let sent = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(AAPL, "10", "150")),
        &ports,
    );
    let id = sent
        .submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("sent");
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    let paused = copied(
        ACCOUNT_STREAM,
        next_seq(&shell),
        "AgentModeApplied",
        with_clock(
            &[
                ("agent", text(common::AGENT)),
                ("to", text("paused")),
                ("restriction", text("daily_loss")),
            ],
            1,
        ),
        &EventId(format!("{AGENT_STREAM}-5")),
    );
    shell.fold_one(&paused).expect("folds");
    let absent = || {
        Input::Broker(Ok(BrokerOutcome::Absent {
            client_order_id: id.clone(),
        }))
    };
    shell.run(absent(), &ports);
    shell.run(Input::Tick(clock(8)), &ports);
    shell.run(absent(), &ports);
    shell.run(Input::Tick(clock(16)), &ports);
    let third = shell.run(absent(), &ports);
    assert!(third.submissions().is_empty(), "paused holds the exit");
    assert!(
        !third.draft_types().contains(&"OrderAbandoned"),
        "a hold is not a denial, so the exit is not abandoned: {:?}",
        third.draft_types()
    );
    assert_eq!(
        shell.state.order(&key(&id)).map(|order| order.state),
        Some(OrderState::Intent)
    );
    let lifted = copied(
        ACCOUNT_STREAM,
        next_seq(&shell),
        "AgentModeApplied",
        with_clock(
            &[
                ("agent", text(common::AGENT)),
                ("to", text("normal")),
                ("restriction", text("daily_loss")),
            ],
            17,
        ),
        &EventId(format!("{AGENT_STREAM}-6")),
    );
    shell.fold_one(&lifted).expect("folds");
    let released = shell.run(Input::Tick(clock(18)), &ports);
    assert_eq!(
        released
            .submissions()
            .first()
            .map(|order| order.client_order_id.as_str()),
        Some(id.as_str()),
        "the same id, resubmitted once the hold clears"
    );
}

#[test]
fn a_paper_crypto_fill_books_no_simulated_regulatory_fee() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, BTC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = accepted(&mut shell, &ports, INTENT, common::AGENT, BTC);

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(mandate_executor::BrokerFill {
            instrument: instrument(BTC),
            ..broker_fill("f-1", Some(&id), "1", "60000")
        })),
        &ports,
    );

    assert!(
        ran.draft_types().contains(&"FillApplied"),
        "the crypto fill is applied: {:?}",
        ran.draft_types()
    );
    assert!(
        !ran.drafts.iter().any(|d| d.event_type == "FeesCharged"
            && d.payload.get("simulated") == Some(&Value::Bool(true))),
        "SEC, TAF and CAT are equity fees, and crypto fees are the broker's own on paper too, so a \
         paper crypto fill books no simulated fee (§10): {:?}",
        ran.draft_types()
    );
}

#[test]
fn a_late_fill_on_a_terminal_order_moves_the_position_and_the_orders_fill() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = submitted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Buy,
            "10",
            "0",
            "canceled",
        ))),
        &ports,
    );
    assert_eq!(
        shell.state.order(&key(&id)).map(|order| order.state),
        Some(OrderState::Canceled),
        "the order is terminal before its fill arrives"
    );

    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            next_seq(&shell),
            "LateFillApplied",
            with_clock(
                &[
                    ("fill_id", text("f-late")),
                    ("client_order_id", text(&id)),
                    ("instrument", text(AAPL)),
                    ("side", text("buy")),
                    ("qty_gross", text("4")),
                    ("price", text("150")),
                ],
                5,
            ),
        ))
        .expect("a late fill folds (§5.7: a fill after a terminal state is still applied)");

    assert_eq!(
        shell.state.positions().get(&instrument(AAPL)).copied(),
        Some(signed_qty("4")),
        "the four shares the broker filled are held"
    );
    assert_eq!(
        shell.state.order(&key(&id)).map(|order| order.filled_qty),
        Some(qty("4")),
        "and are the order's filled quantity"
    );
}

#[test]
fn absences_faster_than_the_window_leave_the_order_unknown() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = submitted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
    shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);

    let mut ran = Vec::new();
    for at in [1, 2, 3] {
        shell.run(Input::Tick(clock(at)), &ports);
        ran.push(shell.run(
            Input::Broker(Ok(BrokerOutcome::Absent {
                client_order_id: id.clone(),
            })),
            &ports,
        ));
    }

    assert!(
        ran.iter().all(|one| one.submissions().is_empty()),
        "three absences inside three seconds reach `unknown_absent_lookups` but not the \
         15-second `unknown_absent_window_s`, so nothing is resubmitted (§5.7, rule 3)"
    );
    assert!(
        ran.iter()
            .all(|one| !one.draft_types().contains(&"OrderAbandoned")),
        "and nothing is abandoned"
    );
    assert_eq!(
        shell.state.order(&key(&id)).map(|order| order.state),
        Some(OrderState::Unknown),
        "the order stays Unknown until the window has passed too"
    );
    assert_eq!(shell.connector.accepted_for(&id), 1, "still one order");
}

#[test]
fn a_sell_takes_only_its_own_instruments_position() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    for (seq, name, held) in [(2, AAPL, "10"), (3, CPHC, "2")] {
        shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                seq,
                "FillApplied",
                with_clock(
                    &[
                        ("fill_id", text(&format!("f-{name}"))),
                        ("instrument", text(name)),
                        ("side", text("buy")),
                        ("qty_gross", text(held)),
                        ("price", text("20")),
                    ],
                    0,
                ),
            ))
            .expect("the position folds");
    }
    let mut shell = shell.restart(&ports).0;

    let over = shell.run(
        handoff(INTENT, common::AGENT, risk_exit(CPHC, "3", "20")),
        &ports,
    );
    assert_eq!(
        gate_field(&over, "reason_code").as_deref(),
        Some("sell_exceeds_available"),
        "three CPHC from two crosses zero, whatever the ten AAPL beside them (§5.3 rules 3 and 4, \
         AGENTS.md rule 12)"
    );
    assert!(over.submissions().is_empty());

    let within = shell.run(
        handoff(OTHER_INTENT, common::AGENT, risk_exit(CPHC, "2", "20")),
        &ports,
    );
    assert_eq!(
        within.submissions().len(),
        1,
        "the two CPHC held are available to sell"
    );
}

/// The paper fee of a buy fill (§6.2, §10): CAT only, keyed as the journal keys every fee.
#[test]
fn a_paper_buy_books_cat_only_under_the_equities_family_and_its_trade_date() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = accepted(&mut shell, &ports, INTENT, common::AGENT, AAPL);

    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-1",
            Some(&id),
            "10",
            "150",
        ))),
        &ports,
    );

    let fee = ran
        .draft("FeesCharged")
        .expect("a paper equity fill books its simulated fee");
    let field = |name: &str| fee.payload.get(name).and_then(Value::as_str);
    assert_eq!(
        (field("family"), field("day")),
        (Some("equities"), Some("2026-09-22")),
        "the family and the New York trade date the account keys equity fees by (journal spec \
         §6, trading spec §6.2)"
    );
    assert_eq!(
        (field("accrued"), field("cat")),
        (Some("0.0001"), Some("0.0001")),
        "a buy owes CAT alone: 10 x 0.00001"
    );
    assert_eq!(
        (field("sec"), field("taf")),
        (None, None),
        "and no SEC or TAF, which fall on sells"
    );
    assert_eq!(
        field("charged"),
        Some("0"),
        "a simulated fee is accrued and never charged by the broker"
    );
}

/// DEC-87: under `per_order` the TAF cap binds across an order's partial fills.
#[test]
fn the_per_order_taf_cap_binds_across_an_orders_partial_fills() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let mut fees = common::fee_config().clone();
    fees.equities.taf_cap_basis = mandate_accounting::TafCapBasis::PerOrder;
    fees.equities.taf_cap =
        mandate_num::FeeCap::parse("0.01").unwrap_or_else(|e| panic!("the cap: {e}"));
    let ports = Ports {
        fees: &fees,
        ..ports(&ids, &mandates, &instruments, &config)
    };
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            2,
            "FillApplied",
            with_clock(
                &[
                    ("fill_id", text("f-0")),
                    ("instrument", text(AAPL)),
                    ("side", text("buy")),
                    ("qty_gross", text("60")),
                    ("price", text("150")),
                ],
                0,
            ),
        ))
        .expect("the position folds");
    let mut shell = shell.restart(&ports).0;
    let id = shell
        .run(
            handoff(INTENT, common::AGENT, risk_exit(AAPL, "60", "150")),
            &ports,
        )
        .submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("the exit is sent");
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Submitted(broker_order(
            "b-1",
            Some(&id),
            AAPL,
            Side::Sell,
            "60",
            "0",
            "new",
        )))),
        &ports,
    );
    let mut taf = |fill: &str| {
        let ran = shell.run(
            Input::BrokerUpdate(BrokerUpdate::Fill(mandate_executor::BrokerFill {
                side: Side::Sell,
                ..broker_fill(fill, Some(&id), "30", "150")
            })),
            &ports,
        );
        ran.draft("FeesCharged")
            .and_then(|fee| fee.payload.get("taf").and_then(Value::as_str))
            .map(str::to_owned)
    };
    assert_eq!(
        taf("f-1").as_deref(),
        Some("0.006"),
        "30 x 0.0002, under the 0.01 cap"
    );
    assert_eq!(
        taf("f-2").as_deref(),
        Some("0.004"),
        "max(0, 0.01 - 0.006 already charged on the order): the cap is the order's, not the \
         fill's (DEC-87)"
    );
}

/// §5.7: a fill during a pending cancel or replace moves the filled quantity, not the state.
#[test]
fn a_fill_on_a_pending_cancel_or_replace_keeps_its_state() {
    for (status, pending) in [
        ("pending_cancel", OrderState::PendingCancel),
        ("pending_replace", OrderState::PendingReplace),
    ] {
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL]);
        let instruments = FixedInstruments;
        let config = config();
        let ports = ports(&ids, &mandates, &instruments, &config);
        let mut shell = fresh(&ports);
        let id = accepted(&mut shell, &ports, INTENT, common::AGENT, AAPL);
        shell.run(
            Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
                "b-1",
                Some(&id),
                AAPL,
                Side::Buy,
                "10",
                "0",
                status,
            ))),
            &ports,
        );
        shell.run(
            Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
                "f-1",
                Some(&id),
                "4",
                "150",
            ))),
            &ports,
        );
        let order = shell.state.order(&key(&id)).expect("folded");
        assert_eq!(
            (order.state, order.filled_qty),
            (pending, qty("4")),
            "{status}: the fill is applied and the order stays where the cancel or replace left \
             it"
        );
    }
}

/// Asserts §7.3's consequence on every agent: the `*` restriction to `mode`, a second agent that
/// placed nothing held to it too, and one alert naming the event that recorded the restriction by
/// its opaque id under `key` (DEC-133 item 39, AGENTS.md rule 6).
fn every_agent_restricted_and_the_owner_alerted(
    shell: &Shell,
    ran: &common::Ran,
    mode: Mode,
    to: &str,
    key: &str,
) {
    let applied = ran
        .draft("AgentModeApplied")
        .expect("the restriction reaches the agents");
    assert_eq!(
        (
            applied.payload.get("agent").and_then(Value::as_str),
            applied.payload.get("to").and_then(Value::as_str),
        ),
        (Some("*"), Some(to)),
        "every agent on the account, including one the executor has not yet seen"
    );
    assert_eq!(shell.state.effective_mode(&agent(OTHER_AGENT)), mode);
    let changed = ran
        .draft("AccountRestrictionChanged")
        .map(|draft| draft.event_id.clone());
    let alerts: Vec<_> = ran
        .effects
        .iter()
        .filter_map(|effect| match effect {
            mandate_executor::Effect::Notify(reference) => {
                Some((Some(reference.subject_event.clone()), reference.message_key))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        alerts,
        vec![(changed, key)],
        "the owner is alerted once, by the restriction's event id and a message key"
    );
}

/// §7.3 row 1: a blocked account pauses every agent, and the owner is alerted.
#[test]
fn a_blocked_account_restricts_every_agent_and_alerts_the_owner() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Account(mandate_executor::BrokerAccount {
            trading_blocked: true,
            ..broker_account()
        })),
        &ports,
    );
    every_agent_restricted_and_the_owner_alerted(
        &shell,
        &ran,
        Mode::Paused,
        "paused",
        "account_trading_blocked",
    );
}

/// §7.3 row 2, DEC-143: a reject whose message says `closing`, without `restricted`, restricts
/// every agent to exits, and the owner is alerted.
#[test]
fn a_closing_message_alone_restricts_every_agent_and_alerts_the_owner() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Reject(broker_reject(
            None,
            403,
            "account is closing transactions only",
        ))),
        &ports,
    );
    assert_eq!(shell.state.account_state(), AccountState::ClosingOnly);
    every_agent_restricted_and_the_owner_alerted(
        &shell,
        &ran,
        Mode::ExitsOnly,
        "exits_only",
        "account_restricted",
    );
}

/// Journal spec §6: a reject is journaled with its status, code and message.
#[test]
fn a_reject_is_journaled_with_its_status_code_and_message() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let ran = shell.run(
        Input::BrokerUpdate(BrokerUpdate::Reject(mandate_executor::BrokerReject {
            code: Some("40310000".to_owned()),
            ..broker_reject(None, 422, "insufficient qty available for order")
        })),
        &ports,
    );
    let observed = ran
        .draft("RejectObserved")
        .expect("the reject is journaled");
    let field = |name: &str| observed.payload.get(name).cloned();
    assert_eq!(field("http_status").and_then(|v| v.as_int()), Some(422));
    assert_eq!(field("code"), Some(text("40310000")));
    assert_eq!(
        field("message"),
        Some(text("insufficient qty available for order"))
    );
}

/// DEC-143: every 403 counts toward §7.3's run, one naming one of our orders with a code
/// included, the stricter reading of "without a known order-level cause".
#[test]
fn a_403_naming_one_of_our_orders_still_counts_toward_the_run() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    for intent in [INTENT, OTHER_INTENT, "01JABCDEFGHJKMNPQRSTVWXYZ2"] {
        let id = accepted(&mut shell, &ports, intent, common::AGENT, AAPL);
        shell.run(
            Input::BrokerUpdate(BrokerUpdate::Reject(mandate_executor::BrokerReject {
                code: Some("40310000".to_owned()),
                ..broker_reject(Some(&id), 403, "forbidden")
            })),
            &ports,
        );
    }
    assert_eq!(
        shell.state.account_state(),
        AccountState::ClosingOnly,
        "three 403s in a row restrict the account, whichever orders they named"
    );
}

/// §7.3: a restriction starts a fresh run of 403s, and the account is asked for once, when the run
/// restricts it, not at every 403 after.
#[test]
fn a_restriction_resets_the_run_and_asks_for_the_account_once() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let asked = |ran: &common::Ran| {
        ran.requests
            .iter()
            .filter(|request| matches!(request, BrokerRequest::GetAccount))
            .count()
    };
    let forbidden =
        || Input::BrokerUpdate(BrokerUpdate::Reject(broker_reject(None, 403, "forbidden")));
    let mut total = 0;
    for _ in 0..3 {
        total += asked(&shell.run(forbidden(), &ports));
    }
    assert_eq!(
        total, 1,
        "the third 403 restricts the account and asks for it"
    );
    assert_eq!(
        shell.state.consecutive_403s(),
        0,
        "and the restriction starts the run afresh"
    );
    for _ in 0..4 {
        assert_eq!(
            asked(&shell.run(forbidden(), &ports)),
            0,
            "a restricted account is not asked for again at every 403"
        );
    }
}
