//! The simulator's invariants over random scripts (E7-25). Each oracle is the test's own: a map of
//! `ref_id`s, an accumulator of accepted fills, and a transition table written here, never the
//! simulator's answer to the same question.

mod common;

use std::collections::BTreeMap;

use common::{AGENTIC, limit, price, qty, sim};
use mandate_num::Qty;
use mandate_rh_sim::{Event, Fault, Order, OrderRequest, Session, Side, SimError, State};
use proptest::prelude::*;

/// One scripted step. `order` counts back from the newest order the script created.
#[derive(Debug, Clone)]
enum Op {
    Place {
        n: u32,
        sell: bool,
        quantity: u8,
        extended: bool,
        fault: Option<Fault>,
    },
    Fill {
        order: usize,
        quantity: u8,
        through: bool,
    },
    Cancel {
        order: usize,
    },
    Advance {
        order: usize,
        to: State,
    },
    Session(Session),
    EndOfDay,
}

const STATES: [State; 10] = [
    State::New,
    State::Queued,
    State::Confirmed,
    State::Unconfirmed,
    State::PartiallyFilled,
    State::Filled,
    State::Cancelled,
    State::Rejected,
    State::Failed,
    State::Voided,
];

fn op() -> impl Strategy<Value = Op> {
    let fault = prop_oneof![
        6 => Just(None),
        1 => Just(Some(Fault::LoseAnswer)),
        1 => proptest::sample::select(STATES.to_vec()).prop_map(|s| Some(Fault::Answer(s))),
    ];
    let session = prop_oneof![
        1 => Just(Session::Closed),
        1 => Just(Session::Overnight),
        1 => Just(Session::Extended),
        3 => Just(Session::Regular),
    ];
    prop_oneof![
        3 => (0..4u32, any::<bool>(), 1..5u8, any::<bool>(), fault)
            .prop_map(|(n, sell, quantity, extended, fault)| Op::Place { n, sell, quantity, extended, fault }),
        6 => (0..4usize, 1..4u8, prop::bool::weighted(0.2)).prop_map(|(order, quantity, through)| Op::Fill { order, quantity, through }),
        1 => (0..4usize).prop_map(|order| Op::Cancel { order }),
        3 => (0..4usize, proptest::sample::select(STATES.to_vec())).prop_map(|(order, to)| Op::Advance { order, to }),
        2 => session.prop_map(Op::Session),
        1 => Just(Op::EndOfDay),
    ]
}

fn is_terminal(state: State) -> bool {
    matches!(
        state,
        State::Filled | State::Cancelled | State::Rejected | State::Failed | State::Voided
    )
}

/// The contract's lifecycle as this test reads it: every move an order's state may make.
fn legal(from: State, to: State) -> bool {
    use State::*;
    match from {
        New => matches!(
            to,
            Queued | Confirmed | Unconfirmed | Cancelled | Rejected | Failed
        ),
        Queued => matches!(to, Confirmed | Unconfirmed | Cancelled | Rejected | Failed),
        Unconfirmed => matches!(to, Confirmed | Cancelled | Rejected | Failed),
        Confirmed => matches!(to, PartiallyFilled | Filled | Cancelled | Voided | Failed),
        PartiallyFilled => matches!(to, PartiallyFilled | Filled | Cancelled | Voided),
        Filled | Cancelled | Rejected | Failed | Voided => false,
    }
}

/// One applied step: the op, the order it targets, how it answered, and the account's orders around it.
struct Step {
    op: Op,
    target: Option<String>,
    result: Result<(), SimError>,
    before: Vec<Order>,
    after: Vec<Order>,
}

impl Step {
    fn target_before(&self) -> Option<&Order> {
        self.before
            .iter()
            .find(|o| Some(&o.id) == self.target.as_ref())
    }
}

struct Trace {
    steps: Vec<Step>,
    first_by_ref: BTreeMap<u32, String>,
    resent: Vec<(String, String)>,
    filled: BTreeMap<String, Qty>,
    bought: Qty,
    sold: Qty,
    position: Qty,
}

fn request(n: u32, sell: bool, quantity: u8, extended: bool) -> OrderRequest {
    let (side, limit_price) = if sell {
        ("sell", "499")
    } else {
        ("buy", "501")
    };
    let hours = extended.then(|| "extended_hours".to_owned());
    OrderRequest {
        market_hours: hours,
        ..limit(side, &quantity.to_string(), limit_price, n)
    }
}

