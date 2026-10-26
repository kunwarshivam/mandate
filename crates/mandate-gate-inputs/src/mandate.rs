//! `mandate-accounting`'s fee reservation for one order, moved here from the paper shell unchanged
//! (DEC-2380 item 5). The mandate's limits as the gate reads them are `mandate-ports`'s
//! `gate_mandate`, which the cell's runtime reads too (DEC-2387 item 1).

use mandate_accounting::{
    AccountingError, AssetClass, InstrumentId, ProspectiveOrder, Side, fee_reservation,
};
use mandate_num::{Price, Qty, Usd};
use mandate_time::UtcNanos;

/// `mandate-accounting`'s fee reservation for one equity order on the reviewed instrument.
///
/// # Errors
/// [`AccountingError`] when the fee schedule cannot price the order.
pub fn reservation(
    fees: &mandate_accounting::Config,
    instrument: &InstrumentId,
    side: Side,
    qty: Qty,
    limit: Price,
    at: UtcNanos,
) -> Result<Usd, AccountingError> {
    fee_reservation(
        &ProspectiveOrder {
            instrument: instrument.clone(),
            asset_class: AssetClass::UsEquity,
            side,
            qty,
            limit,
            at,
        },
        fees,
    )
}
