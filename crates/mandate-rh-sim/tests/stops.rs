//! Stop orders in the simulated broker (E7-25; [DEC-966](../../../docs/project/decisions/DEC-966.md)
//! items 1 and 6): what Robinhood's contract lets protection be. A `stop_market` or `stop_limit`
//! order rests until the market reaches its stop; only then can it fill, a stop-limit no worse
//! than its limit, in part or in whole. With no OCO in the contract, a take-profit sell beside a
//! stop for the whole position would sell it twice, and the broker refuses it.

mod common;

use common::{AGENTIC, price, qty, ref_id, sim};
use mandate_rh_sim::{Event, OrderRequest, Sim, SimError, State};

type Outcome = Result<(), SimError>;

/// A `gtc` SPY sell on the agentic account: `order_type` for `quantity`, with `stop` and `limit`.
fn sell(
    order_type: &str,
    quantity: &str,
    stop: Option<&str>,
    limit: Option<&str>,
    n: u32,
) -> OrderRequest {
    OrderRequest {
        account_number: AGENTIC.to_owned(),
        symbol: "SPY".to_owned(),
        side: "sell".to_owned(),
        order_type: order_type.to_owned(),
        quantity: Some(quantity.to_owned()),
        stop_price: stop.map(str::to_owned),
        limit_price: limit.map(str::to_owned),
        time_in_force: Some("gtc".to_owned()),
        ref_id: Some(ref_id(n)),
        ..OrderRequest::default()
    }
}

/// The simulator with 2 SPY held, bought at 500 by a filled limit buy.
fn holding_two() -> Result<Sim, SimError> {
    let mut sim = sim()?;
    let buy = sim.place(&common::limit("buy", "2", "500", 1))?;
    sim.fill(&buy.id, qty("2"), price("500"))?;
    Ok(sim)
}

fn quote(sim: &mut Sim, at: &str) -> Outcome {
    sim.apply(Event::Quote("SPY".to_owned(), price(at)))
}

/// A stop-limit sell at stop 480, limit 475 does not fill while SPY quotes above 480; once a quote
/// reaches the stop it fills in part and then in whole, never below 475, and the position goes to
/// zero. A triggered stop stays triggered when the quote recovers.
#[test]
fn a_stop_limit_fills_only_once_triggered_and_never_through_its_limit() -> Outcome {
    let mut sim = holding_two()?;
    let stop = sim.place(&sell("stop_limit", "2", Some("480"), Some("475"), 2))?;
    assert_eq!(stop.state, State::Confirmed, "it rests");
    assert_eq!(
        sim.fill(&stop.id, qty("1"), price("490")),
        Err(SimError::NotTriggered),
        "SPY at 500 has not reached the stop"
    );
    quote(&mut sim, "481")?;
    assert_eq!(
        sim.fill(&stop.id, qty("1"), price("481")),
        Err(SimError::NotTriggered)
    );
    quote(&mut sim, "480")?;
    quote(&mut sim, "486")?;
    let part = sim.fill(&stop.id, qty("1"), price("476"))?;
    assert_eq!(
        part.state,
        State::PartiallyFilled,
        "a triggered stop fills in part"
    );
    assert_eq!(
        sim.fill(&stop.id, qty("1"), price("474.99")),
        Err(SimError::ThroughLimit),
        "never below its limit"
    );
    let done = sim.fill(&stop.id, qty("1"), price("475"))?;
    assert_eq!(done.state, State::Filled);
    assert_eq!(sim.position(AGENTIC, "SPY")?, qty("0"));
    Ok(())
}

/// A stop-market sell placed while the quote is already through its stop is triggered at once and
/// fills at any price.
#[test]
fn a_stop_placed_through_its_stop_is_triggered_at_once() -> Outcome {
    let mut sim = holding_two()?;
    quote(&mut sim, "470")?;
    let stop = sim.place(&sell("stop_market", "2", Some("480"), None, 2))?;
    let done = sim.fill(&stop.id, qty("2"), price("460"))?;
    assert_eq!(done.state, State::Filled);
    Ok(())
}

/// No OCO: a take-profit limit sell beside a stop that already covers the whole 2 is refused, so
/// the two can never both fill; once the stop is cancelled, the take-profit is accepted.
#[test]
fn a_take_profit_beside_a_whole_position_stop_is_refused() -> Outcome {
    let mut sim = holding_two()?;
    let stop = sim.place(&sell("stop_limit", "2", Some("480"), Some("475"), 2))?;
    assert_eq!(
        sim.place(&sell("limit", "2", None, Some("520"), 3)),
        Err(SimError::InsufficientShares),
        "the position is already wholly committed to the stop"
    );
    assert_eq!(
        sim.place(&sell("limit", "1", None, Some("520"), 4)),
        Err(SimError::InsufficientShares)
    );
    sim.cancel(AGENTIC, &stop.id)?;
    let take = sim.place(&sell("limit", "2", None, Some("520"), 5))?;
    assert_eq!(take.state, State::Confirmed);
    Ok(())
}

/// A stop the broker rejects or cancels never fills, triggered or not.
#[test]
fn a_rejected_or_cancelled_stop_never_fills() -> Outcome {
    let mut sim = holding_two()?;
    sim.apply(Event::Script(mandate_rh_sim::Fault::Answer(
        State::Rejected,
    )))?;
    let rejected = sim.place(&sell("stop_limit", "2", Some("480"), Some("475"), 2))?;
    assert_eq!(rejected.state, State::Rejected);
    let stop = sim.place(&sell("stop_limit", "2", Some("480"), Some("475"), 3))?;
    sim.cancel(AGENTIC, &stop.id)?;
    quote(&mut sim, "479")?;
    for id in [&rejected.id, &stop.id] {
        assert_eq!(
            sim.fill(id, qty("2"), price("479")),
            Err(SimError::Terminal)
        );
    }
    Ok(())
}

#[test]
fn the_untriggered_refusal_names_itself() {
    assert_eq!(
        SimError::NotTriggered.to_string(),
        "the market has not reached the order's stop"
    );
}

/// A buy stop triggers at or above its stop: a `stop_market` buy at 520 does not fill while SPY
/// quotes 519, and fills once a quote reaches 520; one placed while SPY already quotes above its
/// stop is triggered at once.
#[test]
fn a_buy_stop_triggers_at_or_above_its_stop() -> Outcome {
    let mut sim = sim()?;
    let buy = |stop: &str, n: u32| OrderRequest {
        side: "buy".to_owned(),
        ..sell("stop_market", "1", Some(stop), None, n)
    };
    let waiting = sim.place(&buy("520", 1))?;
    quote(&mut sim, "519")?;
    assert_eq!(
        sim.fill(&waiting.id, qty("1"), price("519")),
        Err(SimError::NotTriggered)
    );
    quote(&mut sim, "520")?;
    assert_eq!(
        sim.fill(&waiting.id, qty("1"), price("521"))?.state,
        State::Filled
    );
    let through = sim.place(&buy("510", 2))?;
    assert_eq!(
        sim.fill(&through.id, qty("1"), price("521"))?.state,
        State::Filled
    );
    Ok(())
}
