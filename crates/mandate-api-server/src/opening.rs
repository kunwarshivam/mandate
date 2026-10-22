//! Route 2's judge (§4.10 route 2 items 2 and 4; identity spec §6.4 route 2; [DEC-2324] item 5):
//! whether an assertion over a challenge the route already took opens a reduction-only session,
//! and for whom.
//!
//! **No membership oracle, by structure.** Every request runs the same steps whatever it names:
//! the whole passkey fold is replayed and looked up by the passkey id; the signature is verified
//! by `mandate_passkey::reduction::verify_reduction` against the passkey's key, or, for an unknown
//! passkey or a key that does not resolve, parse, or match its recorded algorithm, against the
//! decoy its signature's encoding picks; the membership lookup and [DEC-2319]'s interim standing
//! run for the passkey's member, or for [`DECOY_MEMBER`]; and only then are the results judged,
//! together. A decoy's answer is never accepted.
//!
//! [DEC-2319]: ../../../docs/project/decisions/DEC-2319.md
//! [DEC-2324]: ../../../docs/project/decisions/DEC-2324.md

use mandate_authn::SubjectStanding;
use mandate_identity::{MembershipRecord, PrincipalId};
use mandate_journal::ArtifactSource;
use mandate_passkey::credentials::CredentialRecord;
use mandate_passkey::{Challenge, RelyingParty};
use mandate_time::UtcNanos;

use crate::members::Hosted;
use crate::standing::Standings;

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
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn judge(
    rp: &RelyingParty,
    challenge: &Challenge,
    presented: &Presented<'_>,
    reads: &Reads<'_>,
    now: UtcNanos,
) -> Option<Opened> {
    let _ = (rp, challenge, presented, reads, now);
    todo!()
}
