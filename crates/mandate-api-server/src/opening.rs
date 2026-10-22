//! Route 2's judge (§4.10 route 2 items 2 and 4; identity spec §6.4 route 2; [DEC-2324] item 5):
//! whether an assertion over a challenge the route already took opens a reduction-only session,
//! and for whom.
//!
//! **No membership oracle, by structure.** Every request runs the same steps whatever it names:
//! the whole passkey fold is replayed and looked up by the passkey id; the signature is verified
//! by `mandate_passkey::reduction::verify_reduction`, which makes one ES256 and one Ed25519 check
//! on every call, with the passkey's key, or with `None` for an unknown passkey or a key that does
//! not resolve, parse, or match its recorded algorithm ([DEC-2324] item 4); the membership lookup
//! and [DEC-2319]'s interim standing run for the passkey's member, or for [`DECOY_MEMBER`]; and
//! only then are the results judged, together. A decoy's check never answers.
//!
//! [DEC-2319]: ../../../docs/project/decisions/DEC-2319.md
//! [DEC-2324]: ../../../docs/project/decisions/DEC-2324.md

use mandate_authn::SubjectStanding;
use mandate_identity::{LookupFailed, MembershipRecord, PrincipalId};
use mandate_journal::{ArtifactRef, ArtifactSource, get_artifact};
use mandate_passkey::credentials::{CredentialFold, CredentialRecord, Enrolment, PasskeyAlg};
use mandate_passkey::reduction::{cose_key, verify_reduction};
use mandate_passkey::{Assertion, Challenge, PublicKey, RelyingParty};
use mandate_time::UtcNanos;

use crate::members::{ControlLookup, Hosted};
use crate::standing::{Standings, interim_refuses};

/// The principal the membership and standing lookups run for when the passkey is unknown, so
/// they cost the same: the greatest ULID, which no principal is minted as.
pub const DECOY_MEMBER: PrincipalId = PrincipalId(u128::MAX);

/// What the browser presented besides the challenge (`commands/reduction-session-request`),
/// decoded from base64url.
#[derive(Debug, Clone, Copy)]
pub struct Presented<'a> {
    /// The passkey's id, unpadded base64url, as journal spec §9.31 records it.
    pub passkey_id: &'a str,
    pub authenticator_data: &'a [u8],
    pub client_data_json: &'a [u8],
    pub signature: &'a [u8],
}

/// What the judge reads besides the assertion, each read for every request.
#[derive(Clone, Copy)]
pub struct Reads<'a> {
    /// The workspace this deployment hosts.
    pub hosted: Hosted,
    /// The hosted control stream's passkey records, or `None` when they could not be read.
    pub credentials: Option<&'a [CredentialRecord]>,
    /// Its membership records, or `None` when they could not be read.
    pub memberships: Option<&'a [MembershipRecord]>,
    /// The sign-ins this process has seen ([DEC-2319] item 2).
    ///
    /// [DEC-2319]: ../../../docs/project/decisions/DEC-2319.md
    pub standings: &'a Standings,
    /// The store the passkeys' public keys are artifacts in; `None` when the deployment names
    /// none, and then no key resolves.
    pub keys: Option<&'a (dyn ArtifactSource + Send + Sync)>,
}

/// The member a verified assertion opens a session for, and their standing then, which the
/// session store judges a deprovision by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opened {
    pub member: PrincipalId,
    pub standing: SubjectStanding,
}

