//! The own-fill pass's runner (DEC-955 items 2, 4 and 5) over the permissive doubles: it reads
//! the broker only when the folded stream owes an order, and then only the owed order by its own
//! id and the account's activities, page by page to their end; it appends the executor's drafts
//! at the head it read and nothing else; and every stage that cannot answer stops it at Reconcile
//! with nothing journaled.

use std::cell::RefCell;
use std::rc::Rc;

use mandate_canon::{Object, Value};
use mandate_domain::CapabilityProfile;
use mandate_executor::{
    ActivityCursor, BrokerFill, BrokerOrder, BrokerOutcome, BrokerRequest, ClientOrderId,
    ConnectorError, Effect, EventDraft, EventId, FillId, FoldedEvent, RiskClock, WriterEpoch,
};
use mandate_num::{Price, Qty, Usd};
use mandate_time::Date;

use super::doubles::{World, account_stream, instrument, setup};
use super::{Connector, Executor, Stage};
use crate::error::{Cause, ShellError};
use crate::tracer::{Booked, ProductionCycle};

const OWED: &str = "md-00000100000000000005000001";

fn owed() -> Result<ClientOrderId, String> {
    ClientOrderId::parse(OWED).map_err(|e| e.to_string())
}

/// What the doubled executor drafts: nothing, the given drafts, or a broker request.
#[derive(Clone)]
enum Drafts {
    Nothing,
    Records(Vec<&'static str>),
    Request,
}

/// An executor double owing `owes` from `since`, `drafts` answering the own-fill pass; the orders and fills each pass was handed are kept in `handed`.
struct Owing {
    owes: Vec<ClientOrderId>,
    since: ActivityCursor,
    drafts: Drafts,
    handed: Rc<RefCell<Vec<(usize, usize)>>>,
}

impl Executor for Owing {
    fn reset(&mut self) -> Result<(), Cause> {
        Ok(())
    }

    fn use_profile(&mut self, profile: CapabilityProfile) -> Result<(), Cause> {
        let _ = profile;
        Ok(())
    }

    fn step(&mut self, input: mandate_executor::Input) -> Result<Vec<Effect>, Cause> {
        let _ = input;
        Ok(Vec::new())
    }

    fn protected(&mut self, instrument: &mandate_accounting::InstrumentId) -> Result<bool, Cause> {
        let _ = instrument;
        Ok(true)
    }

    fn committed(&mut self, event: &FoldedEvent) -> Result<(), Cause> {
        let _ = event;
        Ok(())
    }

    fn owed(&self) -> Result<(Vec<ClientOrderId>, ActivityCursor), Cause> {
        Ok((self.owes.clone(), self.since.clone()))
    }

    fn book_own(
        &mut self,
        epoch: WriterEpoch,
        at: RiskClock,
        orders: &[BrokerOrder],
        fills: &[BrokerFill],
    ) -> Result<Vec<Effect>, Cause> {
        let _ = at;
        let drafts = self.drafts.clone();
        self.handed.borrow_mut().push((orders.len(), fills.len()));
        Ok(match drafts {
            Drafts::Nothing => Vec::new(),
            Drafts::Records(types) => types
                .iter()
                .enumerate()
                .map(|(n, kind)| {
                    Effect::Journal(EventDraft {
                        event_id: EventId(format!("e{}h0o{n}", epoch.0)),
                        event_type: (*kind).to_owned(),
                        schema_version: 1,
                        config_refs: Object::new(),
                        causation_id: None,
                        payload: Value::Object(Object::new()),
                    })
                })
                .collect(),
            Drafts::Request => vec![Effect::Broker(BrokerRequest::ListPositions)],
        })
    }
}

/// How the doubled broker answers the lookup of the owed order.
#[derive(Clone, Copy)]
enum Lookup {
    Filled,
    Absent,
    Refused,
}

/// A broker that answers the lookup as `lookup` says and every activities page with one fill
/// under a cursor that moves `pages` times before it repeats; it logs every request it is handed.
struct Reads {
    lookup: Lookup,
    pages: usize,
    log: Rc<RefCell<Vec<String>>>,
}

impl Connector for Reads {
    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        match request {
            BrokerRequest::GetOrderByClientId(id) => {
                self.log
                    .borrow_mut()
                    .push(format!("lookup {}", id.as_str()));
                match self.lookup {
                    Lookup::Filled => filled(id)
                        .map(BrokerOutcome::Order)
                        .map_err(|_| ConnectorError::NotSent { code: "fixture" }),
                    Lookup::Absent => Ok(BrokerOutcome::Absent {
                        client_order_id: id.as_str().to_owned(),
                    }),
                    Lookup::Refused => Err(ConnectorError::NotSent { code: "refused" }),
                }
            }
            BrokerRequest::ListActivities { since } => {
                self.log
                    .borrow_mut()
                    .push(format!("activities from {:?}", since.0));
                let read = self.log.borrow().len();
                let page = read.min(self.pages.saturating_add(2));
                let one = fill(page).map_err(|_| ConnectorError::NotSent { code: "fixture" })?;
                Ok(BrokerOutcome::Activities {
                    fills: vec![one],
                    cursor: ActivityCursor(format!("c{page}")),
                })
            }
            other => {
                self.log.borrow_mut().push(format!("{other:?}"));
                Err(ConnectorError::NotSent { code: "not_a_read" })
            }
        }
    }
}

