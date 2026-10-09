//! An order's life in the simulator, one example per rule (E7-25): fills, `ref_id` after a lost
//! answer and its two switches, `gfd` and `gtc`, sessions, scripted answers, and the refusals of
//! cancel, fill and sell.

mod common;

use common::{AGENTIC, DAY_TRADER, NOT_AGENTIC, limit, price, qty, ref_id, sim};
use mandate_rh_sim::{Event, Fault, OrderRequest, Session, Sim, SimError, State};

type Outcome = Result<(), SimError>;

#[test]
fn a_limit_buy_is_confirmed_fills_in_parts_and_builds_the_position() -> Outcome {
    let mut sim = sim()?;
    let order = sim.place(&limit("buy", "2", "501", 1))?;
    assert_eq!((order.state, order.quantity), (State::Confirmed, qty("2")));
    let part = sim.fill(&order.id, qty("1"), price("500"))?;
    assert_eq!(
        (part.state, part.filled_quantity),
        (State::PartiallyFilled, qty("1"))
    );
    let done = sim.fill(&order.id, qty("1"), price("501"))?;
    assert_eq!((done.state, done.executions.len()), (State::Filled, 2));
    assert_eq!(sim.position(AGENTIC, "SPY")?, qty("2"));
    let sold = sim.place(&limit("sell", "2", "499", 2))?;
    sim.fill(&sold.id, qty("2"), price("499"))?;
    assert_eq!(sim.position(AGENTIC, "SPY")?, qty("0"));
    Ok(())
}

#[test]
fn a_lost_answer_leaves_one_order_and_its_ref_id_returns_it() -> Outcome {
    let mut sim = sim()?;
    sim.apply(Event::Script(Fault::LoseAnswer))?;
    assert_eq!(
        sim.place(&limit("buy", "1", "501", 7)),
        Err(SimError::AnswerLost)
    );
    let held = sim.orders(AGENTIC)?;
    assert_eq!(held.len(), 1);
    let resent = sim.place(&limit("buy", "3", "490", 7))?;
    assert_eq!(
        Some(&resent),
        held.first(),
        "a re-send returns the first order unchanged"
    );
    let elsewhere = OrderRequest {
        account_number: DAY_TRADER.to_owned(),
        ..limit("buy", "1", "501", 7)
    };
    assert_ne!(
        sim.place(&elsewhere)?.id,
        resent.id,
        "a ref_id belongs to one account"
    );
    sim.place(&limit("buy", "1", "501", 8))?;
    assert_eq!(
        sim.orders(AGENTIC)?.len(),
        2,
        "another ref_id is another order"
    );
    Ok(())
}

#[test]
fn gfd_expires_at_the_close_and_gtc_rests() -> Outcome {
    let mut sim = sim()?;
    let day = sim.place(&limit("buy", "2", "501", 1))?;
    let gtc = OrderRequest {
        time_in_force: Some("gtc".to_owned()),
        ..limit("buy", "1", "501", 2)
    };
    let gtc = sim.place(&gtc)?;
    sim.fill(&day.id, qty("1"), price("500"))?;
    sim.apply(Event::EndOfDay)?;
    let states: Vec<_> = sim
        .orders(AGENTIC)?
        .into_iter()
        .map(|o| (o.id, o.state, o.filled_quantity))
        .collect();
    let expected = vec![
        (gtc.id, State::Confirmed, qty("0")),
        (day.id, State::Cancelled, qty("1")),
    ];
    assert_eq!(states, expected, "newest first; a cancel keeps what filled");
    Ok(())
}

#[test]
fn an_order_fills_only_in_a_session_its_market_hours_admit() -> Outcome {
    let mut sim = sim()?;
    sim.apply(Event::Session(Session::Extended))?;
    let regular = sim.place(&limit("buy", "1", "501", 1))?;
    let extended = OrderRequest {
        market_hours: Some("extended_hours".to_owned()),
        ..limit("buy", "2", "501", 2)
    };
    let extended = sim.place(&extended)?;
    assert_eq!(
        (regular.state, extended.state),
        (State::Queued, State::Confirmed)
    );
    assert_eq!(
        sim.fill(&regular.id, qty("1"), price("500")),
        Err(SimError::NotWorking)
    );
    sim.fill(&extended.id, qty("1"), price("500"))?;
    sim.apply(Event::Session(Session::Overnight))?;
    let still = sim
        .orders(AGENTIC)?
        .into_iter()
        .find(|o| o.id == regular.id);
    assert_eq!(
        still.map(|o| o.state),
        Some(State::Queued),
        "overnight does not admit regular hours"
    );
    assert_eq!(
        sim.fill(&extended.id, qty("1"), price("500")),
        Err(SimError::OutsideSession)
    );
    let all_day = OrderRequest {
        market_hours: Some("all_day_hours".to_owned()),
        time_in_force: Some("gtc".to_owned()),
        ..limit("buy", "2", "501", 3)
    };
    let all_day = sim.place(&all_day)?;
    sim.fill(&all_day.id, qty("1"), price("500"))?;
    sim.apply(Event::Session(Session::Closed))?;
    assert_eq!(
        sim.fill(&all_day.id, qty("1"), price("500")),
        Err(SimError::OutsideSession)
    );
    sim.apply(Event::Session(Session::Regular))?;
    assert_eq!(
        sim.fill(&regular.id, qty("1"), price("500"))?.state,
        State::Filled
    );
    Ok(())
}

