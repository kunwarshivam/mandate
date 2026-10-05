//! The fee reservation an order carries into the buying-power check (trading-domain spec §2.1, §7.2,
//! §9.5; DEC-129 item 16): `round(estimated fees, 2, ceiling)` per order.

use mandate_num::{Price, Qty, Rounding, Usd};
use mandate_time::UtcNanos;

use crate::{
    Account, AccountType, AccountingError, AssetClass, Config, Execution, Input, InstrumentId, Side,
};

/// §2.1's rounding of a fee reservation: cents, never below the estimate.
const RESERVATION_SCALE: u32 = 2;
/// The id of the one fill a reservation's estimate folds; it names no broker fill.
const PROSPECTIVE_FILL: &str = "fee-reservation";

/// An order not yet sent, as the reservation prices it: as if it filled whole at its limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProspectiveOrder {
    pub instrument: InstrumentId,
    pub asset_class: AssetClass,
    pub side: Side,
    pub qty: Qty,
    pub limit: Price,
    pub at: UtcNanos,
}

/// `round(estimated fees, 2, ceiling)` for `order` under `config`.
///
/// An equity buy's estimate is the fee [`Account::apply`] accrues when the order is folded, as one
/// fill at its limit, into an empty margin account, so the fee rule is the fold's own rather than a
/// second copy. A crypto buy reserves zero, since its fee is paid in the asset (§7.2). A sell
/// reserves zero: only an opening or increasing order meets the buying-power check (§9.5), v1 has
/// no short sales, and a reservation that could fail to compute must never stand between a sell and
/// the gate (`AGENTS.md` rule 13).
///
/// # Errors
/// The fold's own error for an equity buy it cannot price, such as a zero quantity.
pub fn fee_reservation(order: &ProspectiveOrder, config: &Config) -> Result<Usd, AccountingError> {
    match (order.side, order.asset_class) {
        (Side::Sell, _) | (Side::Buy, AssetClass::Crypto) => Ok(Usd::ZERO),
        (Side::Buy, AssetClass::UsEquity) => {
            let fill = Input::Fill(Execution {
                fill_id: PROSPECTIVE_FILL.to_owned(),
                client_order_id: None,
                instrument: order.instrument.clone(),
                asset_class: order.asset_class,
                side: order.side,
                qty_gross: order.qty,
                price: order.limit,
                liquidity: None,
                executed_at: order.at,
            });
            let priced =
                Account::opening(AccountType::Margin, Usd::ZERO, []).apply(&fill, config)?;
            Ok(priced
                .account
                .fees_accrued()?
                .round(RESERVATION_SCALE, Rounding::Ceiling)?)
        }
    }
}