fn filled(id: &ClientOrderId) -> Result<BrokerOrder, Cause> {
    Ok(BrokerOrder {
        broker_order_id: "b-1".to_owned(),
        client_order_id: Some(id.as_str().to_owned()),
        instrument: instrument()?,
        side: mandate_accounting::Side::Buy,
        qty: Qty::parse("1")?,
        filled_qty: Qty::parse("1")?,
        limit_price: None,
        stop_price: None,
        status: "filled".to_owned(),
        reject_code: None,
        replaced_by_broker_order_id: None,
        legs: Vec::new(),
        created_on: None,
    })
}

fn fill(page: usize) -> Result<BrokerFill, Cause> {
    Ok(BrokerFill {
        fill_id: FillId(format!("f-{page}")),
        client_order_id: None,
        broker_order_id: Some("b-1".to_owned()),
        instrument: instrument()?,
        side: mandate_accounting::Side::Buy,
        qty: Qty::parse("1")?,
        price: Price::parse("1")?,
        fees: Usd::ZERO,
        trade_date: Date::parse("2026-09-28").map_err(|_| Cause::Absent { what: "a date" })?,
    })
}

/// One pass over the doubles: the executor owing `owes` and drafting `drafts`, the broker
/// answering `lookup` with `pages` moving activity cursors. Answers the outcome, the broker's
/// log, what the executor was handed, and the account stream's committed types.
struct Pass {
    outcome: Result<Booked, ShellError>,
    log: Vec<String>,
    handed: Vec<(usize, usize)>,
    committed: Vec<String>,
}

fn pass(
    owes: Vec<ClientOrderId>,
    drafts: Drafts,
    lookup: Lookup,
    pages: usize,
) -> Result<Pass, String> {
    let world = World::default();
    let mut stages = world.stages();
    let (log, handed) = (Rc::default(), Rc::default());
    stages.executor = Box::new(Owing {
        owes,
        since: ActivityCursor("c0".to_owned()),
        drafts,
        handed: Rc::clone(&handed),
    });
    stages.connector = Box::new(Reads {
        lookup,
        pages,
        log: Rc::clone(&log),
    });
    let outcome = ProductionCycle::new(stages, setup()?).book_own_fills();
    let committed = world.ledger.borrow().types(&account_stream());
    let log = log.borrow().clone();
    let handed = handed.borrow().clone();
    Ok(Pass {
        outcome,
        log,
        handed,
        committed,
    })
}

/// The stop `outcome` is, as its stage and message.
fn stop(outcome: &Result<Booked, ShellError>) -> Option<(Stage, String)> {
    match outcome {
        Err(ShellError::Refused { stage, cause }) => Some((*stage, cause.to_string())),
        Ok(_) | Err(_) => None,
    }
}

/// An executor without the own-fill reconciliation stops the pass at Reconcile before the broker
/// is read or anything is written, rather than skip it.
#[test]
fn an_executor_without_it_stops_before_any_read() -> Result<(), String> {
    let world = World::default();
    let outcome = ProductionCycle::new(world.stages(), setup()?).book_own_fills();
    assert_eq!(
        stop(&outcome),
        Some((Stage::Reconcile, "the own-fill reconciliation".to_owned()))
    );
    assert!(world.ledger.borrow().types(&account_stream()).is_empty());
    let world = World::default();
    let mut stages = world.stages();
    stages.executor = Box::new(Unbooking(owed()?));
    stages.connector = Box::new(Reads {
        lookup: Lookup::Filled,
        pages: 0,
        log: Rc::default(),
    });
    let outcome = ProductionCycle::new(stages, setup()?).book_own_fills();
    assert_eq!(
        stop(&outcome),
        Some((Stage::Reconcile, "the own-fill reconciliation".to_owned())),
        "an executor that owes but cannot book stops after the reads"
    );
    assert!(world.ledger.borrow().types(&account_stream()).is_empty());
    Ok(())
}

/// An executor that owes one order and has no own-fill booking of its own.
struct Unbooking(ClientOrderId);

impl Executor for Unbooking {
    fn reset(&mut self) -> Result<(), Cause> {
        Ok(())
    }

    fn use_profile(&mut self, profile: CapabilityProfile) -> Result<(), Cause> {
        let _ = profile;
        Ok(())
    }

