#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! Workspace services as a WebAuthn relying party ([identity spec §6.1](../../../docs/specs/identity.md#61-methods),
//! [§6.3](../../../docs/specs/identity.md#63-device-binding); backlog E9-1, [DEC-660]).
//!
//! The workspace deployment verifies passkeys itself, with no outside service, so step-up
//! (§7.2) and the workspace-local risk-reduction route (§6.4 route 2) work when the identity
//! provider does not (ID-10). [`enrol`] reads a registration response and returns the
//! [`Credential`] to store; [`verify`] checks an assertion against a stored one.
//!
//! Every check is the strict reading ([DEC-660] lists them):
//!
//! - attestation format `none` only, with an empty statement (attestation is not required in
//!   v1, §6.1);
//! - `clientDataJSON`'s `type` is `webauthn.create` or `webauthn.get` as the ceremony requires,
//!   its `challenge` is exactly the base64url form (no padding) of the issued challenge, its
//!   `origin` equals the relying party's one origin byte for byte, and `crossOrigin` is never
//!   `true`;
//! - `authenticatorData`'s RP ID hash is SHA-256 of the relying party ID, and both user presence
//!   and user verification are set (§6.1: user verification required);
//! - ES256 (P-256) and EdDSA (Ed25519) keys only;
//! - the signature counter must rise whenever the authenticator keeps one: a counter equal to or
//!   below the stored one is refused, and only a counter that is zero on both sides passes as
//!   "not kept".
//!
//! Every entry point is pure: no clock, no randomness, no I/O. The crate only verifies; it never
//! signs, and it never holds a private key.
//!
//! [DEC-660]: ../../../docs/project/decisions/DEC-660.md

mod auth_data;
mod cbor;
mod client_data;
mod cose;

use mandate_canon::Digest;

use auth_data::AuthData;
use cbor::Cbor;
use client_data::Ceremony;

/// The relying party a workspace deployment is: its RP ID and the one origin its web app is
/// served from, both configuration (`app.owlhead.ai` and `https://app.owlhead.ai` in production,
/// DEC-820).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelyingParty {
    pub rp_id: String,
    pub origin: String,
}

/// The challenge bytes a ceremony was issued. Step-up passes SHA-256 of its challenge record
/// (§7.2 step 2); sign-in passes random bytes. Shorter than [`Challenge::MIN_LEN`] is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge(Vec<u8>);

impl Challenge {
    /// The WebAuthn recommendation: at least 16 bytes.
    pub const MIN_LEN: usize = 16;

    pub fn new(bytes: &[u8]) -> Result<Self, ChallengeTooShort> {
        if bytes.len() < Self::MIN_LEN {
            return Err(ChallengeTooShort);
        }
        Ok(Self(bytes.to_vec()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a challenge has at least 16 bytes")]
pub struct ChallengeTooShort;

/// A credential public key, by COSE algorithm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicKey {
    /// COSE `-7`: ECDSA over P-256 with SHA-256, kept as the affine coordinates.
    Es256 { x: [u8; 32], y: [u8; 32] },
    /// COSE `-8`: EdDSA over Ed25519.
    Ed25519 { x: [u8; 32] },
}

/// What enrolment stores for one passkey: never a private key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub id: Vec<u8>,
    pub public_key: PublicKey,
    /// The signature counter at enrolment; zero when the authenticator keeps none.
    pub sign_count: u32,
    /// The backup-eligible flag at enrolment, which no later assertion may change.
    pub backup_eligible: bool,
}

/// A registration response (`navigator.credentials.create`) as the client sends it.
#[derive(Debug, Clone, Copy)]
pub struct Registration<'a> {
    pub attestation_object: &'a [u8],
    pub client_data_json: &'a [u8],
}

/// An assertion response (`navigator.credentials.get`) as the client sends it. This is also the
/// raw assertion step-up stores, so an auditor can verify it again later (§7.2 step 5).
#[derive(Debug, Clone, Copy)]
pub struct Assertion<'a> {
    pub credential_id: &'a [u8],
    pub authenticator_data: &'a [u8],
    pub client_data_json: &'a [u8],
    pub signature: &'a [u8],
}

/// A verified assertion: the counter to store and the backup state the authenticator reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verified {
    pub sign_count: u32,
    pub backup_state: bool,
}

