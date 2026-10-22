//! The route-2 gate in front of a reduction-only session (identity spec §6.4 route 2, DEC-833,
//! DEC-834, DEC-653): the challenges `POST /v1/reduction-sessions/challenges` issues, and the
//! limits and single-use rule `POST /v1/reduction-sessions` applies before and after the passkey
//! verifier runs. The caller hands in the time and the random challenge bytes, and the verifier as
//! a closure, so the gate reads no clock, draws no randomness, and holds nothing about a workspace.

use core::fmt;
use core::num::NonZeroU32;
use std::collections::{BTreeMap, BTreeSet};

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

/// The limits, none zero. A challenge counts against its issuing client's address and device
/// while it is outstanding; a failed assertion counts against its presenting client's address and
/// device for [`CHALLENGE_LIFETIME_S`] after it failed. The last three bound the gate in all
/// (workspace API spec §4.10 route 2 item 3, DEC-2119 items 4 and 11): at most `max_outstanding`
/// challenges outstanding; once `reserve` are, none more to an address that already holds one;
/// and failures kept for at most `max_failure_keys` addresses and devices together, the key whose
/// latest failure is oldest dropped first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReductionLimits {
    pub challenges_per_address: NonZeroU32,
    pub challenges_per_device: NonZeroU32,
    pub failures_per_address: NonZeroU32,
    pub failures_per_device: NonZeroU32,
    pub max_outstanding: NonZeroU32,
    pub reserve: NonZeroU32,
    pub max_failure_keys: NonZeroU32,
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
///
/// **Memory.** Every call that changes the gate first forgets the challenges that expired and the
/// failure keys whose latest failure lapsed by its `now`, so no key is held past
/// [`CHALLENGE_LIFETIME_S`] after its last failure; a kept key may still hold older lapsed
/// failures, within its limit, which no count reads. An
/// address holds at most `challenges_per_address` outstanding challenges and a device at most
/// `challenges_per_device`; an address keeps at most its `failures_per_address` latest failures and
/// a device its `failures_per_device` latest, since older ones can no longer change what
/// [`ReductionGate::failure_limit_reached`] answers. A flood from one address or one device
/// therefore holds a bounded state however fast it sends. The bound is per key, not across keys:
/// the state grows with the number of distinct addresses and devices seen in the last 300 s, at
/// most `challenges_per_address + failures_per_address` entries per address plus
/// `failures_per_device` per device. Both keys come from the client, so H1 must cap how many
/// distinct keys reach the gate (for example by aggregating addresses into prefixes and holding a
/// global ceiling). The gate expects `now` never to run backwards across calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReductionGate {
    limits: ReductionLimits,
    outstanding: BTreeMap<[u8; 32], Outstanding>,
    failures_by_address: BTreeMap<String, Vec<UtcNanos>>,
    failures_by_device: BTreeMap<String, Vec<UtcNanos>>,
    /// One entry per failure key held, by its latest failure, then its kind, then the key: the
    /// first is both the next to lapse and the next to drop, so neither scans the keys.
    by_latest: BTreeSet<(UtcNanos, Kind, String)>,
}

/// Which map a failure key is held in. An address orders before a device, so at a tie the
/// address is dropped first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Address,
    Device,
}

/// An issued challenge not yet consumed: when it expires and which client's slots it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Outstanding {
    expires_at: UtcNanos,
    issuer: ClientKey,
}

/// `at` plus [`CHALLENGE_LIFETIME_S`], or `None` past the last instant [`UtcNanos`] holds.
fn lifetime_after(at: UtcNanos) -> Option<UtcNanos> {
    let secs = at.secs().checked_add(CHALLENGE_LIFETIME_S)?;
    UtcNanos::from_parts(secs, at.nanos()).ok()
}

/// Whether a failure at `failed_at` still counts at `now`: for [`CHALLENGE_LIFETIME_S`] after it,
/// and for good when that end is past the last instant [`UtcNanos`] holds, so an overflow never
/// drops a failure early.
fn counts_at(failed_at: UtcNanos, now: UtcNanos) -> bool {
    lifetime_after(failed_at).is_none_or(|lapses_at| now < lapses_at)
}

/// Whether `held` entries fill `limit`.
fn fills(held: usize, limit: NonZeroU32) -> bool {
    usize::try_from(limit.get()).is_ok_and(|limit| held >= limit)
}

/// How many failures of `key` count at `now`.
fn failures_at(failures: &BTreeMap<String, Vec<UtcNanos>>, key: &str, now: UtcNanos) -> usize {
    failures.get(key).map_or(0, |times| {
        times
            .iter()
            .filter(|failed_at| counts_at(**failed_at, now))
            .count()
    })
}

impl ReductionGate {
    /// A gate with no outstanding challenge and no failure.
    pub fn new(limits: ReductionLimits) -> Self {
        Self {
            limits,
            outstanding: BTreeMap::new(),
            failures_by_address: BTreeMap::new(),
            failures_by_device: BTreeMap::new(),
            by_latest: BTreeSet::new(),
        }
    }

    /// The failures held for keys of `kind`.
    fn failures_mut(&mut self, kind: Kind) -> &mut BTreeMap<String, Vec<UtcNanos>> {
        match kind {
            Kind::Address => &mut self.failures_by_address,
            Kind::Device => &mut self.failures_by_device,
        }
    }

