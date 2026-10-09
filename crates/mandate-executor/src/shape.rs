//! The protective order the executor places on a position, read from the broker's capability
//! profile and nothing else (E7-23 B2a, DEC-531 item 2, DEC-838, trading-domain spec §5.2 and
//! §5.4).
//!
//! Shared code never names a broker and never reads the asset class to learn a broker rule: the
//! profile says which forms an order of each type and quantity form may be sent as, and this
//! module takes the strongest one the executor can place on a position that already exists.

use mandate_domain::{AssetClass, CapabilityProfile, MarketSession, ProtectionForm};
use mandate_num::{Fraction, Qty};

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
    _profile: &CapabilityProfile,
    _asset_class: AssetClass,
    _clock_session: MarketSession,
    _qty: Qty,
    _prices: ProtectionPrices,
    _stop_limit_offset: Option<Fraction>,
) -> Result<Option<ProtectiveShape>, ExecutorError> {
    Err(ExecutorError::Unimplemented { story: "E7-23" })
}
