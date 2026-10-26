//! The fee reservation for one order (DEC-2380 item 5).

use mandate_accounting::{AssetClass, InstrumentId, ProspectiveOrder, Side, fee_reservation};
use mandate_gate_inputs::{CALENDAR_MEMBERS, artifact_from, fee_artifact, fees, reservation};
use mandate_num::{Price, Qty, Usd};
use mandate_time::UtcNanos;

const FEE: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/config/fee-config.json");
const CALENDAR: &str =
    include_str!("../../mandate-shell/tests/fixtures/tracer/config/trading-calendar.json");

fn text<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

#[test]
fn the_reservation_is_mandate_accountings() -> Result<(), String> {
    let calendar =
        artifact_from(CALENDAR.trim().as_bytes().to_vec(), CALENDAR_MEMBERS).map_err(text)?;
    let fee = fee_artifact(FEE.trim().as_bytes().to_vec()).map_err(text)?;
    let config = fees(&fee, &calendar).map_err(text)?;
    let instrument = InstrumentId::new("SPY").map_err(text)?;
    let qty = Qty::parse("3").map_err(text)?;
    let limit = Price::parse("100.05").map_err(text)?;
    let at = UtcNanos::parse_rfc3339("2026-09-28T17:00:00Z").map_err(text)?;
    for side in [Side::Buy, Side::Sell] {
        let expected = fee_reservation(
            &ProspectiveOrder {
                instrument: instrument.clone(),
                asset_class: AssetClass::UsEquity,
                side,
                qty,
                limit,
                at,
            },
            &config,
        )
        .map_err(text)?;
        assert_eq!(
            reservation(&config, &instrument, side, qty, limit, at).map_err(text)?,
            expected
        );
    }
    let buy = reservation(&config, &instrument, Side::Buy, qty, limit, at).map_err(text)?;
    assert!(buy > Usd::ZERO, "a buy reserves its fees");
    Ok(())
}
