//! The personal-data vault's references (DEC-2339): the opaque `pii_<ULID>` an executor mints for a
//! connection's account entry, which `ConnectionChecked.account_pii_ref` journals and nothing
//! derives the account id from (journal spec §6.4). The randomness is the caller's source, the
//! operating system's ([`OsRandom`]) in a process and a fixed one in a test.

use mandate_canon::encode_ulid;
use mandate_connections::record::AccountPiiRef;
use mandate_time::UtcNanos;
use ring::rand::{SecureRandom, SystemRandom};

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
        SystemRandom::new()
            .fill(bytes)
            .map_err(|_| SourceError::Unavailable)
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
    let mut bits = [0_u8; 10];
    random.fill(&mut bits)?;
    let millis = now
        .secs()
        .unsigned_abs()
        .saturating_mul(1000)
        .saturating_add(u64::from(now.nanos() / 1_000_000))
        .to_be_bytes();
    let mut ulid = [0_u8; 16];
    ulid[..6].copy_from_slice(&millis[2..]);
    ulid[6..].copy_from_slice(&bits);
    AccountPiiRef::new(&format!("pii_{}", encode_ulid(u128::from_be_bytes(ulid))))
        .map_err(|_| SourceError::Malformed)
}
