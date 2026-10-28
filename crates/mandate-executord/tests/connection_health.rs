//! E7-2z slice D (connections spec §8.2, DEC-687, DEC-2826, DEC-2832): the process counts the
//! broker's answers per class and, once a count holds at its threshold, tells the core at every
//! pass, so the account is restricted to closing with cause `connection_unavailable`. A lost answer
//! counts as a network failure, never as healthy; a `401` as an authorization failure; a good
//! answer of a class resets only that class. The oracles are the records each case expects.

mod support;

use mandate_canon::Value;
use mandate_executor::{
    AccountState, BrokerOutcome, BrokerReject, BrokerUnknown, ConnectionTrouble, ConnectorError,
    DegradeReason, SuspendReason,
};
use mandate_executord::ExecutorProcess;
use mandate_executord::health::Health;
use mandate_process::State;
use proptest::prelude::*;
use support::{
    ACCOUNT_STREAM, AGENT, Error, Journal, OPENED, OPENING, Paper, at, deployed, propose,
    start_allowing,
};

const FIRST: &str = "01JINTENT0000000000000000A";
const SECOND: &str = "01JINTENT0000000000000000B";
const THIRD: &str = "01JINTENT0000000000000000D";

/// An executor up and ready with the opening still working at the broker, so every pass reads the
/// broker for its scheduled reconciliation (X1b-3).
fn working() -> Result<(Journal, Paper, ExecutorProcess<Journal, Paper>), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    broker.resting(OPENING, "AAPL", "10", "150")?;
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at("2026-09-21T15:00:00Z")?);
    if !process.ready() {
        return Err("the executor is not ready".into());
    }
    Ok((journal, broker, process))
}

/// Steps a pass a minute apart from 15:01, `passes` of them, starting at minute `from`.
fn passes(
    process: &mut ExecutorProcess<Journal, Paper>,
    from: u32,
    passes: u32,
) -> Result<(), Error> {
    for minute in from..from + passes {
        process.step(at(&format!("2026-09-21T15:{minute:02}:00Z"))?);
        if matches!(process.lifecycle().state(), State::Exited(_)) {
            return Err(format!("15:{minute:02}: the process stopped").into());
        }
    }
    Ok(())
}

