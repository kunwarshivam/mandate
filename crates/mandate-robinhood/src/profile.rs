//! Robinhood's capability profile: trading spec §5.2's Robinhood table as data (DEC-531,
//! DEC-630, DEC-860 item 5).

use mandate_domain::{
    AssetClass, CapabilityProfile, Cell, Idempotency, MarketSession, OrderType, ProfileError,
    ProtectionForm, QuantityForm, Retry, Row, TimeInForce,
};

/// Version 1, US equities in the regular session, as DEC-860 item 5 reads §5.2: `market` in whole,
/// fractional and notional quantities; `limit`, `stop` and `stop_limit` in whole shares only;
/// `day` (`gfd`) and `gtc` in every cell; the whole-share stop-limit the one protective form, with
/// no OCO and no bracket. `ref_id` is sent, what a retry returns is unknown, and no order is read
/// back by it (U-R1, U-R2), so a lost answer is never re-sent blindly (DEC-529 item 4).
pub fn robinhood() -> Result<CapabilityProfile, ProfileError> {
    let cell = |order_type, quantity_form, protection_forms: &[ProtectionForm]| Cell {
        order_type,
        quantity_form,
        times_in_force: [TimeInForce::Day, TimeInForce::Gtc].into(),
        protection_forms: protection_forms.iter().copied().collect(),
    };
    let cells = vec![
        cell(OrderType::Market, QuantityForm::Whole, &[]),
        cell(OrderType::Market, QuantityForm::Fractional, &[]),
        cell(OrderType::Market, QuantityForm::Notional, &[]),
        cell(OrderType::Limit, QuantityForm::Whole, &[]),
        cell(OrderType::Stop, QuantityForm::Whole, &[]),
        cell(
            OrderType::StopLimit,
            QuantityForm::Whole,
            &[ProtectionForm::StopLimit],
        ),
    ];
    let rows = vec![Row {
        asset_class: AssetClass::UsEquity,
        session: MarketSession::Regular,
        cells,
    }];
    let idempotency = Idempotency {
        client_order_id: true,
        retry: Retry::Unknown,
        query_by_client_order_id: false,
    };
    CapabilityProfile::new(1, rows, idempotency)
}