    /// Records a failure at `now` against `key` of `kind`, keeping only the `limit` latest failures
    /// of that key, and moves the key to its new place in [`Self::by_latest`].
    fn record_failure(&mut self, kind: Kind, key: &str, now: UtcNanos, limit: NonZeroU32) {
        let failures = match kind {
            Kind::Address => &mut self.failures_by_address,
            Kind::Device => &mut self.failures_by_device,
        };
        let times = failures.entry(key.to_owned()).or_default();
        if let Some(latest) = times.last() {
            self.by_latest.remove(&(*latest, kind, key.to_owned()));
        }
        times.push(now);
        times.sort_unstable();
        let surplus = times
            .len()
            .saturating_sub(usize::try_from(limit.get()).unwrap_or(usize::MAX));
        times.drain(..surplus);
        if let Some(latest) = times.last() {
            self.by_latest.insert((*latest, kind, key.to_owned()));
        }
    }

    /// Forgets every challenge expired at `now`, and every failure key whose latest failure lapsed
    /// by then, oldest first from [`Self::by_latest`]. A kept key's older failures that lapsed stay
    /// until its next failure pushes them out; no count reads them, since [`failures_at`] counts
    /// only those that still count, and a key holds at most its limit of them.
    fn forget_before(&mut self, now: UtcNanos) {
        self.outstanding.retain(|_, held| now < held.expires_at);
        while let Some((latest, _, _)) = self.by_latest.first() {
            if counts_at(*latest, now) {
                break;
            }
            let Some((_, kind, key)) = self.by_latest.pop_first() else {
                break;
            };
            self.failures_mut(kind).remove(&key);
        }
    }

    /// Issues `random` as a challenge for `client`, outstanding until `now` plus
    /// [`CHALLENGE_LIFETIME_S`]. Refused when the client's address or device already holds its
    /// limit of outstanding challenges (a consumed or expired one holds none), or when `random` is
    /// an outstanding challenge's bytes.
    pub fn issue(
        &mut self,
        client: &ClientKey,
        random: [u8; 32],
        now: UtcNanos,
    ) -> Result<Challenge, Unauthenticated> {
        self.forget_before(now);
        let issuers = || self.outstanding.values().map(|held| &held.issuer);
        let address_full = fills(
            issuers()
                .filter(|issuer| issuer.address == client.address)
                .count(),
            self.limits.challenges_per_address,
        );
        let device_full = fills(
            issuers()
                .filter(|issuer| issuer.device == client.device)
                .count(),
            self.limits.challenges_per_device,
        );
        let held = self.outstanding.len();
        let in_all = fills(held, self.limits.max_outstanding);
        let reserved = fills(held, self.limits.reserve)
            && issuers().any(|issuer| issuer.address == client.address);
        if address_full
            || device_full
            || in_all
            || reserved
            || self.outstanding.contains_key(&random)
        {
            return Err(Unauthenticated);
        }
        let expires_at = lifetime_after(now).ok_or(Unauthenticated)?;
        let issuer = client.clone();
        self.outstanding
            .insert(random, Outstanding { expires_at, issuer });
        Ok(Challenge {
            bytes: random,
            expires_at,
        })
    }

    /// Takes an assertion over `challenge`. The challenge is looked up and consumed first, whoever
    /// presents it and whatever follows. Only an outstanding, unexpired challenge reaches `verify`,
    /// once; `verify` returns `None` for an unknown credential ID, a removed or suspended row, a
    /// bad signature, or a refused subject. A verified assertion is answered `Ok` whatever the
    /// limits say (DEC-834); every other outcome is [`Unauthenticated`] and counts one failure
    /// against `client`.
    pub fn present<V>(
        &mut self,
        client: &ClientKey,
        challenge: &[u8; 32],
        now: UtcNanos,
        verify: impl FnOnce(&Challenge) -> Option<V>,
    ) -> Result<V, Unauthenticated> {
        self.forget_before(now);
        let verified = self.outstanding.remove(challenge).and_then(|held| {
            verify(&Challenge {
                bytes: *challenge,
                expires_at: held.expires_at,
            })
        });
        if let Some(verified) = verified {
            return Ok(verified);
        }
        let limits = self.limits;
        self.record_failure(
            Kind::Address,
            &client.address,
            now,
            limits.failures_per_address,
        );
        self.record_failure(
            Kind::Device,
            &client.device,
            now,
            limits.failures_per_device,
        );
        self.bound_failure_keys();
        Err(Unauthenticated)
    }

    /// Drops failure keys, the one whose latest failure is oldest first and an address before a
    /// device at a tie, until at most `max_failure_keys` addresses and devices are held: each drop
    /// is the first entry of [`Self::by_latest`], so the bound costs no scan of the keys. Dropping
    /// one changes no answer, since failures only report. The loop also ends if the index runs
    /// out first, which it cannot while it holds one entry per key, rather than spin.
    fn bound_failure_keys(&mut self) {
        let cap = usize::try_from(self.limits.max_failure_keys.get()).unwrap_or(usize::MAX);
        while self
            .failures_by_address
            .len()
            .saturating_add(self.failures_by_device.len())
            > cap
        {
            let Some((_, kind, key)) = self.by_latest.pop_first() else {
                break;
            };
            self.failures_mut(kind).remove(&key);
        }
    }

    /// Whether `client`'s address or device has reached its failure limit at `now`. No answer of
    /// either route depends on it, since every failure is already [`Unauthenticated`] and a
    /// verified assertion is never refused; the HTTP layer reads it to record abuse.
    pub fn failure_limit_reached(&self, client: &ClientKey, now: UtcNanos) -> bool {
        fills(
            failures_at(&self.failures_by_address, &client.address, now),
            self.limits.failures_per_address,
        ) || fills(
            failures_at(&self.failures_by_device, &client.device, now),
            self.limits.failures_per_device,
        )
    }
}