/// The causes of the restrictions the account stream holds, in order.
fn restriction_causes(journal: &Journal) -> Vec<String> {
    journal
        .payloads("AccountRestrictionChanged")
        .iter()
        .filter_map(|payload| {
            payload
                .get("cause")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

#[test]
fn lost_answers_in_a_row_restrict_the_account_once() -> Result<(), Error> {
    let (journal, broker, mut process) = working()?;
    broker.lose_next_answers(64);
    passes(&mut process, 1, 8)?;
    assert_eq!(restriction_causes(&journal), ["connection_unavailable"]);
    assert_eq!(process.state().account_state(), AccountState::ClosingOnly);
    let restricted_at = journal
        .types(ACCOUNT_STREAM)
        .iter()
        .position(|kind| kind == "AccountRestrictionChanged");
    let modes_at = journal
        .types(ACCOUNT_STREAM)
        .iter()
        .position(|kind| kind == "AgentModeApplied");
    assert!(
        restricted_at < modes_at && modes_at.is_some(),
        "the agents follow the restriction: {:?}",
        journal.types(ACCOUNT_STREAM)
    );
    Ok(())
}

#[test]
fn two_lost_answers_then_a_good_one_restrict_nothing() -> Result<(), Error> {
    let (journal, broker, mut process) = working()?;
    for from in [1, 4, 7, 10] {
        broker.lose_next_answers(2);
        passes(&mut process, from, 3)?;
    }
    assert!(restriction_causes(&journal).is_empty());
    assert_eq!(process.state().account_state(), AccountState::Active);
    Ok(())
}

#[test]
fn authorization_failures_restrict_the_account() -> Result<(), Error> {
    let (journal, broker, mut process) = working()?;
    broker.refuse_next_with(401, 64);
    passes(&mut process, 1, 6)?;
    assert_eq!(restriction_causes(&journal), ["connection_unavailable"]);
    assert_eq!(process.state().account_state(), AccountState::ClosingOnly);
    Ok(())
}

/// The pass a refused credential crosses its threshold in restricts the account before the next
/// proposal is taken (connections spec §9.1: the restriction before anything else is sent), so an
/// opening proposed behind it is never handed as an opening of an active account.
#[test]
fn a_threshold_crossed_mid_pass_restricts_before_the_next_proposal() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(support::NOON)?);
    broker.refuse_next_with(401, 64);
    propose(&journal, AGENT, FIRST, "buy", "10", "150", "open")?;
    propose(&journal, AGENT, SECOND, "buy", "10", "150", "open")?;
    propose(&journal, AGENT, THIRD, "buy", "10", "150", "open")?;
    process.step(at(support::NOON)?);
    let types = journal.types(ACCOUNT_STREAM);
    let restricted = types
        .iter()
        .position(|kind| kind == "AccountRestrictionChanged")
        .ok_or("the refused credential restricted nothing")?;
    let received: Vec<usize> = types
        .iter()
        .enumerate()
        .filter(|(_, kind)| *kind == "IntentReceived")
        .map(|(at, _)| at)
        .collect();
    assert_eq!(received.len(), 3, "{types:?}");
    assert!(
        received.iter().skip(1).all(|at| *at > restricted),
        "the openings behind the crossing are taken only after the restriction: {types:?}"
    );
    assert_eq!(
        types
            .iter()
            .filter(|kind| *kind == "OrderSubmitted")
            .count(),
        1,
        "{types:?}"
    );
    Ok(())
}

#[test]
fn a_broker_s_403_is_not_an_authorization_failure() -> Result<(), Error> {
    let (journal, broker, mut process) = working()?;
    broker.refuse_next_with(403, 64);
    for minute in 1..7 {
        process.step(at(&format!("2026-09-21T15:{minute:02}:00Z"))?);
    }
    assert!(
        !restriction_causes(&journal).contains(&"connection_unavailable".to_owned()),
        "a 403 is the broker's refusal, read by trading spec §7.3's reject rows: {:?}",
        restriction_causes(&journal)
    );
    Ok(())
}

/// An answer with the network and authorization readings the test expects of it.
type Classified = (
    Result<BrokerOutcome, ConnectorError>,
    Option<bool>,
    Option<bool>,
);

/// The answers the counter is fuzzed with, each with the classes the test reads from it on its
/// own (connections spec §8.2, DEC-2832): `(network, authorization)`, where `Some(true)` is a
/// failure, `Some(false)` a good result, and `None` no reading.
fn answers() -> Vec<Classified> {
    let reject = |status| {
        Ok(BrokerOutcome::Rejected(BrokerReject {
            client_order_id: None,
            http_status: status,
            code: None,
            message: String::new(),
        }))
    };
    vec![
        (
            Err(ConnectorError::Unknown(BrokerUnknown::Timeout)),
            Some(true),
            None,
        ),
        (
            Err(ConnectorError::Unknown(BrokerUnknown::Ambiguous)),
            Some(true),
            None,
        ),
        (
            Err(ConnectorError::Unreadable { code: "wire" }),
            Some(true),
            None,
        ),
        (Err(ConnectorError::NotSent { code: "refused" }), None, None),
        (reject(401), Some(false), Some(true)),
        (
            Err(ConnectorError::Unknown(BrokerUnknown::Unauthorized)),
            Some(false),
            Some(true),
        ),
        (reject(403), Some(false), None),
        (reject(422), Some(false), Some(false)),
        (
            Ok(BrokerOutcome::OpenOrders(Vec::new())),
            Some(false),
            Some(false),
        ),
    ]
}

/// The failures in a row a class shows at the end of `readings`: counted back from the last one,
/// skipping answers with no reading of the class, up to the last good result.
fn trailing(readings: &[Option<bool>]) -> usize {
    readings
        .iter()
        .rev()
        .flatten()
        .take_while(|failed| **failed)
        .count()
}

proptest! {
    #[test]
    fn the_troubles_are_the_classes_failing_at_their_thresholds(
        picks in proptest::collection::vec(0usize..9, 0..40),
    ) {
        let table = answers();
        let mut health = Health::default();
        let mut network = Vec::new();
        let mut authorization = Vec::new();
        for pick in picks {
            let (answer, net, auth) = table
                .get(pick)
                .cloned()
                .ok_or_else(|| TestCaseError::fail("no such answer"))?;
            health.observe(&answer);
            network.push(net);
            authorization.push(auth);
        }
        let mut expected = Vec::new();
        if trailing(&network) >= 3 {
            expected.push(ConnectionTrouble::Degraded(DegradeReason::NetworkErrors));
        }
        if trailing(&authorization) >= 2 {
            expected.push(ConnectionTrouble::Suspended(SuspendReason::AuthorizationFailed));
        }
        prop_assert_eq!(health.troubles(), expected);
    }
}

const EXIT: &str = "01JINTENT0000000000000000C";
const SWITCH: &str = "01JKILL00000000000000000AA";

/// An executor over a broker whose every answer leaves 3 of a budget of 200 (DEC-2842), its agent
/// holding the 10 AAPL its opening bought with the opening's protection resting at the broker (the
/// outage tests' owed exit), handed `owed` before its first pass; the passes go on until the low
/// headroom has restricted the account to closing.
fn owed_under_low_headroom(owed: &dyn Fn(&Journal) -> Result<(), Error>) -> Result<Journal, Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    broker.resting(OPENING, "AAPL", "10", "150")?;
    broker.fill(OPENING, "150")?;
    broker.resting_sell("md-protect", "AAPL", "10", "147")?;
    broker.report_rate_limit(200, 3);
    let mut process = start_allowing(&journal, &broker)?;
    owed(&journal)?;
    process.step(at("2026-09-21T15:00:00Z")?);
    passes(&mut process, 1, 4)?;
    assert_eq!(restriction_causes(&journal), ["connection_unavailable"]);
    assert_eq!(process.state().account_state(), AccountState::ClosingOnly);
    Ok(journal)
}