/// Judges `presented` over `challenge` at `now`: [`Opened`] only when the passkey is held, its key
/// resolves to the algorithm it was recorded with, the assertion verifies against it for `rp`, the
/// member's membership of the hosted workspace reaches it (`active` or `cooling_off`), and
/// [DEC-2319]'s interim does not refuse them; `None` otherwise, after the same steps.
///
/// [DEC-2319]: ../../../docs/project/decisions/DEC-2319.md
pub fn judge(
    rp: &RelyingParty,
    challenge: &Challenge,
    presented: &Presented<'_>,
    reads: &Reads<'_>,
    now: UtcNanos,
) -> Option<Opened> {
    let fold = CredentialFold::new(reads.credentials.unwrap_or_default().iter().cloned());
    let held = fold.held(presented.passkey_id);
    let key = resolve(held, reads.keys);
    let assertion = Assertion {
        credential_id: &[],
        authenticator_data: presented.authenticator_data,
        client_data_json: presented.client_data_json,
        signature: presented.signature,
    };
    let verified = verify_reduction(rp, challenge, key.as_ref(), &assertion).is_ok();
    let member = held.map_or(DECOY_MEMBER, |enrolment| enrolment.member);
    let records = reads.memberships.map(<[MembershipRecord]>::to_vec);
    let lookup = ControlLookup::new(reads.hosted, records.ok_or(LookupFailed), now);
    let reaches = lookup.reaches(member) == Ok(true);
    let standing = reads.standings.standing(member);
    let refused = interim_refuses(standing, reads.memberships.unwrap_or_default(), member);
    (key.is_some() && verified && reaches && !refused).then_some(Opened { member, standing })
}

/// The key `held` names, read from `keys` and parsed, when it is the algorithm the enrolment
/// recorded; `None` otherwise. The store is read and a key parsed for every request: a decoy
/// reference when no passkey is held, and the decoy key when the read finds nothing, so whether one
/// is held costs no difference in reads or parses.
fn resolve(
    held: Option<&Enrolment>,
    keys: Option<&(dyn ArtifactSource + Send + Sync)>,
) -> Option<PublicKey> {
    let named = held.and_then(|enrolment| ArtifactRef::parse(&enrolment.public_key));
    let reference = named.unwrap_or_else(|| ArtifactRef::of(DECOY_KEY_BYTES));
    let bytes = keys.and_then(|keys| get_artifact(keys, &reference).ok());
    let parsed = cose_key(bytes.as_deref().unwrap_or(&DECOY_COSE_KEY));
    let key = bytes.and(parsed.ok())?;
    let recorded = match (&key, held?.alg) {
        (PublicKey::Es256 { .. }, PasskeyAlg::Es256) => true,
        (PublicKey::Ed25519 { .. }, PasskeyAlg::EdDsa) => true,
        (PublicKey::Es256 { .. }, PasskeyAlg::EdDsa) | (PublicKey::Ed25519 { .. }, _) => false,
    };
    recorded.then_some(key)
}

/// A COSE_Key (RFC 9053) for the P-256 base point, the bytes a read that found no key parses
/// instead, so a store hit and a miss cost the same parse; the key it gives is never used.
const DECOY_COSE_KEY: [u8; 77] = [
    0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x58, 0x20, 0x6b, 0x17, 0xd1, 0xf2, 0xe1, 0x2c,
    0x42, 0x47, 0xf8, 0xbc, 0xe6, 0xe5, 0x63, 0xa4, 0x40, 0xf2, 0x77, 0x03, 0x7d, 0x81, 0x2d, 0xeb,
    0x33, 0xa0, 0xf4, 0xa1, 0x39, 0x45, 0xd8, 0x98, 0xc2, 0x96, 0x22, 0x58, 0x20, 0x4f, 0xe3, 0x42,
    0xe2, 0xfe, 0x1a, 0x7f, 0x9b, 0x8e, 0xe7, 0xeb, 0x4a, 0x7c, 0x0f, 0x9e, 0x16, 0x2b, 0xce, 0x33,
    0x57, 0x6b, 0x31, 0x5e, 0xce, 0xcb, 0xb6, 0x40, 0x68, 0x37, 0xbf, 0x51, 0xf5,
];

/// The bytes whose reference an unknown passkey's read asks the store for: no key is stored as
/// them, so the read finds nothing.
const DECOY_KEY_BYTES: &[u8] = b"route 2: no passkey is held";
