//! The personal-data vault's references (DEC-2339): `pii_` and a ULID, its time the minting
//! millisecond and its 80 low bits the caller's random source.

use std::collections::BTreeSet;

use mandate_canon::{decode_ulid, is_ulid};
use mandate_time::UtcNanos;

use crate::SourceError;
use crate::personal::{OsRandom, PiiRandom, mint_pii_ref};

fn at(secs: i64, nanos: u32) -> UtcNanos {
    UtcNanos::from_parts(secs, nanos).unwrap()
}

/// A source that answers `bytes` every time, or fails.
struct Fixed(Option<[u8; 10]>);

impl PiiRandom for Fixed {
    fn fill(&mut self, bytes: &mut [u8; 10]) -> Result<(), SourceError> {
        *bytes = self.0.ok_or(SourceError::Unavailable)?;
        Ok(())
    }
}

fn minted(secs: i64, nanos: u32, random: [u8; 10]) -> Result<String, SourceError> {
    mint_pii_ref(at(secs, nanos), &mut Fixed(Some(random))).map(|p| p.as_str().to_owned())
}

/// The ULID's 48 high bits.
fn millis(pii_ref: &str) -> u128 {
    decode_ulid(pii_ref.strip_prefix("pii_").unwrap()).unwrap() >> 80
}

/// A reference is `pii_` and the ULID of the millisecond and the source's 80 bits, most
/// significant first, as the journal's `PiiRef` type reads it.
#[test]
fn a_reference_is_pii_and_the_ulid_of_its_millisecond_and_random_bits() {
    let random = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x10, 0x32];
    let text = minted(1_760_000_000, 123_456_789, random).unwrap();
    assert!(is_ulid(text.strip_prefix("pii_").unwrap()), "{text}");
    let value = decode_ulid(&text[4..]).unwrap();
    assert_eq!(value >> 80, 1_760_000_000_123);
    assert_eq!(value & ((1 << 80) - 1), 0x0123_4567_89ab_cdef_1032);
    assert_eq!(
        minted(0, 0, [0; 10]),
        Ok("pii_00000000000000000000000000".to_owned())
    );
    assert_eq!(
        minted(0, 999_999, [0xff; 10]),
        Ok("pii_0000000000ZZZZZZZZZZZZZZZZ".to_owned())
    );
}

/// The millisecond is the time's whole milliseconds, never rounded up, and the last time a
/// timestamp holds, the end of 9999, fits the ULID's 48 bits.
#[test]
fn the_millisecond_is_truncated_and_every_time_fits() {
    for (secs, nanos, want) in [
        (1, 0, 1_000),
        (1, 999_999, 1_000),
        (1, 1_000_000, 1_001),
        (2, 999_999_999, 2_999),
        (253_402_300_799, 999_999_999, 253_402_300_799_999),
    ] {
        let pii_ref = minted(secs, nanos, [0; 10]).unwrap();
        assert_eq!(millis(&pii_ref), want, "{secs}.{nanos:09}");
    }
}

/// A source that cannot fill mints nothing.
#[test]
fn a_failed_source_mints_nothing() {
    assert_eq!(
        mint_pii_ref(at(1, 0), &mut Fixed(None)).map(|p| p.as_str().to_owned()),
        Err(SourceError::Unavailable)
    );
}

/// A time before the epoch has no millisecond the ULID's unsigned 48 bits can hold, so it mints
/// nothing, rather than a reference at its distance from the epoch.
#[test]
#[ignore = "pending E21-32"]
fn a_time_before_the_epoch_mints_nothing() {
    for (secs, nanos) in [(-1, 0), (-1, 999_999_999), (-62_135_596_800, 0)] {
        assert_eq!(
            minted(secs, nanos, [0; 10]),
            Err(SourceError::Malformed),
            "{secs}.{nanos:09}"
        );
    }
    assert!(minted(0, 0, [0; 10]).is_ok());
}

/// The operating system's references are in the form, at their millisecond, and no two of many
/// are alike.
#[test]
fn the_operating_systems_references_are_fresh() {
    let now = at(1_760_000_000, 5_000_000);
    let minted: BTreeSet<String> = (0..64)
        .map(|_| mint_pii_ref(now, &mut OsRandom).map(|p| p.as_str().to_owned()))
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(minted.len(), 64);
    for pii_ref in &minted {
        assert_eq!(millis(pii_ref), 1_760_000_000_005, "{pii_ref}");
    }
}
