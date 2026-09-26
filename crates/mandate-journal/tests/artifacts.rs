//! Content-addressed artifacts (journal spec §6.3, backlog E5-2) on the in-memory store. Oracles:
//! published SHA-256 vectors for the address, and the stored map read directly for the contents.

mod common;

use std::collections::BTreeMap;

use common::{edit, journal_with, mark_draft, now, stream};
use mandate_canon::Digest;
use mandate_journal::{
    AppendOutcome, ArtifactError, ArtifactRef, ArtifactSource, ArtifactStore, EventCheck,
    EventFailure, TrustedStart, check_artifact, get_artifact, verify_events,
};
use proptest::collection::vec;
use proptest::prelude::*;

/// FIPS 180-2 SHA-256 test vectors.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

type Store = BTreeMap<Digest, Vec<u8>>;

#[test]
#[ignore = "pending E5-2"]
fn the_address_is_sha256_of_the_stored_bytes() {
    assert_eq!(
        ArtifactRef::of(b"").to_string(),
        format!("sha256:{EMPTY_SHA256}")
    );
    assert_eq!(
        ArtifactRef::of(b"abc").to_string(),
        format!("sha256:{ABC_SHA256}")
    );
    assert_eq!(
        ArtifactRef::of(b"abc").digest().to_hex(),
        ABC_SHA256,
        "digest() is the hashed bytes"
    );
    let mut store = Store::new();
    assert_eq!(
        store.put_artifact(b"abc").map(|r| r.to_string()),
        Ok(format!("sha256:{ABC_SHA256}"))
    );
}

#[test]
#[ignore = "pending E5-2"]
fn references_parse_only_in_the_artifact_refs_form() {
    let text = format!("sha256:{ABC_SHA256}");
    let parsed = ArtifactRef::parse(&text).expect("canonical reference");
    assert_eq!(parsed, ArtifactRef::of(b"abc"));
    assert_eq!(parsed.to_string(), text);
    assert_eq!(
        ArtifactRef::from_digest(Digest::of(b"abc")),
        ArtifactRef::of(b"abc")
    );
    for bad in [
        ABC_SHA256.to_owned(),
        format!("SHA256:{ABC_SHA256}"),
        format!("sha256:{}", ABC_SHA256.to_uppercase()),
        format!("sha256:{}", &ABC_SHA256[1..]),
        format!("sha256:{ABC_SHA256}0"),
        format!("sha256: {ABC_SHA256}"),
        format!("sha512:{ABC_SHA256}"),
    ] {
        assert_eq!(ArtifactRef::parse(&bad), None, "{bad}");
    }
}

#[test]
#[ignore = "pending E5-2"]
fn error_codes_are_stable() {
    assert_eq!(ArtifactError::Missing.code(), "artifact_missing");
    assert_eq!(ArtifactError::Corrupt.code(), "artifact_mismatch");
    assert_eq!(ArtifactError::Unavailable.code(), "artifact_unavailable");
}

#[test]
#[ignore = "pending E5-2"]
fn an_absent_reference_is_missing() {
    let store = Store::new();
    let reference = ArtifactRef::of(b"never stored");
    assert_eq!(store.read_artifact(&reference), Err(ArtifactError::Missing));
    assert_eq!(
        get_artifact(&store, &reference),
        Err(ArtifactError::Missing)
    );
}

#[test]
#[ignore = "pending E5-2"]
fn put_never_overwrites_a_corrupt_object() {
    let mut store = Store::new();
    let reference = store.put_artifact(b"thesis").expect("stored");
    store.insert(reference.digest(), b"thesix".to_vec());
    assert_eq!(store.put_artifact(b"thesis"), Err(ArtifactError::Corrupt));
    assert_eq!(
        store.get(&reference.digest()).map(Vec::as_slice),
        Some(&b"thesix"[..]),
        "the corrupt object stays as evidence"
    );
}

/// A source whose backend cannot be reached.
struct Unreachable;

impl ArtifactSource for Unreachable {
    fn read_artifact(&self, _: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        Err(ArtifactError::Unavailable)
    }
}

/// A source whose backend detects corruption itself.
struct SelfChecking;

impl ArtifactSource for SelfChecking {
    fn read_artifact(&self, _: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        Err(ArtifactError::Corrupt)
    }
}

#[test]
#[ignore = "pending E5-2"]
fn verification_reads_artifacts_through_the_store() {
    let content = b"model response".to_vec();
    let reference = ArtifactRef::of(&content);
    let quoted = format!("\"{reference}\"");
    let draft = edit(&mark_draft(1, "1"), "payload.source", Some(&quoted));
    let draft = edit(&draft, "artifact_refs", Some(&format!("[{quoted}]")));
    let mut journal = journal_with(0);
    assert!(matches!(
        journal.append(&stream(), 1, 1, now(), &[&draft]),
        AppendOutcome::Committed(_)
    ));
    let rows = journal.rows(&stream());
    let failed = |check| Err(EventFailure { seq: 2, check });
    assert_eq!(
        verify_events(rows, TrustedStart::GENESIS, &Unreachable),
        failed(EventCheck::ArtifactMissing),
        "an unreachable store never passes verification"
    );
    assert_eq!(
        verify_events(rows, TrustedStart::GENESIS, &SelfChecking),
        failed(EventCheck::ArtifactMismatch)
    );
    let mut store = Store::new();
    assert_eq!(store.put_artifact(&content), Ok(reference));
    assert!(verify_events(rows, TrustedStart::GENESIS, &store).is_ok());
}

proptest! {
    #[test]
    #[ignore = "pending E5-2"]
    fn put_then_get_returns_the_bytes(bytes in vec(any::<u8>(), 0..512)) {
        let mut store = Store::new();
        let reference = store.put_artifact(&bytes).expect("stored");
        prop_assert_eq!(reference, ArtifactRef::from_digest(Digest::of(&bytes)));
        prop_assert_eq!(store.get(&reference.digest()), Some(&bytes));
        prop_assert_eq!(get_artifact(&store, &reference), Ok(bytes.clone()));
        prop_assert_eq!(check_artifact(&reference, &bytes), Ok(()));
        prop_assert_eq!(ArtifactRef::parse(&reference.to_string()), Some(reference));
    }

    #[test]

    #[ignore = "pending E5-2"]
    fn the_same_bytes_get_the_same_address_once(
        a in vec(any::<u8>(), 0..64),
        b in vec(any::<u8>(), 0..64),
    ) {
        let mut store = Store::new();
        let first = store.put_artifact(&a).expect("stored");
        prop_assert_eq!(store.put_artifact(&a), Ok(first), "re-put is idempotent");
        let other = store.put_artifact(&b).expect("stored");
        prop_assert_eq!(first == other, a == b);
        prop_assert_eq!(store.len(), if a == b { 1 } else { 2 });
    }

    #[test]

    #[ignore = "pending E5-2"]
    fn one_flipped_bit_is_detected_on_read(
        bytes in vec(any::<u8>(), 1..256),
        index in any::<prop::sample::Index>(),
        bit in 0u8..8,
    ) {
        let mut store = Store::new();
        let reference = store.put_artifact(&bytes).expect("stored");
        let mut flipped = bytes.clone();
        flipped[index.index(bytes.len())] ^= 1 << bit;
        store.insert(reference.digest(), flipped.clone());
        prop_assert_eq!(store.read_artifact(&reference), Ok(flipped.clone()), "raw read");
        prop_assert_eq!(get_artifact(&store, &reference), Err(ArtifactError::Corrupt));
        prop_assert_eq!(check_artifact(&reference, &flipped), Err(ArtifactError::Corrupt));
    }
}
