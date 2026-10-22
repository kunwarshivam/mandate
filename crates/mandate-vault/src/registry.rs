//! The fingerprint registry's claim and release (DEC-2345 item 4) over any [`Registry`], so the
//! check-and-set loop is the same code whether the registry is the vault's entry or a scripted
//! one, and its bound is tested without a race (review F-a).
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the connection manager's claim and release run over it once F3b-2 lands"
    )
)]

use std::collections::BTreeMap;

use mandate_connections::record::ConnectionId;

use crate::fingerprint::{AccountFingerprint, Claim, FingerprintError};

/// How many times a claim or a release reads the registry again when it moved under it.
pub(crate) const CLAIM_TRIES: usize = 8;

/// The registry as a claim and a release use it: read with the version it was read at, an empty
/// one at version 0 where there is none, and written back by check-and-set on that version,
/// `false` when it moved since (DEC-2345 item 4).
pub(crate) trait Registry {
    /// The registry, fingerprint to holding connection, and the version it was read at.
    async fn claims(&self) -> Result<(u64, BTreeMap<String, String>), FingerprintError>;

    /// Writes `claims` by check-and-set on `version`: `false` when it moved since.
    async fn write_claims(
        &self,
        version: u64,
        claims: &BTreeMap<String, String>,
    ) -> Result<bool, FingerprintError>;
}

/// Claims `fingerprint` for `connection` in `registry`, by check-and-set on the version read:
/// [`Claim::Claimed`] when it is now this connection's or was already, and [`Claim::HeldBy`]
/// another connection that holds it. A registry that moves under it [`CLAIM_TRIES`] times answers
/// [`FingerprintError::Unavailable`] with nothing written, as does a holder that is not a
/// connection id.
pub(crate) async fn claim_in(
    registry: &impl Registry,
    connection: &ConnectionId,
    fingerprint: &AccountFingerprint,
) -> Result<Claim, FingerprintError> {
    let _ = (
        registry.claims(),
        registry.write_claims(0, &BTreeMap::new()),
        connection,
        fingerprint,
        CLAIM_TRIES,
    );
    Err(FingerprintError::Unimplemented { story: "E21-32" })
}

/// Gives back `connection`'s claim of `fingerprint` in `registry`, when it holds it; one it does
/// not hold changes nothing. A registry that moves under it [`CLAIM_TRIES`] times answers
/// [`FingerprintError::Unavailable`] with nothing written.
pub(crate) async fn release_in(
    registry: &impl Registry,
    connection: &ConnectionId,
    fingerprint: &AccountFingerprint,
) -> Result<(), FingerprintError> {
    let _ = (
        registry.claims(),
        registry.write_claims(0, &BTreeMap::new()),
        connection,
        fingerprint,
        CLAIM_TRIES,
    );
    Err(FingerprintError::Unimplemented { story: "E21-32" })
}
