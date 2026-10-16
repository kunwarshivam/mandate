//! The accounts, quote and requests every test starts from.

#![allow(
    dead_code,
    reason = "each test binary uses a different part of the shared fixtures"
)]

pub mod wire;

use mandate_accounting::InstrumentId;
use mandate_num::{Price, Qty, Usd};
use mandate_rh_sim::{Account, Event, OrderRequest, Session, Sim, SimError};
use mandate_robinhood::Tickers;

pub const AGENTIC: &str = "5QR00001";
pub const NOT_AGENTIC: &str = "5QR00002";
pub const DAY_TRADER: &str = "5QR00003";

/// SPY's asset id, the instrument id the executor names it by (mandate spec §3); Robinhood names
/// it by its ticker, `SPY`.
pub const SPY_ASSET: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";

/// The registered instrument snapshot's pairing of [`SPY_ASSET`] with its symbol, `SPY`
/// (DEC-956 item 1).
pub fn tickers() -> Tickers {
    let asset = InstrumentId::new(SPY_ASSET).unwrap();
    Tickers::registered([(asset, "SPY".to_owned())]).unwrap()
}

pub fn price(text: &str) -> Price {
    Price::parse(text).unwrap()
}

pub fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap()
}

pub fn account(number: &str, agentic_allowed: bool, pattern_day_trader: bool) -> Account {
    let (buying_power, cash) = (Usd::parse("10000").unwrap(), Usd::parse("750.25").unwrap());
    Account {
        number: number.to_owned(),
        agentic_allowed,
        cash,
        buying_power,
        pattern_day_trader,
    }
}

/// Three accounts with 10,000 USD of buying power each, a SPY quote of 500, and the regular
/// session.
pub fn sim() -> Result<Sim, SimError> {
    let mut sim = Sim::new(vec![
        account(AGENTIC, true, false),
        account(NOT_AGENTIC, false, false),
        account(DAY_TRADER, true, true),
    ])?;
    sim.apply(Event::Quote("SPY".to_owned(), price("500")))?;
    sim.apply(Event::Session(Session::Regular))?;
    Ok(sim)
}

pub fn ref_id(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

/// A `gfd`, regular-hours SPY limit order on the agentic account, with `ref_id(n)`.
pub fn limit(side: &str, quantity: &str, limit_price: &str, n: u32) -> OrderRequest {
    OrderRequest {
        account_number: AGENTIC.to_owned(),
        symbol: "SPY".to_owned(),
        side: side.to_owned(),
        order_type: "limit".to_owned(),
        quantity: Some(quantity.to_owned()),
        limit_price: Some(limit_price.to_owned()),
        ref_id: Some(ref_id(n)),
        ..OrderRequest::default()
    }
}
