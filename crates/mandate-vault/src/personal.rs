//! The personal-data vault's references (DEC-2339): the opaque `pii_<ULID>` an executor mints for a
//! connection's account entry, which `ConnectionChecked.account_pii_ref` journals and nothing
//! derives the account id from (journal spec §6.4). The randomness is the caller's source, the
//! operating system's ([`OsRandom`]) in a process and a fixed one in a test.

use mandate_connections::record::AccountPiiRef;
use mandate_time::UtcNanos;

use crate::SourceError;

/// Where a reference's 80 random bits come from.
pub trait PiiRandom {
    /// Fills `bytes` from the source.
    ///
    /// # Errors
    /// [`SourceError::Unavailable`] when the source cannot.
    fn fill(&mut self, bytes: &mut [u8; 10]) -> Result<(), SourceError>;
}

/// The operating system's generator.
#[derive(Debug, Default)]
pub struct OsRandom;

impl PiiRandom for OsRandom {
    fn fill(&mut self, bytes: &mut [u8; 10]) -> Result<(), SourceError> {
        let _ = bytes;
        Err(SourceError::Unimplemented { story: "E21-32" })
    }
}

/// A fresh reference at `now`: its ULID is `now`'s whole millisecond in its 48 high bits and 80
/// bits from `random` in its low ones. Every timestamp's millisecond, the last of 9999 included,
/// fits 48 bits.
///
/// # Errors
/// [`SourceError::Unavailable`] when `random` cannot fill.
pub fn mint_pii_ref(
    now: UtcNanos,
    random: &mut dyn PiiRandom,
) -> Result<AccountPiiRef, SourceError> {
    let _ = (now, random);
    Err(SourceError::Unimplemented { story: "E21-32" })
}
