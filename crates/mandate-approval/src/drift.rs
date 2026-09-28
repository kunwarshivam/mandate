//! The drift band (E8-3, DEC-156 item 3): `|m_now − m_req| × 10 000 ≤ band_bp × m_req`, in exact
//! decimals, with no division.

use mandate_num::{Price, Qty, Usd};

use crate::ApprovalError;
use crate::content::AssetClass;

/// 100 bp for `us_equity` and 200 bp for `crypto`: the smallest collar aggressiveness `x` of each
/// class in trading-domain spec §9.6 (1% and 2%), so the band is never looser than the gate's.
///
/// # Errors
/// None: the `Result` is the stub API's shape.
pub fn band_bp(class: AssetClass) -> Result<u32, ApprovalError> {
    Ok(match class {
        AssetClass::UsEquity => 100,
        AssetClass::Crypto => 200,
    })
}

/// Basis points in one.
const BP_PER_UNIT: u32 = 10_000;

/// Whether the mark moved no further than the band since the request. A missing mark at either end
/// is outside it (fail closed), and so is a move too large for the exact arithmetic to hold.
///
/// # Errors
/// None: the `Result` is the stub API's shape.
pub fn within_band(
    m_req: Option<Price>,
    m_now: Option<Price>,
    class: AssetClass,
) -> Result<bool, ApprovalError> {
    let (Some(m_req), Some(m_now)) = (m_req, m_now) else {
        return Ok(false);
    };
    Ok(drift_and_allowance(m_req, m_now, band_bp(class)?)
        .is_some_and(|(drift, allowance)| drift <= allowance))
}

/// `(|m_now − m_req| × 10 000, band_bp × m_req)`, exact, or `None` when either overflows.
fn drift_and_allowance(m_req: Price, m_now: Price, band: u32) -> Option<(Usd, Usd)> {
    let times = |n: u32, p: Price| Qty::parse(&n.to_string()).ok()?.notional(p).ok();
    let moved = times(BP_PER_UNIT, m_now)?
        .checked_sub(times(BP_PER_UNIT, m_req)?)
        .ok()?;
    let drift = if moved.is_negative() {
        moved.negated()
    } else {
        moved
    };
    Some((drift, times(band, m_req)?))
}

#[cfg(test)]
mod tests {
    use mandate_num::NumError;

    use super::*;

    /// A mark whose `× 10 000` the exact arithmetic cannot hold is outside the band even unmoved,
    /// so an overflow skips as `drift` rather than erring or acting (fail closed, rule 3).
    #[test]
    fn a_move_too_large_to_compute_is_outside_the_band() -> Result<(), NumError> {
        let huge = Price::parse("79228162514264337593543950")?;
        let ordinary = Price::parse("7922816251426433759")?;
        for class in [AssetClass::UsEquity, AssetClass::Crypto] {
            assert_eq!(within_band(Some(huge), Some(huge), class), Ok(false));
            assert_eq!(within_band(Some(ordinary), Some(ordinary), class), Ok(true));
        }
        Ok(())
    }
}
