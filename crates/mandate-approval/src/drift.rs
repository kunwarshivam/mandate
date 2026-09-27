//! The drift band (E8-3, DEC-156 item 3): `|m_now − m_req| × 10 000 ≤ band_bp × m_req`, in exact
//! decimals, with no division.

use mandate_num::Price;

use crate::ApprovalError;
use crate::content::AssetClass;

/// 100 bp for `us_equity` and 200 bp for `crypto`: the smallest collar aggressiveness `x` of each
/// class in trading-domain spec §9.6 (1% and 2%), so the band is never looser than the gate's.
///
/// # Errors
/// [`ApprovalError::Unimplemented`] until E8-3.
pub fn band_bp(class: AssetClass) -> Result<u32, ApprovalError> {
    let _ = class;
    Err(ApprovalError::Unimplemented { story: "E8-3" })
}

/// Whether the mark moved no further than the band since the request. A missing mark at either end
/// is outside it (fail closed).
///
/// # Errors
/// [`ApprovalError::Unimplemented`] until E8-3.
pub fn within_band(
    m_req: Option<Price>,
    m_now: Option<Price>,
    class: AssetClass,
) -> Result<bool, ApprovalError> {
    let _ = (m_req, m_now, class);
    Err(ApprovalError::Unimplemented { story: "E8-3" })
}
