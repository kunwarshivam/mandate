//! The protective order the executor places on a position, read from the broker's capability
//! profile and nothing else (E7-23 B2a, DEC-531 item 2, DEC-838, trading-domain spec §5.2 and
//! §5.4).
//!
//! Shared code never names a broker and never reads the asset class to learn a broker rule: the
//! profile says which forms an order of each type and quantity form may be sent as, and this
//! module takes the strongest one the executor can place on a position that already exists.

use mandate_domain::{
    AssetClass, CapabilityProfile, Cell, Idempotency, MarketSession, OrderType as CellType,
    ProfileError, ProtectionForm, QuantityForm, Retry, Row, TimeInForce as CellTif,
};
use mandate_num::{Adverse, Fraction, Qty, ShareIncrement};

use crate::error::ExecutorError;
use crate::types::{OcoLegs, OrderType, ProtectionPrices, TimeInForce};

/// One protective sell, as the executor would send it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectiveShape {
    pub form: ProtectionForm,
    pub order_type: OrderType,
    pub tif: TimeInForce,
    pub limit_price: Option<mandate_num::Price>,
    pub stop_price: Option<mandate_num::Price>,
    pub oco: Option<OcoLegs>,
}

/// The strongest protective form `profile` offers for `qty` in `asset_class`, as the
/// order to send, or `None` when it offers none the executor can place on a position.
///
/// The order of strength is OCO, then one resting stop-limit (DEC-838 item 1; §5.1, §5.4). A
/// bracket is the entry's, never placed on a position that exists. An OCO needs a take-profit. The
/// stop-limit needs the mandate's `stop_limit_offset` (limit = stop x (1 - offset), DEC-539) and
/// its take-profit is the runtime's, never a leg. A form is offered only when the cell for the
/// order's own type and quantity form lists it and lists `gtc` (item 2); the quantity form is
/// whole for a whole `qty` and fractional otherwise (item 3). An unlisted cell offers nothing
/// (DEC-630 item 9).
///
/// The row read is the protective order's own session, never `clock_session`: a US equity reads
/// its regular-session row and crypto its crypto row, since a GTC protective order rests and
/// triggers in the regular session (§5.4; DEC-838 item 4). Re-placement runs in pre-market and
/// after the close, and must not lose its protection to the clock.
pub fn protective_shape(
    profile: &CapabilityProfile,
    asset_class: AssetClass,
    _clock_session: MarketSession,
    qty: Qty,
    prices: ProtectionPrices,
    stop_limit_offset: Option<Fraction>,
) -> Result<Option<ProtectiveShape>, ExecutorError> {
    let session = resting_session(asset_class);
    let quantity_form = if qty.portion(Fraction::ONE, ShareIncrement::Whole)? == qty {
        QuantityForm::Whole
    } else {
        QuantityForm::Fractional
    };
    let offered = |order_type: CellType, form: ProtectionForm| {
        profile
            .cell(asset_class, session, order_type, quantity_form)
            .is_ok_and(|cell| {
                cell.protection_forms.contains(&form) && cell.times_in_force.contains(&CellTif::Gtc)
            })
    };
    if let Some(take_profit) = prices.take_profit
        && offered(CellType::Limit, ProtectionForm::Oco)
    {
        return Ok(Some(ProtectiveShape {
            form: ProtectionForm::Oco,
            order_type: OrderType::Limit,
            tif: TimeInForce::Gtc,
            limit_price: None,
            stop_price: None,
            oco: Some(OcoLegs {
                take_profit,
                stop: prices.stop,
                qty,
            }),
        }));
    }
    if let Some(offset) = stop_limit_offset
        && offered(CellType::StopLimit, ProtectionForm::StopLimit)
    {
        return Ok(Some(ProtectiveShape {
            form: ProtectionForm::StopLimit,
            order_type: OrderType::StopLimit,
            tif: TimeInForce::Gtc,
            limit_price: Some(prices.stop.collar_bound(offset, Adverse::Down)?),
            stop_price: Some(prices.stop),
            oco: None,
        }));
    }
    Ok(None)
}

/// The session a GTC protective order rests and triggers in, whatever the clock reads: a US
/// equity's regular session and crypto's continuous one (§5.4, DEC-838 item 4).
fn resting_session(asset_class: AssetClass) -> MarketSession {
    match asset_class {
        AssetClass::UsEquity => MarketSession::Regular,
        AssetClass::Crypto => MarketSession::Crypto,
    }
}

/// The transitional profile of an [`crate::ExecutorState`] built without one: trading spec
/// §5.2's Alpaca table, cell for cell as `mandate-alpaca`'s profile declares it, so its content
/// hash is the same (DEC-838 item 5). Alpaca-only by contract; the story that lands the first
/// non-Alpaca executor path (B3) deletes it before that path merges.
pub(crate) fn transitional_alpaca() -> Result<CapabilityProfile, ProfileError> {
    let cell = |order_type, quantity_form, tifs: &[CellTif], forms: &[ProtectionForm]| Cell {
        order_type,
        quantity_form,
        times_in_force: tifs.iter().copied().collect(),
        protection_forms: forms.iter().copied().collect(),
    };
    let equity = [
        (CellType::Market, &[ProtectionForm::Bracket][..]),
        (
            CellType::Limit,
            &[ProtectionForm::Bracket, ProtectionForm::Oco],
        ),
        (CellType::Stop, &[]),
        (CellType::StopLimit, &[]),
    ]
    .into_iter()
    .flat_map(|(order_type, forms)| {
        [
            cell(
                order_type,
                QuantityForm::Whole,
                &[CellTif::Day, CellTif::Gtc],
                forms,
            ),
            cell(order_type, QuantityForm::Fractional, &[CellTif::Day], &[]),
            cell(order_type, QuantityForm::Notional, &[CellTif::Day], &[]),
        ]
    })
    .collect();
    let crypto = [QuantityForm::Whole, QuantityForm::Fractional]
        .map(|form| {
            cell(
                CellType::StopLimit,
                form,
                &[CellTif::Gtc],
                &[ProtectionForm::StopLimit],
            )
        })
        .into();
    let rows = vec![
        Row {
            asset_class: AssetClass::UsEquity,
            session: MarketSession::Regular,
            cells: equity,
        },
        Row {
            asset_class: AssetClass::Crypto,
            session: MarketSession::Crypto,
            cells: crypto,
        },
    ];
    let idempotency = Idempotency {
        client_order_id: true,
        retry: Retry::Idempotent,
        query_by_client_order_id: true,
    };
    CapabilityProfile::new(1, rows, idempotency)
}
