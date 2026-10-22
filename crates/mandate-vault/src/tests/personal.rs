//! The personal-data vault's references (DEC-2339): `pii_` and a ULID, its time the minting
//! millisecond and its randomness the operating system's.

use std::collections::BTreeSet;

use mandate_canon::{decode_ulid, is_ulid};
use mandate_time::UtcNanos;

use crate::personal::{mint_pii_ref, pii_ref_of};

fn at(secs: i64, nanos: u32) -> UtcNanos {
    UtcNanos::from_parts(secs, nanos).unwrap()
}

/// The ULID's 48 high bits.
fn millis(pii_ref: &str) -> u128 {
    decode_ulid(pii_ref.strip_prefix("pii_").unwrap()).unwrap() >> 80
}

/// A reference is `pii_` and the ULID of the millisecond and the 80 random bits, most significant
/// first, as the journal's `PiiRef` type reads it.
#[test]
#[ignore = "pending E21-32"]
fn a_reference_is_pii_and_the_ulid_of_its_millisecond_and_random_bits() {
    let random = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x10, 0x32];
    let pii_ref = pii_ref_of(at(1_760_000_000, 123_456_789), random).unwrap();
    let text = pii_ref.as_str();
    assert!(is_ulid(text.strip_prefix("pii_").unwrap()), "{text}");
    let value = decode_ulid(&text[4..]).unwrap();
    assert_eq!(value >> 80, 1_760_000_000_123);
    assert_eq!(value & ((1 << 80) - 1), 0x0123_4567_89ab_cdef_1032);
    assert_eq!(
        pii_ref_of(at(0, 0), [0; 10]).map(|p| p.as_str().to_owned()),
        Ok("pii_00000000000000000000000000".to_owned())
    );
    assert_eq!(
        pii_ref_of(at(0, 999_999), [0xff; 10]).map(|p| p.as_str().to_owned()),
        Ok("pii_0000000000ZZZZZZZZZZZZZZZZ".to_owned())
    );
}

/// The millisecond is the time's whole milliseconds, never rounded up.
#[test]
#[ignore = "pending E21-32"]
fn the_millisecond_is_truncated() {
    for (secs, nanos, want) in [
        (1, 0, 1_000),
        (1, 999_999, 1_000),
        (1, 1_000_000, 1_001),
        (2, 999_999_999, 2_999),
    ] {
        let pii_ref = pii_ref_of(at(secs, nanos), [0; 10]).unwrap();
        assert_eq!(millis(pii_ref.as_str()), want, "{secs}.{nanos:09}");
    }
}

/// The last time a timestamp holds, 9999-12-31T23:59:59.999999999Z, fits the ULID's 48-bit
/// millisecond, so no time is ever wrapped.
#[test]
#[ignore = "pending E21-32"]
fn the_last_time_fits_the_ulids_millisecond() {
    let last = at(253_402_300_799, 999_999_999);
    assert_eq!(
        pii_ref_of(last, [0; 10]).map(|p| millis(p.as_str())),
        Ok(253_402_300_799_999)
    );
}

/// A minted reference is in the form, at its millisecond, and no two of many are alike.
#[test]
#[ignore = "pending E21-32"]
fn minted_references_are_fresh() {
    let now = at(1_760_000_000, 5_000_000);
    let minted: BTreeSet<String> = (0..64)
        .map(|_| mint_pii_ref(now).map(|p| p.as_str().to_owned()))
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(minted.len(), 64);
    for pii_ref in &minted {
        assert_eq!(millis(pii_ref), 1_760_000_000_005, "{pii_ref}");
    }
}
