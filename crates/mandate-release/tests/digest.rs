//! The build digest a process journals as `actor.build` (E21-8, DEC-970 item 4): the SHA-256 of
//! the executable's bytes, the same digest the release's `SHA256SUMS` lists. The oracle is the
//! `sha2` crate over the file's bytes, read here, not the crate's own hashing.

use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

use mandate_release::{BuildDigest, ReleaseError};
use proptest::prelude::*;
use sha2::{Digest as _, Sha256};

fn oracle(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn scratch(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("mandate-release-{name}"))
}

/// FIPS 180-2's vectors, so the digest is SHA-256 and not some other hash of the bytes.
#[test]
fn the_digest_of_bytes_is_their_sha256() {
    assert_eq!(
        BuildDigest::of_bytes(b"").to_hex(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        BuildDigest::of_bytes(b"abc").to_hex(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// The reference is the content reference the envelope's `actor.build` admits: `sha256:` and
/// the 64 lowercase hex digits `SHA256SUMS` lists.
#[test]
fn the_reference_is_sha256_and_the_hex() {
    let digest = BuildDigest::of_bytes(b"abc");
    assert_eq!(
        digest.reference(),
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn the_digest_of_a_file_is_the_sha256_of_its_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let path = scratch("file");
    let bytes: Vec<u8> = (0..=255u8).cycle().take(1_000_003).collect();
    fs::write(&path, &bytes)?;
    let digest = BuildDigest::of_file(&path)?;
    assert_eq!(digest.to_hex(), oracle(&bytes));
    assert_eq!(digest, BuildDigest::of_bytes(&bytes));
    fs::remove_file(&path)?;
    Ok(())
}

#[test]
fn a_missing_file_is_unreadable_and_names_no_path() {
    let path = scratch("missing");
    let refused = BuildDigest::of_file(&path);
    assert_eq!(refused, Err(ReleaseError::Unreadable(io::ErrorKind::NotFound)));
    if let Err(error) = refused {
        let text = error.to_string();
        assert!(!text.contains("mandate-release-missing"), "{text}");
    }
}

/// The process's own digest is the digest of the file it was started from: this test binary.
#[test]
fn the_running_digest_is_the_sha256_of_the_running_executable()
-> Result<(), Box<dyn std::error::Error>> {
    let running = BuildDigest::running()?;
    let bytes = fs::read(env::current_exe()?)?;
    assert_eq!(running.to_hex(), oracle(&bytes));
    Ok(())
}

#[test]
fn a_reference_reads_back_as_its_digest() -> Result<(), ReleaseError> {
    let digest = BuildDigest::of_bytes(b"abc");
    assert_eq!(BuildDigest::from_reference(&digest.reference())?, digest);
    Ok(())
}

#[test]
fn only_sha256_and_64_lowercase_hex_digits_is_a_reference() {
    let hex = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    for refused in [
        String::new(),
        hex.to_owned(),
        format!("sha256:{}", hex.to_uppercase()),
        format!("sha256:{}", &hex[1..]),
        format!("sha256:{hex}0"),
        format!("sha512:{hex}"),
        format!("SHA256:{hex}"),
        format!(" sha256:{hex}"),
        format!("sha256:{hex} "),
    ] {
        assert_eq!(
            BuildDigest::from_reference(&refused),
            Err(ReleaseError::NotAReference),
            "{refused:?}"
        );
    }
}

proptest! {
    #[test]
    fn every_digest_round_trips_through_its_reference(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let digest = BuildDigest::of_bytes(&bytes);
        prop_assert_eq!(digest.to_hex(), oracle(&bytes));
        prop_assert_eq!(digest.reference(), format!("sha256:{}", oracle(&bytes)));
        prop_assert_eq!(BuildDigest::from_reference(&digest.reference()), Ok(digest));
    }

    #[test]
    fn a_reference_with_any_other_prefix_is_refused(prefix in "[a-zA-Z0-9:]{0,10}", bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        prop_assume!(prefix != "sha256:");
        let candidate = format!("{prefix}{}", oracle(&bytes));
        prop_assert_eq!(BuildDigest::from_reference(&candidate), Err(ReleaseError::NotAReference));
    }
}
