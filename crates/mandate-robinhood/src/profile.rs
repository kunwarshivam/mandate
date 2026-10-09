//! Robinhood's capability profile: trading spec §5.2's Robinhood table as data (DEC-531,
//! DEC-630, DEC-860 item 5).

use mandate_domain::{CapabilityProfile, ProfileError};

/// Version 1, US equities in the regular session, as DEC-860 item 5 reads §5.2.
pub fn robinhood() -> Result<CapabilityProfile, ProfileError> {
    Err(ProfileError::Unimplemented)
}