/// Why a registration or an assertion was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("clientDataJSON is not the expected JSON object")]
    ClientDataMalformed,
    #[error("clientDataJSON names another ceremony type")]
    ClientDataType,
    #[error("the challenge is not the one issued")]
    ChallengeMismatch,
    #[error("the origin is not the relying party's")]
    OriginMismatch,
    #[error("the ceremony ran in a cross-origin frame")]
    CrossOrigin,
    #[error("the attestation object is not well-formed")]
    AttestationMalformed,
    #[error("the attestation format is not `none` with an empty statement")]
    AttestationFormat,
    #[error("the authenticator data is not well-formed")]
    AuthenticatorDataMalformed,
    #[error("the RP ID hash is not the relying party's")]
    RpIdMismatch,
    #[error("the authenticator did not report user presence")]
    UserNotPresent,
    #[error("the authenticator did not report user verification")]
    UserNotVerified,
    #[error("the backup flags are inconsistent or changed since enrolment")]
    BackupFlags,
    #[error("the credential public key is not well-formed")]
    PublicKeyMalformed,
    #[error("the credential's algorithm is not supported")]
    AlgorithmUnsupported,
    #[error("the assertion names another credential")]
    CredentialMismatch,
    #[error("the signature does not verify")]
    SignatureInvalid,
    #[error("the signature counter did not rise")]
    CounterNotRising,
}

impl Refusal {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(self) -> &'static str {
        match self {
            Self::ClientDataMalformed => "client_data_malformed",
            Self::ClientDataType => "client_data_type",
            Self::ChallengeMismatch => "challenge_mismatch",
            Self::OriginMismatch => "origin_mismatch",
            Self::CrossOrigin => "cross_origin",
            Self::AttestationMalformed => "attestation_malformed",
            Self::AttestationFormat => "attestation_format",
            Self::AuthenticatorDataMalformed => "authenticator_data_malformed",
            Self::RpIdMismatch => "rp_id_mismatch",
            Self::UserNotPresent => "user_not_present",
            Self::UserNotVerified => "user_not_verified",
            Self::BackupFlags => "backup_flags",
            Self::PublicKeyMalformed => "public_key_malformed",
            Self::AlgorithmUnsupported => "algorithm_unsupported",
            Self::CredentialMismatch => "credential_mismatch",
            Self::SignatureInvalid => "signature_invalid",
            Self::CounterNotRising => "counter_not_rising",
        }
    }
}

/// Reads a registration response for `challenge` and returns the credential to store.
pub fn enrol(
    rp: &RelyingParty,
    challenge: &Challenge,
    registration: &Registration<'_>,
) -> Result<Credential, Refusal> {
    client_data::check(
        registration.client_data_json,
        Ceremony::Create,
        rp,
        challenge,
    )?;
    let object = cbor::read_all(registration.attestation_object)
        .map_err(|_| Refusal::AttestationMalformed)?;
    let text = |key: &str| object.get(&Cbor::Text(key.to_owned()));
    let none_format = text("fmt") == Some(&Cbor::Text("none".to_owned()));
    let empty_statement = text("attStmt") == Some(&Cbor::Map(Vec::new()));
    if !none_format || !empty_statement {
        return Err(Refusal::AttestationFormat);
    }
    let Some(Cbor::Bytes(bytes)) = text("authData") else {
        return Err(Refusal::AttestationMalformed);
    };
    let auth_data = AuthData::parse(bytes)?;
    auth_data.check(rp)?;
    let Some(attested) = &auth_data.attested else {
        return Err(Refusal::AuthenticatorDataMalformed);
    };
    Ok(Credential {
        id: attested.credential_id.clone(),
        public_key: cose::public_key(&attested.public_key)?,
        sign_count: auth_data.sign_count,
        backup_eligible: auth_data.backup_eligible(),
    })
}

/// Verifies an assertion for `challenge` against the stored `credential`. On success the caller
/// stores [`Verified::sign_count`] in the same transaction that commits what the assertion
/// authorizes.
pub fn verify(
    rp: &RelyingParty,
    challenge: &Challenge,
    credential: &Credential,
    assertion: &Assertion<'_>,
) -> Result<Verified, Refusal> {
    if assertion.credential_id != credential.id {
        return Err(Refusal::CredentialMismatch);
    }
    client_data::check(assertion.client_data_json, Ceremony::Get, rp, challenge)?;
    let auth_data = AuthData::parse(assertion.authenticator_data)?;
    if auth_data.attested.is_some() {
        return Err(Refusal::AuthenticatorDataMalformed);
    }
    auth_data.check(rp)?;
    if auth_data.backup_eligible() != credential.backup_eligible {
        return Err(Refusal::BackupFlags);
    }
    let client_data_hash = Digest::of(assertion.client_data_json);
    let message = [assertion.authenticator_data, client_data_hash.as_bytes()].concat();
    cose::verify(&credential.public_key, &message, assertion.signature)?;
    let kept = credential.sign_count != 0 || auth_data.sign_count != 0;
    if kept && auth_data.sign_count <= credential.sign_count {
        return Err(Refusal::CounterNotRising);
    }
    Ok(Verified {
        sign_count: auth_data.sign_count,
        backup_state: auth_data.backup_state(),
    })
}