/// Where on the account stream the low headroom's restriction and each sell submitted sit, with
/// each sell's quantity.
fn sells_after_restriction(journal: &Journal) -> Result<(usize, Vec<(usize, String)>), Error> {
    let types = journal.types(ACCOUNT_STREAM);
    let restricted = types
        .iter()
        .position(|kind| kind == "AccountRestrictionChanged")
        .ok_or("the low headroom restricted nothing")?;
    let at: Vec<usize> = types
        .iter()
        .enumerate()
        .filter(|(_, kind)| *kind == "OrderSubmitted")
        .map(|(at, _)| at)
        .collect();
    let sells = at
        .into_iter()
        .zip(journal.payloads("OrderSubmitted"))
        .filter(|(_, payload)| payload.get("side").and_then(Value::as_str) == Some("sell"))
        .map(|(at, payload)| {
            let qty = payload
                .get("qty")
                .and_then(Value::as_str)
                .unwrap_or_default();
            (at, qty.to_owned())
        })
        .collect();
    Ok((restricted, sells))
}

/// Rule 13 (DEC-2842): a connection degraded by low rate-limit headroom never holds a risk exit.
/// The agent's exit of its 10 shares is journaled and sent after the account is restricted.
#[test]
fn low_headroom_never_holds_a_risk_exit() -> Result<(), Error> {
    let journal = owed_under_low_headroom(&|journal| {
        propose(journal, AGENT, EXIT, "sell", "10", "149", "risk_exit")
    })?;
    let (restricted, sells) = sells_after_restriction(&journal)?;
    assert!(
        sells
            .iter()
            .any(|(at, qty)| *at > restricted && qty == "10"),
        "the exit goes out under the restriction: {sells:?} after {restricted}, {:?}",
        journal.types(ACCOUNT_STREAM)
    );
    Ok(())
}

/// Rule 13 (DEC-2842): an agent's kill switch applied after low headroom has restricted the account
/// to closing still sells the agent's 10 shares. As in the completed tests, the switch follows the
/// first pass, which already restricts, since every answer from the start leaves 3 of 200.
#[test]
fn low_headroom_never_holds_a_kill_switch() -> Result<(), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    broker.resting(OPENING, "AAPL", "10", "150")?;
    broker.fill(OPENING, "150")?;
    broker.report_rate_limit(200, 3);
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(support::NOON)?);
    process.step(at(support::NOON)?);
    assert_eq!(restriction_causes(&journal), ["connection_unavailable"]);
    support::kill(&journal, AGENT, SWITCH)?;
    process.step(at(support::NOON)?);
    let (restricted, sells) = sells_after_restriction(&journal)?;
    assert!(
        sells
            .iter()
            .any(|(at, qty)| *at > restricted && qty == "10"),
        "the switch's sell goes out under the restriction: {sells:?} after {restricted}, {:?}",
        journal.types(ACCOUNT_STREAM)
    );
    Ok(())
}

/// Rule 13 (DEC-2842): an opening that fills while low headroom holds the account at closing is
/// still protected: the fill's protection, the bracket's legs, is journaled `placed` after the
/// restriction.
#[test]
fn low_headroom_never_holds_a_fills_protection() -> Result<(), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    broker.resting(OPENING, "AAPL", "10", "150")?;
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at("2026-09-21T15:00:00Z")?);
    broker.report_rate_limit(200, 3);
    passes(&mut process, 1, 3)?;
    assert_eq!(restriction_causes(&journal), ["connection_unavailable"]);
    let (restricted, _) = sells_after_restriction(&journal)?;
    let placed = |journal: &Journal| -> Vec<usize> {
        journal
            .types(ACCOUNT_STREAM)
            .iter()
            .enumerate()
            .filter(|(_, kind)| *kind == "ProtectionChanged")
            .map(|(at, _)| at)
            .zip(journal.payloads("ProtectionChanged"))
            .filter(|(_, payload)| payload.get("action").and_then(Value::as_str) == Some("placed"))
            .map(|(at, _)| at)
            .collect()
    };
    assert!(
        placed(&journal).is_empty(),
        "nothing protected before the fill"
    );
    broker.fill(OPENING, "150")?;
    passes(&mut process, 4, 3)?;
    let after = placed(&journal);
    assert!(
        after.len() == 1 && after.iter().all(|at| *at > restricted),
        "the protection lands while the account is restricted: {:?}",
        journal.types(ACCOUNT_STREAM)
    );
    assert_eq!(process.state().account_state(), AccountState::ClosingOnly);
    Ok(())
}