    fn step(&mut self, input: mandate_executor::Input) -> Result<Vec<Effect>, Cause> {
        let _ = input;
        Ok(Vec::new())
    }

    fn protected(&mut self, instrument: &mandate_accounting::InstrumentId) -> Result<bool, Cause> {
        let _ = instrument;
        Ok(true)
    }

    fn committed(&mut self, event: &FoldedEvent) -> Result<(), Cause> {
        let _ = event;
        Ok(())
    }

    fn owed(&self) -> Result<(Vec<ClientOrderId>, ActivityCursor), Cause> {
        Ok((vec![self.0.clone()], ActivityCursor(String::new())))
    }
}

/// A stream that owes nothing reads nothing and writes nothing.
#[test]
fn nothing_owed_reads_and_writes_nothing() -> Result<(), String> {
    let pass = pass(
        Vec::new(),
        Drafts::Records(vec!["LateFillApplied"]),
        Lookup::Filled,
        0,
    )?;
    assert!(
        matches!(pass.outcome, Ok(Booked { fills: 0 })),
        "{:?}",
        pass.outcome
    );
    assert!(pass.log.is_empty(), "{:?}", pass.log);
    assert!(pass.handed.is_empty() && pass.committed.is_empty());
    Ok(())
}

/// An owed order is read by its own id, the activities from the stream's checkpoint to the page
/// whose cursor stops moving, and the executor's drafts are appended; only fill records count.
#[test]
fn an_owed_order_is_read_by_its_id_and_its_records_appended() -> Result<(), String> {
    let pass = pass(
        vec![owed()?],
        Drafts::Records(vec!["LateFillApplied", "OrderStateChanged", "FillApplied"]),
        Lookup::Filled,
        1,
    )?;
    assert!(
        matches!(pass.outcome, Ok(Booked { fills: 2 })),
        "{:?}",
        pass.outcome
    );
    assert_eq!(
        pass.log,
        [
            format!("lookup {OWED}"),
            "activities from \"c0\"".to_owned(),
            "activities from \"c2\"".to_owned(),
            "activities from \"c3\"".to_owned(),
        ]
    );
    assert_eq!(pass.handed, [(1, 3)], "one order, every fill read");
    assert_eq!(
        pass.committed,
        ["LateFillApplied", "OrderStateChanged", "FillApplied"]
    );
    let none = self::pass(vec![owed()?], Drafts::Nothing, Lookup::Filled, 0)?;
    assert!(
        matches!(none.outcome, Ok(Booked { fills: 0 })),
        "{:?}",
        none.outcome
    );
    assert!(none.committed.is_empty());
    Ok(())
}

/// An order the broker does not have filled nothing, so it is handed to nobody; a lookup the
/// broker does not answer stops the pass.
#[test]
fn an_absent_order_owes_nothing_and_an_unanswered_lookup_stops() -> Result<(), String> {
    let absent = pass(vec![owed()?], Drafts::Nothing, Lookup::Absent, 0)?;
    assert!(
        matches!(absent.outcome, Ok(Booked { fills: 0 })),
        "{:?}",
        absent.outcome
    );
    assert_eq!(absent.handed, [(0, 2)]);
    let refused = pass(vec![owed()?], Drafts::Nothing, Lookup::Refused, 0)?;
    assert_eq!(
        stop(&refused.outcome),
        Some((
            Stage::Reconcile,
            "the broker's answer for an owned order".to_owned()
        ))
    );
    assert_eq!(
        refused.log,
        [format!("lookup {OWED}")],
        "nothing read after it"
    );
    Ok(())
}

/// Activities whose cursor never stops moving stop the pass at Reconcile after the bounded pages,
/// with nothing written.
#[test]
fn activities_that_never_end_stop_the_pass() -> Result<(), String> {
    let pass = pass(
        vec![owed()?],
        Drafts::Records(vec!["LateFillApplied"]),
        Lookup::Filled,
        usize::MAX,
    )?;
    assert_eq!(
        stop(&pass.outcome),
        Some((Stage::Reconcile, "account activities that end".to_owned()))
    );
    let pages = pass
        .log
        .iter()
        .filter(|line| line.starts_with("activities"))
        .count();
    assert_eq!(pages, 20);
    assert!(pass.committed.is_empty());
    Ok(())
}

/// An executor that answers the pass with anything but journal records stops it, with nothing
/// written and nothing sent.
#[test]
fn a_draft_that_is_not_a_record_stops_the_pass() -> Result<(), String> {
    let pass = pass(vec![owed()?], Drafts::Request, Lookup::Filled, 0)?;
    assert_eq!(
        stop(&pass.outcome),
        Some((
            Stage::Reconcile,
            "an own-fill reconciliation that only journals".to_owned()
        ))
    );
    assert!(pass.committed.is_empty());
    assert!(
        !pass.log.iter().any(|line| line.contains("ListPositions")),
        "{:?}",
        pass.log
    );
    Ok(())
}
