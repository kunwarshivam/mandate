//! The core's vocabulary: the rule tables that are live code today, pinned case by case against
//! the specs rather than against the code.
//!
//! These cases are **not** pending. Each function here is rule logic that already exists in the
//! tests PR — which purposes add risk, which are exempt from pacing, which states are terminal,
//! which differences §11 adopts, what an initiator's final mode is — so a mutation of any of them
//! must fail a test today, not after the implementation PR (review round 1, finding 3). Every
//! expectation is transcribed from the spec clause named beside it.

mod common;

use mandate_executor::{
    BrokerUnknown, ConnectorError, DifferenceKind, ExecutorState, Initiator, Mode, OrderState,
    Purpose, RiskClock,
};

/// Every purpose, so a table below cannot quietly skip one.
const PURPOSES: [Purpose; 7] = [
    Purpose::Open,
    Purpose::Increase,
    Purpose::RiskExit,
    Purpose::OwnerExit,
    Purpose::DiscretionaryExit,
    Purpose::Protective,
    Purpose::Flatten,
];

#[test]
fn only_an_open_and_an_increase_add_risk() {
    for purpose in PURPOSES {
        let expected = matches!(purpose, Purpose::Open | Purpose::Increase);
        assert_eq!(
            purpose.adds_risk(),
            expected,
            "trading-domain spec §6.1: only an opening or an increase adds risk; every exit, \
             protective order, and flatten reduces it ({purpose:?})"
        );
    }
}

#[test]
fn risk_exits_protective_orders_and_flattens_are_exempt_from_every_pacing_control() {
    for purpose in PURPOSES {
        let expected = matches!(
            purpose,
            Purpose::RiskExit | Purpose::Protective | Purpose::Flatten
        );
        assert_eq!(
            purpose.exempt_from_pacing(),
            expected,
            "AGENTS.md rule 13: risk exits, protective orders, and automated kill switches are \
             exempt from all pacing; owner exits are paced by participation caps and \
             discretionary exits by conduct controls, so neither is exempt ({purpose:?})"
        );
    }
}

#[test]
fn an_owner_or_operator_stop_is_stopped_and_a_risk_limit_is_paused() {
    for (initiator, mode, sells) in [
        (Initiator::Owner, Mode::Stopped, Purpose::OwnerExit),
        (
            Initiator::PlatformOperator,
            Mode::Stopped,
            Purpose::RiskExit,
        ),
        (Initiator::RiskLimit, Mode::Paused, Purpose::RiskExit),
    ] {
        assert_eq!(
            initiator.final_mode(),
            mode,
            "trading-domain spec §5.5, DEC-100: the final mode of a {initiator:?} stop"
        );
        assert_eq!(
            initiator.sell_purpose(),
            sells,
            "an operator cannot confirm a bid, so only the owner's sells are owner exits"
        );
    }
}

#[test]
fn the_six_terminal_states_are_terminal_and_nothing_else_is() {
    let all = [
        OrderState::Intent,
        OrderState::Submitting,
        OrderState::Accepted,
        OrderState::PartiallyFilled,
        OrderState::PendingCancel,
        OrderState::PendingReplace,
        OrderState::Unknown,
        OrderState::Filled,
        OrderState::Canceled,
        OrderState::Rejected,
        OrderState::Expired,
        OrderState::Replaced,
        OrderState::Abandoned,
    ];
    let terminal = [
        OrderState::Filled,
        OrderState::Canceled,
        OrderState::Rejected,
        OrderState::Expired,
        OrderState::Replaced,
        OrderState::Abandoned,
    ];
    for state in all {
        assert_eq!(
            state.is_terminal(),
            terminal.contains(&state),
            "trading-domain spec §5.7's diagram, interpretation 26: {state:?}"
        );
    }
    assert_eq!(
        OrderState::TERMINAL,
        terminal,
        "and the published set is the same six"
    );
}

#[test]
fn only_an_order_state_or_a_missing_fill_is_ever_adopted() {
    for (kind, adoptable) in [
        (DifferenceKind::OrderState, true),
        (DifferenceKind::MissingFill, true),
        (DifferenceKind::ExternalActivity, false),
        (DifferenceKind::Position, false),
        (DifferenceKind::Cash, false),
        (DifferenceKind::Fee, false),
    ] {
        assert_eq!(
            kind.adoptable(),
            adoptable,
            "trading-domain spec §11's on-mismatch column, interpretation 13: {kind:?}"
        );
    }
}

#[test]
fn modes_and_account_states_order_from_least_to_most_strict() {
    assert!(Mode::Normal < Mode::ExitsOnly && Mode::ExitsOnly < Mode::Paused);
    assert!(Mode::Paused < Mode::Stopped);
    assert_eq!(Mode::default(), Mode::Normal);
    use mandate_executor::AccountState;
    assert!(AccountState::Active < AccountState::ClosingOnly);
    assert!(AccountState::ClosingOnly < AccountState::Blocked);
    assert_eq!(AccountState::default(), AccountState::Active);
}

#[test]
fn the_risk_clock_is_the_second_it_was_given() {
    for secs in [0_i64, 1, 42, 86_400, -5] {
        assert_eq!(RiskClock::from_secs(secs).secs(), secs);
    }
    assert!(RiskClock::from_secs(1) < RiskClock::from_secs(2));
}

#[test]
fn every_unknown_outcome_and_connector_failure_has_its_code() {
    for (unknown, code) in [
        (BrokerUnknown::Timeout, "timeout"),
        (BrokerUnknown::Ambiguous, "ambiguous"),
        (BrokerUnknown::Transport, "transport"),
    ] {
        assert_eq!(unknown.code(), code);
        assert!(!format!("{unknown}").is_empty());
        let connector = ConnectorError::from(unknown);
        assert_eq!(connector.code(), "unknown_outcome");
        assert_eq!(
            connector.as_unknown(),
            Some(unknown),
            "only an unknown outcome is folded as one (interpretation 10)"
        );
    }
    for (failure, code) in [
        (
            ConnectorError::Unreadable { code: "not_json" },
            "unreadable",
        ),
        (
            ConnectorError::NotSent {
                code: "refused_path",
            },
            "not_sent",
        ),
    ] {
        assert_eq!(failure.code(), code);
        assert_eq!(
            failure.as_unknown(),
            None,
            "an answer nobody could read, and a request that never left, are not unknown \
             outcomes: the shell stops on them (DEC-85)"
        );
        assert!(!format!("{failure}").is_empty());
    }
}

#[test]
fn a_fresh_state_knows_its_account_and_nothing_else() {
    let state = ExecutorState::new(common::scope());
    assert_eq!(state.scope(), &common::scope());
    assert!(
        !state.started(),
        "nothing has started before Input::Started"
    );
    assert_eq!(state.epoch(), None);
    assert_eq!(state.environment(), None);
    assert_eq!(state.risk_clock(), None);
    assert!(state.orders().is_empty());
    assert!(state.reservations().is_empty());
    assert!(state.unprotected_intervals().is_empty());
    assert!(state.positions().is_empty());
    assert!(state.fills().is_empty());
    assert!(state.modes().is_empty());
    assert_eq!(state.consecutive_403s(), 0);
    assert!(state.observed_account().is_none());
    assert!(state.mismatched().is_empty());
    assert!(state.checkpoint().is_none());
    assert!(state.reconciled_through().is_none());
    assert!(state.last_submission().is_none());
    assert!(state.unresolved().is_none());
    assert_eq!(
        state.account_state(),
        mandate_executor::AccountState::Active
    );
}
