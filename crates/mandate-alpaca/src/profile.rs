//! Alpaca's capability profile: trading spec §5.2 as data (DEC-531 item 5, DEC-630).
//!
//! Only the rows the first live order reads are declared: US equities in the regular session
//! (DEC-531 item 6). Crypto's row comes with B2a, which moves the executor's crypto stop-limit
//! choice onto the profile.

use mandate_domain::{CapabilityProfile, ProfileError};

/// The profile [`crate::TradingClient`] hands the executor.
pub fn alpaca() -> Result<CapabilityProfile, ProfileError> {
    Err(ProfileError::Unimplemented)
}
