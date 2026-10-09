//! Robinhood's capability profile: trading spec §5.2's Robinhood table as data (DEC-531,
//! DEC-630, DEC-860 item 5).

use mandate_domain::{CapabilityProfile, ProfileError};

/// Version 1, one row: US equities in the regular session. Every order type takes `day` (the
/// contract's `gfd`) and `gtc`; a market order may be whole, fractional or notional, and every
/// other type whole shares only; the whole-share stop-limit is the one resting protective form,
/// and nothing is an OCO or a bracket. Each order carries our `ref_id`, which the broker
/// deduplicates by, with what a retry returns unknown and no query by it.
pub fn robinhood() -> Result<CapabilityProfile, ProfileError> {
    Err(ProfileError::Unimplemented)
}
