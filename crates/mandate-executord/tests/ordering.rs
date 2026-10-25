//! The order in which the process talks to the broker (DEC-2356 item 8's residual, the
//! coordinator's ruling): it sends one request at a time and hands each answer to the core before
//! it sends the next, and it gathers a reconciliation's reads only once every other request queued
//! has been sent and its answer handed. So a read the core asks beside a submission is always
//! taken after that submission went out, and a read that omits one of our sells is evidence the
//! broker does not hold it; `closes_due` relies on it. A shell that sent requests before handing
//! earlier answers would break both, and these tests with it.

mod support;

use mandate_executor::{BrokerRequest, ClientOrderId};
use support::{AGENT, Error, NOON, Paper, Seen, at, deployed, kill, propose, start_allowing};

const FIRST: &str = "01JINTENT0000000000000000A";
const SECOND: &str = "01JINTENT0000000000000000B";
const LOST: &str = "01JINTENT0000000000000000D";
const SWITCH: &str = "01JKILL00000000000000000AA";

fn opening(journal: &support::Journal, id: &str) -> Result<(), Error> {
    propose(journal, AGENT, id, "buy", "5", "150", "open")
}

/// The order `id` the process derives for the intent `intent`.
fn order(intent: &str) -> String {
    format!("md-{intent}")
}

/// A kill switch that cancels three working openings sends the three cancels in one step, and the
/// process hands each cancel's answer, journaling it, before it sends the next.
#[test]
fn each_answer_is_handed_before_the_next_request_is_sent() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    opening(&journal, FIRST)?;
    opening(&journal, SECOND)?;
    opening(&journal, LOST)?;
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(NOON)?);
    kill(&journal, AGENT, SWITCH)?;
    process.step(at(NOON)?);
    let cancels: Vec<(String, usize)> = broker
        .seen()
        .into_iter()
        .filter_map(|seen| match seen.request {
            BrokerRequest::Cancel { client_order_id } => {
                Some((client_order_id.as_str().to_owned(), seen.appended))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        cancels
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>(),
        [order(FIRST), order(SECOND), order(LOST)],
        "the switch cancels the three openings in one step"
    );
    assert!(
        cancels.windows(2).all(|pair| pair[1].1 > pair[0].1),
        "each cancel goes out only once the one before it was answered and journaled: {cancels:?}"
    );
    Ok(())
}

/// A restarted process's first step asks the lookup of an order whose submission's answer, and
/// its first lookup's, were lost, and the startup reconciliation together; the reconciliation's reads go out only once the
/// lookup went out and its answer was handed.
#[test]
fn a_reconciliation_is_read_only_once_every_other_request_is_answered() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let mut earlier = start_allowing(&journal, &broker)?;
    earlier.step(at(NOON)?);
    broker.lose_next_answers(2);
    opening(&journal, LOST)?;
    earlier.step(at(NOON)?);
    drop(earlier);
    let before = broker.seen().len();
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(NOON)?);
    let seen: Vec<Seen> = broker.seen().into_iter().skip(before).collect();
    let lost = ClientOrderId::parse(&order(LOST))?;
    let lookup = seen
        .iter()
        .position(
            |seen| matches!(&seen.request, BrokerRequest::GetOrderByClientId(id) if *id == lost),
        )
        .ok_or("the restart looks the lost order up")?;
    let read = seen
        .iter()
        .position(|seen| matches!(seen.request, BrokerRequest::ListOpenOrders))
        .ok_or("the restart reconciles")?;
    assert!(
        lookup < read && seen[read].appended > seen[lookup].appended,
        "the lookup is sent and its answer handed before the reconciliation reads: {:?}",
        seen.iter()
            .map(|seen| (format!("{:?}", seen.request), seen.appended))
            .collect::<Vec<_>>()
    );
    Ok(())
}
