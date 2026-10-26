//! Route 2's verifier (workspace API spec §4.10 route 2 items 2 and 4, [DEC-2324]): an assertion
//! judged for a reduction-only session, where no answer and no timing may tell a passkey the
//! workspace holds from one it does not.
//!
//! [`verify`](crate::verify) returns at the first check that fails, before the signature, and
//! compares the backup-eligible flag with the stored credential's, so a held passkey with another
//! flag would fail sooner than an unknown one. [`verify_reduction`] instead verifies the signature
//! first and always, then judges the client data and the authenticator data, and only then
//! answers. It compares no credential id (the caller looked the key up by it), no backup-eligible
//! flag (journal spec §9.31 records none, so nothing holds one to compare; where it comes from
//! stays open, as [DEC-2121]'s follow-ups say), and no counter (route 2 neither reads nor advances
//! it).
//!
//! **Constant work.** `ring` refuses a signature of the wrong shape for its algorithm without any
//! arithmetic, so checking only the held key's algorithm would tell a held key, and its
//! algorithm, from an unknown one by time ([DEC-2324] item 4). Every call therefore runs exactly
//! one ES256 and one Ed25519 check over the same message and signature, ES256 first: the held key
//! for its own algorithm and the other algorithm's decoy for the other, or both decoys when no key
//! is held. Only the held key's check answers; with no key the signature never verifies.
//!
//! [DEC-2121]: ../../../docs/project/decisions/DEC-2121.md
//! [DEC-2324]: ../../../docs/project/decisions/DEC-2324.md

use mandate_canon::Digest;

use crate::auth_data::AuthData;
use crate::client_data::{self, Ceremony};
use crate::{Assertion, Challenge, PublicKey, Refusal, RelyingParty, cbor, cose};

/// The ES256 decoy: the P-256 base point, a valid key, so checking a signature against it costs
/// a full verification. Its private key is 1, so anyone can sign for it; its check never answers.
pub const DECOY_ES256: PublicKey = PublicKey::Es256 {
    x: [
        0x6b, 0x17, 0xd1, 0xf2, 0xe1, 0x2c, 0x42, 0x47, 0xf8, 0xbc, 0xe6, 0xe5, 0x63, 0xa4, 0x40,
        0xf2, 0x77, 0x03, 0x7d, 0x81, 0x2d, 0xeb, 0x33, 0xa0, 0xf4, 0xa1, 0x39, 0x45, 0xd8, 0x98,
        0xc2, 0x96,
    ],
    y: [
        0x4f, 0xe3, 0x42, 0xe2, 0xfe, 0x1a, 0x7f, 0x9b, 0x8e, 0xe7, 0xeb, 0x4a, 0x7c, 0x0f, 0x9e,
        0x16, 0x2b, 0xce, 0x33, 0x57, 0x6b, 0x31, 0x5e, 0xce, 0xcb, 0xb6, 0x40, 0x68, 0x37, 0xbf,
        0x51, 0xf5,
    ],
};

/// The Ed25519 decoy: RFC 8032's first test key, a valid key whose secret is published, for the
/// same reason as [`DECOY_ES256`].
pub const DECOY_ED25519: PublicKey = PublicKey::Ed25519 {
    x: [
        0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07,
        0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07,
        0x51, 0x1a,
    ],
};

/// One signature check: whether `signature` over `message` verifies under `key`. The crate's own
/// is [`RingCheck`]; a test may count the checks a call makes through
/// [`verify_reduction_with`].
pub trait SignatureCheck {
    /// # Errors
    /// [`Refusal::SignatureInvalid`] for a signature that does not verify under `key`.
    fn check(&self, key: &PublicKey, message: &[u8], signature: &[u8]) -> Result<(), Refusal>;
}

/// The signature check through `ring`, the one [`verify_reduction`] uses.
#[derive(Debug, Clone, Copy, Default)]
pub struct RingCheck;

impl SignatureCheck for RingCheck {
    fn check(&self, key: &PublicKey, message: &[u8], signature: &[u8]) -> Result<(), Refusal> {
        cose::verify(key, message, signature)
    }
}

/// The public key a COSE_Key's bytes encode (RFC 9053), as a `CredentialEnrolled`'s `public_key`
/// artifact holds it: ES256 or EdDSA (DEC-660 item 3), and nothing after the key.
///
/// # Errors
/// [`Refusal::PublicKeyMalformed`] for bytes that are not one well-formed key, and
/// [`Refusal::AlgorithmUnsupported`] for another algorithm.
pub fn cose_key(bytes: &[u8]) -> Result<PublicKey, Refusal> {
    let key = cbor::read_all(bytes).map_err(|_| Refusal::PublicKeyMalformed)?;
    cose::public_key(&key)
}

/// Judges `assertion` for route 2 against `challenge`, `rp`, and `key`, the passkey's own key, or
/// `None` for an unknown passkey or a key that does not resolve or parse. One ES256 and one
/// Ed25519 check run over the signature, whatever `key` is (the module's constant work); then the
/// client data (a `get` ceremony, the challenge, the origin, not cross-origin) and the
/// authenticator data (no attested credential, the RP ID hash, user presence and verification,
/// consistent backup flags) are judged. A bad signature is the answer whenever it is bad, and with
/// no key it always is.
///
/// # Errors
/// [`Refusal::SignatureInvalid`] for a signature that does not verify under `key`, or no `key`;
/// otherwise the client data's refusal, then the authenticator data's.
pub fn verify_reduction(
    rp: &RelyingParty,
    challenge: &Challenge,
    key: Option<&PublicKey>,
    assertion: &Assertion<'_>,
) -> Result<(), Refusal> {
    verify_reduction_with(&RingCheck, rp, challenge, key, assertion)
}

/// [`verify_reduction`] with its two signature checks made through `check`.
///
/// # Errors
/// As [`verify_reduction`].
pub fn verify_reduction_with(
    check: &impl SignatureCheck,
    rp: &RelyingParty,
    challenge: &Challenge,
    key: Option<&PublicKey>,
    assertion: &Assertion<'_>,
) -> Result<(), Refusal> {
    let client_data_hash = Digest::of(assertion.client_data_json);
    let message = [assertion.authenticator_data, client_data_hash.as_bytes()].concat();
    let (es256, ed25519) = match key {
        Some(held @ PublicKey::Es256 { .. }) => (held, &DECOY_ED25519),
        Some(held @ PublicKey::Ed25519 { .. }) => (&DECOY_ES256, held),
        None => (&DECOY_ES256, &DECOY_ED25519),
    };
    let es256_signed = check.check(es256, &message, assertion.signature);
    let ed25519_signed = check.check(ed25519, &message, assertion.signature);
    let signed = match key {
        Some(PublicKey::Es256 { .. }) => es256_signed,
        Some(PublicKey::Ed25519 { .. }) => ed25519_signed,
        None => Err(Refusal::SignatureInvalid),
    };
    let client_data = client_data::check(assertion.client_data_json, Ceremony::Get, rp, challenge);
    let authenticator_data = AuthData::parse(assertion.authenticator_data).and_then(|parsed| {
        if parsed.attested.is_some() {
            return Err(Refusal::AuthenticatorDataMalformed);
        }
        parsed.check(rp)
    });
    signed.and(client_data).and(authenticator_data)
}