fn run(ops: &[Op]) -> Result<Trace, SimError> {
    let mut sim = sim()?;
    let mut ids: Vec<String> = Vec::new();
    let mut trace = Trace {
        steps: Vec::new(),
        first_by_ref: BTreeMap::new(),
        resent: Vec::new(),
        filled: BTreeMap::new(),
        bought: Qty::ZERO,
        sold: Qty::ZERO,
        position: Qty::ZERO,
    };
    for op in ops {
        let before = sim.orders(AGENTIC)?;
        let target = match op {
            Op::Fill { order, .. } | Op::Cancel { order } | Op::Advance { order, .. } => {
                ids.iter().rev().nth(order % ids.len().max(1)).cloned()
            }
            _ => None,
        };
        let id = target.clone().unwrap_or_default();
        let result = match op {
            Op::Place {
                n,
                sell,
                quantity,
                extended,
                fault,
            } => {
                if let Some(fault) = fault {
                    let _ = sim.apply(Event::Script(*fault));
                }
                let placed = sim.place(&request(*n, *sell, *quantity, *extended));
                let created = match &placed {
                    Ok(order) => Some(order.id.clone()),
                    Err(SimError::AnswerLost) => sim.orders(AGENTIC)?.first().map(|o| o.id.clone()),
                    Err(_) => None,
                };
                match (created, trace.first_by_ref.get(n)) {
                    (Some(returned), Some(first)) => trace.resent.push((first.clone(), returned)),
                    (Some(created), None) => {
                        trace.first_by_ref.insert(*n, created.clone());
                        ids.push(created);
                    }
                    (None, _) => {}
                }
                placed.map(|_| ())
            }
            Op::Fill {
                quantity, through, ..
            } => {
                let sell = before.iter().any(|o| o.id == id && o.side == Side::Sell);
                let fill_price = match (sell, through) {
                    (false, true) => "502",
                    (true, true) => "498",
                    _ => "500",
                };
                let q = qty(&quantity.to_string());
                let filled = sim.fill(&id, q, price(fill_price));
                if filled.is_ok() {
                    let total = trace.filled.entry(id.clone()).or_insert(Qty::ZERO);
                    *total = total.checked_add(q).unwrap();
                    let side = if sell {
                        &mut trace.sold
                    } else {
                        &mut trace.bought
                    };
                    *side = side.checked_add(q).unwrap();
                }
                filled.map(|_| ())
            }
            Op::Cancel { .. } => sim.cancel(AGENTIC, &id).map(|_| ()),
            Op::Advance { to, .. } => sim.advance(&id, *to).map(|_| ()),
            Op::Session(session) => sim.apply(Event::Session(*session)),
            Op::EndOfDay => sim.apply(Event::EndOfDay),
        };
        let after = sim.orders(AGENTIC)?;
        trace.steps.push(Step {
            op: op.clone(),
            target,
            result,
            before,
            after,
        });
    }
    trace.position = sim.position(AGENTIC, "SPY")?;
    Ok(trace)
}

fn scripts() -> impl Strategy<Value = Vec<Op>> {
    proptest::collection::vec(op(), 1..40)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn a_ref_id_never_yields_a_second_order(ops in scripts()) {
        let trace = run(&ops)?;
        for (first, returned) in &trace.resent {
            prop_assert_eq!(first, returned, "a re-sent ref_id returns the first order");
        }
        let last = trace.steps.last().map(|s| s.after.len()).unwrap_or_default();
        prop_assert_eq!(last, trace.first_by_ref.len(), "one order per ref_id that created one");
    }

    #[test]
    fn a_terminal_order_never_changes_and_is_refused(ops in scripts()) {
        for step in run(&ops)?.steps {
            for old in step.before.iter().filter(|o| is_terminal(o.state)) {
                prop_assert_eq!(step.after.iter().find(|o| o.id == old.id), Some(old), "after {:?}", step.op);
            }
            if step.target_before().is_some_and(|o| is_terminal(o.state)) {
                prop_assert_eq!(&step.result, &Err(SimError::Terminal), "{:?}", step.op);
            }
        }
    }

    #[test]
    fn a_fill_never_exceeds_the_quantity_and_filled_means_complete(ops in scripts()) {
        let trace = run(&ops)?;
        prop_assert!(trace.sold <= trace.bought, "a sell never fills more than the shares held");
        prop_assert_eq!(trace.position, trace.bought.checked_sub(trace.sold).unwrap(), "buys less sells");
        let Some(last) = trace.steps.last() else { return Ok(()) };
        for order in &last.after {
            let executed = order.executions.iter().try_fold(Qty::ZERO, |sum, e| sum.checked_add(e.quantity)).unwrap();
            let accepted = trace.filled.get(&order.id).copied().unwrap_or(Qty::ZERO);
            prop_assert_eq!((order.filled_quantity, executed), (accepted, accepted));
            prop_assert!(order.filled_quantity <= order.quantity);
            prop_assert_eq!(order.state == State::Filled, order.filled_quantity == order.quantity);
        }
    }

    #[test]
    fn every_state_change_is_a_legal_transition(ops in scripts()) {
        for step in run(&ops)?.steps {
            for new in &step.after {
                match step.before.iter().find(|o| o.id == new.id) {
                    Some(old) => prop_assert!(old.state == new.state || legal(old.state, new.state),
                        "{:?} -> {:?} after {:?}", old.state, new.state, step.op),
                    None => prop_assert!(matches!(new.state,
                        State::New | State::Queued | State::Confirmed | State::Unconfirmed | State::Rejected | State::Failed)),
                }
            }
            if let (Op::Advance { to, .. }, Some(old)) = (&step.op, step.target_before()) {
                let by_fill = matches!(to, State::PartiallyFilled | State::Filled);
                if legal(old.state, *to) && !by_fill {
                    prop_assert_eq!(&step.result, &Ok(()), "{:?} -> {:?} is legal", old.state, to);
                }
            }
            if let (Op::Cancel { .. }, Ok(())) = (&step.op, &step.result) {
                let now = step.after.iter().find(|o| Some(&o.id) == step.target.as_ref()).map(|o| o.state);
                prop_assert_eq!(now, Some(State::Cancelled));
            }
        }
    }
}
