//! The personal-data vault's references (DEC-2339): the opaque `pii_<ULID>` an executor mints for a
//! connection's account entry, which `ConnectionChecked.account_pii_ref` journals and nothing
//! derives the account id from (journal spec §6.4).

use mandate_connections::record::AccountPiiRef;
use mandate_time::UtcNanos;

use crate::SourceError;

/// The reference whose ULID is `now`'s whole millisecond in its 48 high bits and `random` in its
/// 80 low bits. Every timestamp's millisecond, the last of 9999 included, fits 48 bits.
pub fn pii_ref_of(now: UtcNanos, random: [u8; 10]) -> Result<AccountPiiRef, SourceError> {
    let _ = (now, random);
    Err(SourceError::Unimplemented { story: "E21-32" })
}

/// A fresh reference at `now`, its 80 random bits from the operating system's generator. A
/// generator that fails is [`SourceError::Unavailable`].
pub fn mint_pii_ref(now: UtcNanos) -> Result<AccountPiiRef, SourceError> {
    let _ = now;
    Err(SourceError::Unimplemented { story: "E21-32" })
}
