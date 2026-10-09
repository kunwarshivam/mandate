//! Alpaca's capability profile: trading spec §5.2 as data (DEC-531 item 5, DEC-630).
//!
//! Only the rows the first live order reads are declared: US equities in the regular session
//! (DEC-531 item 6). Crypto's row comes with B2a, which moves the executor's crypto stop-limit
//! choice onto the profile.

use mandate_domain::{
    AssetClass, CapabilityProfile, Cell, Idempotency, MarketSession, OrderType, ProfileError,
    ProtectionForm, QuantityForm, Retry, Row, TimeInForce,
};

/// The profile [`crate::TradingClient`] hands the executor.
///
/// Read from §5.2 and [Alpaca's order documentation](https://docs.alpaca.markets/docs/orders-at-alpaca)
/// as DEC-630 item 1 says, taking the tighter set where the documentation is unclear (DEC-176):
///
/// - Whole shares take `day` or `gtc`.
/// - A cell lists the forms its order may be *sent as*. A market or limit order may be a
///   bracket's entry, and a limit order an OCO's parent ("the type parameter must always be
///   `limit`"). A stop or stop-limit order is neither, since the documentation names no such
///   entry. No equity order is the resting stop-limit, which DEC-36 gives to crypto.
/// - Fractional and notional orders are `day` only and never in an OCO or bracket.
///
/// Our `client_order_id` goes on the wire, a second order with one Alpaca holds is refused
/// (`client::DUPLICATE_CLIENT_ORDER_ID`), and an order is read back by it
/// (`/v2/orders:by_client_order_id`).
pub fn alpaca() -> Result<CapabilityProfile, ProfileError> {
    let cells = [
        (OrderType::Market, &[ProtectionForm::Bracket][..]),
        (
            OrderType::Limit,
            &[ProtectionForm::Bracket, ProtectionForm::Oco],
        ),
        (OrderType::Stop, &[]),
        (OrderType::StopLimit, &[]),
    ]
    .into_iter()
    .flat_map(|(order_type, protection)| {
        [
            Cell {
                order_type,
                quantity_form: QuantityForm::Whole,
                times_in_force: [TimeInForce::Day, TimeInForce::Gtc].into(),
                protection_forms: protection.iter().copied().collect(),
            },
            day_only(order_type, QuantityForm::Fractional),
            day_only(order_type, QuantityForm::Notional),
        ]
    })
    .collect();
    let rows = vec![Row {
        asset_class: AssetClass::UsEquity,
        session: MarketSession::Regular,
        cells,
    }];
    let idempotency = Idempotency {
        client_order_id: true,
        retry: Retry::Idempotent,
        query_by_client_order_id: true,
    };
    CapabilityProfile::new(1, rows, idempotency)
}

fn day_only(order_type: OrderType, quantity_form: QuantityForm) -> Cell {
    Cell {
        order_type,
        quantity_form,
        times_in_force: [TimeInForce::Day].into(),
        protection_forms: [].into(),
    }
}