#[test]
fn cancels_fills_and_sells_are_refused_where_the_contract_refuses() -> Outcome {
    let mut sim = sim()?;
    let order = sim.place(&limit("buy", "2", "501", 1))?;
    assert_eq!(
        sim.fill(&order.id, qty("1"), price("501.01")),
        Err(SimError::ThroughLimit)
    );
    assert_eq!(
        sim.fill(&order.id, qty("3"), price("500")),
        Err(SimError::Overfill)
    );
    assert_eq!(
        sim.fill(&order.id, qty("0"), price("500")),
        Err(SimError::Overfill)
    );
    assert_eq!(
        sim.cancel(NOT_AGENTIC, &order.id),
        Err(SimError::UnknownOrder)
    );
    assert_eq!(
        sim.place(&limit("sell", "1", "499", 2)),
        Err(SimError::InsufficientShares)
    );
    sim.fill(&order.id, qty("1"), price("500"))?;
    let sell = sim.place(&limit("sell", "1", "499", 4))?;
    assert_eq!(
        sim.fill(&sell.id, qty("1"), price("498.99")),
        Err(SimError::ThroughLimit)
    );
    let second = limit("sell", "1", "499", 5);
    let oversold = sim.place(&second);
    assert_eq!(
        oversold,
        Err(SimError::InsufficientShares),
        "the working sell holds the one share"
    );
    let other = sim.place(&OrderRequest {
        symbol: "QQQ".to_owned(),
        ..limit("buy", "1", "501", 6)
    })?;
    sim.fill(&other.id, qty("1"), price("500"))?;
    let other_sell = OrderRequest {
        symbol: "QQQ".to_owned(),
        ..limit("sell", "1", "499", 7)
    };
    sim.place(&other_sell)?;
    sim.cancel(AGENTIC, &sell.id)?;
    sim.place(&second)?;
    let cancelled = sim.cancel(AGENTIC, &order.id)?;
    assert_eq!(
        (cancelled.state, cancelled.filled_quantity),
        (State::Cancelled, qty("1"))
    );
    assert_eq!(sim.cancel(AGENTIC, &order.id), Err(SimError::Terminal));
    assert_eq!(
        sim.advance(&order.id, State::Voided),
        Err(SimError::Terminal)
    );
    assert_eq!(
        sim.place(&limit("buy", "1", "501", 1))?.id,
        order.id,
        "the ref_id is spent"
    );
    Ok(())
}

#[test]
fn scripted_answers_and_broker_changes_follow_the_lifecycle() -> Outcome {
    let mut sim = sim()?;
    sim.apply(Event::Script(Fault::Answer(State::Rejected)))?;
    assert_eq!(
        sim.place(&limit("buy", "1", "501", 1))?.state,
        State::Rejected
    );
    let filled = sim.apply(Event::Script(Fault::Answer(State::Filled)));
    assert_eq!(filled, Err(SimError::IllegalTransition));
    sim.apply(Event::Script(Fault::Answer(State::Unconfirmed)))?;
    let order = sim.place(&limit("buy", "1", "501", 2))?;
    assert_eq!(
        sim.fill(&order.id, qty("1"), price("500")),
        Err(SimError::NotWorking)
    );
    let fill_by_advance = sim.advance(&order.id, State::Filled);
    assert_eq!(fill_by_advance, Err(SimError::IllegalTransition));
    assert_eq!(
        sim.advance(&order.id, State::Confirmed)?.state,
        State::Confirmed
    );
    assert_eq!(sim.advance(&order.id, State::Voided)?.state, State::Voided);
    assert_eq!(
        sim.advance("rh-sim-missing", State::Voided),
        Err(SimError::UnknownOrder)
    );
    Ok(())
}

#[test]
fn a_ref_id_is_echoed_and_a_changed_resend_refused_only_when_switched_on() -> Outcome {
    let mut sim = sim()?;
    let order = sim.place(&limit("buy", "1", "501", 1))?;
    let echoed = |sim: &Sim| -> Result<Option<String>, SimError> {
        Ok(sim.orders(AGENTIC)?.first().and_then(|o| o.ref_id.clone()))
    };
    assert_eq!(
        (order.ref_id.clone(), echoed(&sim)?),
        (None, None),
        "not echoed by default"
    );
    sim.apply(Event::EchoRefId(true))?;
    assert_eq!(echoed(&sim)?, Some(ref_id(1)));
    let changed = limit("buy", "2", "501", 1);
    assert_eq!(
        sim.place(&changed)?.id,
        order.id,
        "a changed re-send returns the first by default"
    );
    sim.apply(Event::RefuseChangedResend(true))?;
    assert_eq!(sim.place(&changed), Err(SimError::ChangedResend));
    assert_eq!(
        sim.place(&limit("buy", "1", "501", 1))?.id,
        order.id,
        "the same body still returns it"
    );
    assert_eq!(sim.orders(AGENTIC)?.len(), 1);
    Ok(())
}

#[test]
fn a_working_sell_reserves_only_its_unfilled_remainder() -> Outcome {
    let mut sim = sim()?;
    let bought = sim.place(&limit("buy", "3", "501", 1))?;
    sim.fill(&bought.id, qty("3"), price("500"))?;
    let first = sim.place(&limit("sell", "2", "499", 2))?;
    sim.fill(&first.id, qty("1"), price("500"))?;
    let rest = sim.place(&limit("sell", "1", "499", 3))?;
    assert_eq!(
        rest.quantity,
        qty("1"),
        "two held, one of them reserved by the working sell"
    );
    let more = sim.place(&limit("sell", "1", "499", 4));
    assert_eq!(
        more,
        Err(SimError::InsufficientShares),
        "nothing is left unreserved"
    );
    Ok(())
}
