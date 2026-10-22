//! The claim and the release over a scripted registry (DEC-2345 item 4, review F-a): every way
//! the registry answers, and above all one that keeps moving under them, which they answer
//! `Unavailable` after a bounded number of reads, with nothing written.

use std::cell::RefCell;
use std::collections::BTreeMap;

use mandate_connections::record::ConnectionId;

use crate::fingerprint::{AccountFingerprint, Claim, FingerprintError};
use crate::registry::{CLAIM_TRIES, Registry, claim_in, release_in};

/// A transit HMAC-SHA256 output, as [`AccountFingerprint`] takes it.
const HASH: &str = "vault:v1:q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA=";

fn c(id: &str) -> ConnectionId {
    ConnectionId::new(id).unwrap()
}

fn held() -> AccountFingerprint {
    AccountFingerprint::new(HASH).unwrap()
}

/// A registry in memory: its claims at a version, every read, every write asked for, and, when
/// `moving`, another writer that lands between every read and the write after it, so no
/// check-and-set ever holds.
#[derive(Default)]
struct Scripted {
    version: RefCell<u64>,
    claims: RefCell<BTreeMap<String, String>>,
    moving: bool,
    reads: RefCell<usize>,
    writes: RefCell<Vec<BTreeMap<String, String>>>,
}

impl Scripted {
    fn holding(holder: &str, moving: bool) -> Self {
        let scripted = Self {
            moving,
            ..Self::default()
        };
        scripted
            .claims
            .borrow_mut()
            .insert(HASH.to_owned(), holder.to_owned());
        *scripted.version.borrow_mut() = 3;
        scripted
    }
}

impl Registry for Scripted {
    async fn claims(&self) -> Result<(u64, BTreeMap<String, String>), FingerprintError> {
        *self.reads.borrow_mut() += 1;
        let read = (*self.version.borrow(), self.claims.borrow().clone());
        if self.moving {
            *self.version.borrow_mut() += 1;
        }
        Ok(read)
    }

    async fn write_claims(
        &self,
        version: u64,
        claims: &BTreeMap<String, String>,
    ) -> Result<bool, FingerprintError> {
        if version != *self.version.borrow() {
            return Ok(false);
        }
        self.writes.borrow_mut().push(claims.clone());
        *self.claims.borrow_mut() = claims.clone();
        *self.version.borrow_mut() += 1;
        Ok(true)
    }
}

/// A registry that keeps moving under a claim fails it closed after [`CLAIM_TRIES`] reads, with
/// nothing written, and leaves it unclaimed.
#[tokio::test(flavor = "current_thread")]
async fn a_claim_under_a_moving_registry_fails_closed_with_nothing_written() {
    let registry = Scripted {
        moving: true,
        ..Scripted::default()
    };
    assert_eq!(
        claim_in(&registry, &c("c1"), &held()).await,
        Err(FingerprintError::Unavailable)
    );
    assert_eq!(*registry.reads.borrow(), CLAIM_TRIES);
    assert!(registry.writes.borrow().is_empty());
    assert!(registry.claims.borrow().is_empty());
}

/// A registry that keeps moving under a release fails it closed after [`CLAIM_TRIES`] reads, with
/// nothing written, and the claim kept: never reported given back while it is not.
#[tokio::test(flavor = "current_thread")]
async fn a_release_under_a_moving_registry_fails_closed_with_nothing_written() {
    let registry = Scripted::holding("c1", true);
    assert_eq!(
        release_in(&registry, &c("c1"), &held()).await,
        Err(FingerprintError::Unavailable)
    );
    assert_eq!(*registry.reads.borrow(), CLAIM_TRIES);
    assert!(registry.writes.borrow().is_empty());
    assert_eq!(
        registry.claims.borrow().get(HASH).map(String::as_str),
        Some("c1")
    );
}

/// A claim of a free fingerprint writes the one more pair once; one held by the connection, or by
/// another, writes nothing and answers `Claimed` or `HeldBy` it; a holder that is not a
/// connection id is `Unavailable`.
#[tokio::test(flavor = "current_thread")]
async fn a_claim_writes_only_a_free_fingerprint() {
    let free = Scripted::default();
    assert_eq!(claim_in(&free, &c("c1"), &held()).await, Ok(Claim::Claimed));
    let mut wrote = BTreeMap::new();
    wrote.insert(HASH.to_owned(), "c1".to_owned());
    assert_eq!(free.writes.borrow().as_slice(), [wrote]);
    for (holder, answer) in [
        ("c1", Ok(Claim::Claimed)),
        ("c2", Ok(Claim::HeldBy(c("c2")))),
        ("not a connection", Err(FingerprintError::Unavailable)),
    ] {
        let registry = Scripted::holding(holder, false);
        assert_eq!(
            claim_in(&registry, &c("c1"), &held()).await,
            answer,
            "{holder}"
        );
        assert!(registry.writes.borrow().is_empty(), "{holder}");
        assert_eq!(*registry.reads.borrow(), 1, "{holder}");
    }
}

/// A release by the holder writes the registry without its pair once; one by a connection that
/// does not hold the fingerprint, or of one nobody holds, writes nothing.
#[tokio::test(flavor = "current_thread")]
async fn a_release_writes_only_the_holders_pair_away() {
    let holder = Scripted::holding("c1", false);
    assert_eq!(release_in(&holder, &c("c1"), &held()).await, Ok(()));
    assert_eq!(holder.writes.borrow().as_slice(), [BTreeMap::new()]);
    for registry in [Scripted::holding("c2", false), Scripted::default()] {
        assert_eq!(release_in(&registry, &c("c1"), &held()).await, Ok(()));
        assert!(registry.writes.borrow().is_empty());
        assert_eq!(*registry.reads.borrow(), 1);
    }
}
