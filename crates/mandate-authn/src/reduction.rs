//! The route-2 gate in front of a reduction-only session (identity spec §6.4 route 2, DEC-833,
//! DEC-834, DEC-653): the challenges `POST /v1/reduction-sessions/challenges` issues, and the
//! limits and single-use rule `POST /v1/reduction-sessions` applies before and after the passkey
//! verifier runs. The caller hands in the time and the random challenge bytes, and the verifier as
//! a closure, so the gate reads no clock, draws no randomness, and holds nothing about a workspace.

use core::fmt;
use core::num::NonZeroU32;

use crate::UtcNanos;

/// How long a challenge is outstanding, and how long a failed assertion counts, in seconds. It is
/// a constant, not a setting (identity spec §6.4 route 2).
pub const CHALLENGE_LIFETIME_S: i64 = 300;

/// The one answer every refusal of either route gives (DEC-816 item 6 for the assertion route,
/// DEC-653 item 3 for the challenge route): no variant, no field, and
/// one fixed `Debug`, `Display` and code, so no refusal says whether a credential exists, its row
/// was removed or suspended, its member was refused, or a limit was reached.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Unauthenticated;

impl Unauthenticated {
    /// The stable code the 401 answer carries.
    pub fn code(&self) -> &'static str {
        "unauthenticated"
    }
}

impl fmt::Debug for Unauthenticated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Unauthenticated")
    }
}

impl fmt::Display for Unauthenticated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unauthenticated")
    }
}

/// Who is asking, as the HTTP layer sees it: the client's address and its device, each limited on
/// its own. Both are opaque to the gate. Its `Debug` prints both, so H1 never logs one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientKey {
    pub address: String,
    pub device: String,
}

/// The four limits, none zero. A challenge counts against its issuing client's address and device
/// while it is outstanding; a failed assertion counts against its presenting client's address and
/// device for [`CHALLENGE_LIFETIME_S`] after it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReductionLimits {
    pub challenges_per_address: NonZeroU32,
    pub challenges_per_device: NonZeroU32,
    pub failures_per_address: NonZeroU32,
    pub failures_per_device: NonZeroU32,
}

/// What the challenge route returns, and all it returns: the bytes the caller drew and when they
/// stop being accepted, `issued + CHALLENGE_LIFETIME_S`. Its `Debug` prints the bytes, so H1 never
/// logs one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Challenge {
    pub bytes: [u8; 32],
    pub expires_at: UtcNanos,
}

/// The outstanding challenges and the recent failures, per address and per device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReductionGate {
    stub: (),
}

#[expect(
    clippy::todo,
    reason = "the gate's refusal is one opaque value with no Unimplemented variant (DEC-816 item 6) \
              and its constructor has no error, so these stubs are todo!(), the other form DEC-137 \
              names"
)]
impl ReductionGate {
    /// A gate with no outstanding challenge and no failure.
    pub fn new(_limits: ReductionLimits) -> Self {
        todo!()
    }

    /// Issues `random` as a challenge for `client`, outstanding until `now` plus
    /// [`CHALLENGE_LIFETIME_S`]. Refused when the client's address or device already holds its
    /// limit of outstanding challenges (a consumed or expired one holds none), or when `random` is
    /// an outstanding challenge's bytes.
    pub fn issue(
        &mut self,
        _client: &ClientKey,
        _random: [u8; 32],
        _now: UtcNanos,
    ) -> Result<Challenge, Unauthenticated> {
        todo!()
    }

    /// Takes an assertion over `challenge`. The challenge is looked up and consumed first, whoever
    /// presents it and whatever follows. Only an outstanding, unexpired challenge reaches `verify`,
    /// once; `verify` returns `None` for an unknown credential ID, a removed or suspended row, a
    /// bad signature, or a refused subject. A verified assertion is answered `Ok` whatever the
    /// limits say (DEC-834); every other outcome is [`Unauthenticated`] and counts one failure
    /// against `client`.
    pub fn present<V>(
        &mut self,
        _client: &ClientKey,
        _challenge: &[u8; 32],
        _now: UtcNanos,
        _verify: impl FnOnce(&Challenge) -> Option<V>,
    ) -> Result<V, Unauthenticated> {
        todo!()
    }

    /// Whether `client`'s address or device has reached its failure limit at `now`. No answer of
    /// either route depends on it, since every failure is already [`Unauthenticated`] and a
    /// verified assertion is never refused; the HTTP layer reads it to record abuse.
    pub fn failure_limit_reached(&self, _client: &ClientKey, _now: UtcNanos) -> bool {
        todo!()
    }
}
