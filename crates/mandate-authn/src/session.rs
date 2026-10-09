//! The server-side session record (identity spec §6.2) and the risk-reduction routes 1 and 2 of
//! §6.4, as pure state transitions: the caller hands in the time and the fresh refresh secrets its
//! randomness drew, and journals what each transition returns.

use core::fmt;
use std::collections::BTreeSet;

use ring::digest::{SHA256, digest};

use mandate_identity::{Membership, Session, SessionKind, SessionRef};
use mandate_identity_seal::Seal;

use crate::UtcNanos;

/// How long an access token lasts, in seconds, for every organization (§6.2: no org changes it).
pub const ACCESS_TOKEN_LIFETIME_S: i64 = 300;

/// How long a reduction-only session (§6.4 route 2) lasts, in seconds, from its opening.
pub const REDUCTION_ONLY_LIFETIME_S: i64 = 900;

const INDIVIDUAL_IDLE_S: i64 = 86_400;
const INDIVIDUAL_ABSOLUTE_S: i64 = 604_800;
const BUSINESS_IDLE_S: i64 = 3_600;
const BUSINESS_ABSOLUTE_S: i64 = 43_200;
/// The provider statuses that mean it is unavailable, an outage (§6.4 route 1), not a refusal.
const OUTAGE_STATUS: std::ops::RangeInclusive<u16> = 500..=599;

/// The kind of organization a workspace belongs to, which sets its session defaults (§6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrgKind {
    /// A retail sign-up's organization: idle 24 h, absolute 7 days.
    Individual,
    /// Every other organization: idle 1 h, absolute 12 h.
    Business,
}

/// What an organization set, in seconds; `None` keeps its kind's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionPolicy {
    pub idle_s: Option<i64>,
    pub absolute_s: Option<i64>,
}

/// The idle and absolute limits a session runs under, never longer than its org kind's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLimits {
    idle_s: i64,
    absolute_s: i64,
}

/// One limit: the org's setting when it is positive and no longer than the default.
fn shortened(default_s: i64, setting: Option<i64>) -> Result<i64, SessionRefusal> {
    match setting {
        None => Ok(default_s),
        Some(s) if s > 0 && s <= default_s => Ok(s),
        Some(_) => Err(SessionRefusal::LimitNotShortened),
    }
}

impl SessionLimits {
    /// The limits for `kind` under `policy`. A setting longer than the default, or not positive,
    /// is refused: an organization may only shorten (§6.2). So is an idle timeout longer than the
    /// absolute lifetime it would run inside (DEC-652 item 2).
    pub fn resolve(kind: OrgKind, policy: SessionPolicy) -> Result<Self, SessionRefusal> {
        let (idle_s, absolute_s) = match kind {
            OrgKind::Individual => (INDIVIDUAL_IDLE_S, INDIVIDUAL_ABSOLUTE_S),
            OrgKind::Business => (BUSINESS_IDLE_S, BUSINESS_ABSOLUTE_S),
        };
        let limits = Self {
            idle_s: shortened(idle_s, policy.idle_s)?,
            absolute_s: shortened(absolute_s, policy.absolute_s)?,
        };
        if limits.idle_s > limits.absolute_s {
            return Err(SessionRefusal::IdleLongerThanAbsolute);
        }
        Ok(limits)
    }

    /// The idle timeout, in seconds.
    pub fn idle_s(&self) -> i64 {
        self.idle_s
    }

    /// The absolute lifetime, in seconds.
    pub fn absolute_s(&self) -> i64 {
        self.absolute_s
    }
}

/// A refresh token's secret, drawn by the caller. The session keeps only its SHA-256 digest, and
/// its `Debug` prints nothing of it (ID-9).
#[derive(Clone, PartialEq, Eq)]
pub struct RefreshSecret(pub [u8; 32]);

impl fmt::Debug for RefreshSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RefreshSecret(..)")
    }
}

/// A refresh token's SHA-256 digest, the only form a session record keeps of it.
pub type RefreshDigest = [u8; 32];

/// What a request asks to do, as far as a session's reach goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Pause an agent.
    Pause,
    /// Engage a kill switch (its stop and flatten).
    KillSwitch,
    /// Anything else.
    Other,
}

/// How far a session reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Every permission the principal's roles give.
    Full,
    /// §6.4 route 1: a refresh found the identity provider unreachable, so only pause and the kill
    /// switch remain, until the absolute lifetime ends or a later refresh is granted.
    Outage,
    /// §6.4 route 2: opened by a workspace-local passkey, pause and the kill switch only, for
    /// [`REDUCTION_ONLY_LIFETIME_S`], with no refresh.
    ReductionOnly,
}

/// What the identity provider answered to a refresh, as the caller saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAnswer {
    /// The provider issued new tokens.
    Granted,
    /// No answer: a timeout, or a connection, TLS, or name-resolution failure.
    Unreachable,
    /// An HTTP answer that granted nothing and named no deprovision, with its status code. A 5xx
    /// is an outage; any other status, a 408 or a 429 included, is a failed refresh that ends the
    /// session but is not a deprovision signal (§6.4 route 1, DEC-652 item 3).
    Status(u16),
    /// A deprovision signal (§11.1): `invalid_grant`, or a disabled or revoked subject. It ends
    /// the session and closes route 2 for the subject until its next successful sign-in.
    Deprovision,
}

/// Why a session ended, as `SessionRevoked.reason` journals it (§12.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    SignOut,
    Deactivated,
    /// The provider sent a deprovision signal on a refresh, or a back-channel logout.
    Deprovisioned,
    /// A refresh got an answer that was neither a grant, an outage, nor a deprovision signal.
    RefreshFailed,
    /// A rotated refresh token was presented again: the whole family is revoked.
    RefreshReuse,
    /// The idle timeout or the absolute lifetime passed, or a granted refresh found an outage
    /// session's idle timeout lapsed (DEC-816 item 8).
    Expired,
    Admin,
}

impl EndReason {
    /// The reason code `SessionRevoked` carries.
    pub fn code(self) -> &'static str {
        match self {
            Self::SignOut => "sign_out",
            Self::Deactivated => "deactivated",
            Self::Deprovisioned => "deprovisioned",
            Self::RefreshFailed => "refresh_failed",
            Self::RefreshReuse => "refresh_reuse",
            Self::Expired => "expired",
            Self::Admin => "admin",
        }
    }
}

/// What a successful refresh did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refreshed {
    /// The refresh token rotated and a new access token lasts until `access_expires_at`.
    Rotated { access_expires_at: UtcNanos },
    /// The provider was unreachable: the session keeps pause and the kill switch only, and the
    /// presented refresh token stays current so a later refresh can restore it.
    Outage,
}

/// When the workspace last saw this principal's identity-provider subject sign in successfully,
/// and last saw a deprovision signal for it (§6.4 route 2, §11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SubjectStanding {
    pub last_sign_in: Option<UtcNanos>,
    pub last_deprovision: Option<UtcNanos>,
}

impl SubjectStanding {
    /// The standing after the workspace saw a deprovision signal for this subject at `now`
    /// (`invalid_grant`, a disabled or revoked subject, a back-channel logout), whatever state
    /// any session of it is in, or with none open at all (§11.1, DEC-652 item 7). The latest
    /// signal is kept, so an older one arriving late moves nothing back.
    pub fn saw_deprovision(self, now: UtcNanos) -> Self {
        let latest = self.last_deprovision.map_or(now, |seen| seen.max(now));
        Self {
            last_deprovision: Some(latest),
            ..self
        }
    }
}

/// Every way a session transition refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SessionRefusal {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// An organization tried to lengthen a limit, or set one that is not positive.
    #[error("the session limit may only be shortened")]
    LimitNotShortened,
    /// The idle timeout would be longer than the absolute lifetime.
    #[error("the idle timeout is longer than the absolute lifetime")]
    IdleLongerThanAbsolute,
    /// `now` is earlier than the session's opening or its last admitted activity: the clock
    /// went backwards, and the session refuses rather than guess. Only [`Request::Other`] and a
    /// refresh are refused this way; a pause or a kill switch is judged at the last activity
    /// instead, so a backwards clock never refuses risk reduction (rule 13, DEC-652 item 5).
    #[error("the clock is behind the session")]
    ClockBehind,
    /// The refresh secret to rotate to is the presented one or one already rotated away.
    #[error("the next refresh secret was used before")]
    RefreshSecretReused,
    /// A time this transition would compute is outside the clock's range.
    #[error("the session's times cannot be represented")]
    Unrepresentable,
    /// The session ended. The call that ends it returns this too, and so does every later one;
    /// the caller journals the reason once, when [`SessionRecord::ended`] first turns `Some`.
    #[error("the session has ended")]
    Ended { reason: EndReason },
    /// The access token expired; a refresh is needed.
    #[error("the access token has expired")]
    AccessExpired,
    /// The request needs a full session, and this one reaches pause and the kill switch only.
    #[error("this session reaches pause and the kill switch only")]
    RiskReductionOnly,
    /// A reduction-only session has no refresh token.
    #[error("this session cannot be refreshed")]
    NotRefreshable,
    /// The presented refresh token is neither current nor rotated.
    #[error("the refresh token is not this session's")]
    UnknownRefreshToken,
    /// The provider sent a deprovision signal for this subject since its last successful sign-in,
    /// so the local passkey route is closed to it. Route 2 answers it, like every refusal, with
    /// the one [`crate::Unauthenticated`] (DEC-816 item 6).
    #[error("the identity provider deprovisioned this subject since its last sign-in")]
    DeprovisionSeen,
}

type Digest = RefreshDigest;

fn digest_of(secret: &RefreshSecret) -> Digest {
    let mut out = [0; 32];
    out.copy_from_slice(digest(&SHA256, &secret.0).as_ref());
    out
}

/// `now` plus `secs` seconds, but never past `cap`: a deadline inside a session's absolute
/// lifetime, so the end of the clock's range never overflows it (rule 13).
fn capped(now: UtcNanos, secs: i64, cap: UtcNanos) -> UtcNanos {
    plus(now, secs).map_or(cap, |t| t.min(cap))
}

/// `t` plus `secs` seconds, or a refusal when the clock cannot hold it.
fn plus(t: UtcNanos, secs: i64) -> Result<UtcNanos, SessionRefusal> {
    t.secs()
        .checked_add(secs)
        .and_then(|s| UtcNanos::from_parts(s, t.nanos()).ok())
        .ok_or(SessionRefusal::Unrepresentable)
}

/// An access token's deadline from `now`; past the clock's range, the absolute lifetime `cap`,
/// which ends the session first anyway.
fn access_deadline(now: UtcNanos, cap: UtcNanos) -> UtcNanos {
    plus(now, ACCESS_TOKEN_LIFETIME_S).unwrap_or(cap)
}

/// One session's server-side record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    reach: Reach,
    ended: Option<EndReason>,
    idle_s: i64,
    /// The opening, then the last admitted request or granted refresh: the idle timer runs from
    /// it, and a `now` before it is a clock gone backwards.
    last_activity: UtcNanos,
    absolute_until: UtcNanos,
    idle_until: UtcNanos,
    access_until: UtcNanos,
    /// The current refresh token's digest; `None` for a reduction-only session.
    current: Option<Digest>,
    /// Every digest rotated away, so presenting one again is detected as reuse.
    rotated: BTreeSet<Digest>,
}

impl SessionRecord {
    /// Opens a full session at `now` after a sign-in, holding `refresh`'s digest as the current
    /// refresh token, with an access token lasting [`ACCESS_TOKEN_LIFETIME_S`].
    pub fn open(
        limits: SessionLimits,
        refresh: &RefreshSecret,
        now: UtcNanos,
    ) -> Result<Self, SessionRefusal> {
        let absolute_until = plus(now, limits.absolute_s)?;
        Ok(Self {
            reach: Reach::Full,
            ended: None,
            idle_s: limits.idle_s,
            last_activity: now,
            absolute_until,
            idle_until: capped(now, limits.idle_s, absolute_until),
            access_until: access_deadline(now, absolute_until),
            current: Some(digest_of(refresh)),
            rotated: BTreeSet::new(),
        })
    }

    /// Opens a reduction-only session (§6.4 route 2) after the caller verified a fresh passkey
    /// assertion locally, unless a deprovision signal for the subject came at or after its last
    /// successful sign-in.
    pub fn open_reduction_only(
        standing: SubjectStanding,
        now: UtcNanos,
    ) -> Result<Self, SessionRefusal> {
        if let Some(refusal) = standing.last_deprovision
            && standing
                .last_sign_in
                .is_none_or(|sign_in| refusal >= sign_in)
        {
            return Err(SessionRefusal::DeprovisionSeen);
        }
        let until = plus(now, REDUCTION_ONLY_LIFETIME_S)?;
        Ok(Self {
            reach: Reach::ReductionOnly,
            ended: None,
            idle_s: REDUCTION_ONLY_LIFETIME_S,
            last_activity: now,
            absolute_until: until,
            idle_until: until,
            access_until: until,
            current: None,
            rotated: BTreeSet::new(),
        })
    }

    /// How far the session reaches now.
    pub fn reach(&self) -> Reach {
        self.reach
    }

    /// The SHA-256 digest of the current refresh token, the only form the record keeps of it;
    /// `None` for a reduction-only session, which has none.
    pub fn refresh_digest(&self) -> Option<RefreshDigest> {
        self.current
    }

    /// The digests of every refresh token rotated away, in byte order, kept to detect reuse.
    pub fn rotated_digests(&self) -> Vec<RefreshDigest> {
        self.rotated.iter().copied().collect()
    }

    /// Why the session ended, if it has.
    pub fn ended(&self) -> Option<EndReason> {
        self.ended
    }

    /// Whether a step-up may be presented through this session: only a full one (§7.3), and an
    /// ended session presents none.
    pub fn can_present_step_up(&self) -> bool {
        self.reach == Reach::Full && self.ended.is_none()
    }

    /// Ends the session for `reason` and returns the refusal that call and every later one gets.
    fn revoke(&mut self, reason: EndReason) -> SessionRefusal {
        self.ended = Some(reason);
        SessionRefusal::Ended { reason }
    }

    /// Ends the session as [`EndReason::Expired`] (DEC-816 item 8).
    fn expire(&mut self) -> SessionRefusal {
        self.revoke(EndReason::Expired)
    }

    /// The refusal an ended session gives every call.
    fn still_open(&self) -> Result<(), SessionRefusal> {
        match self.ended {
            Some(reason) => Err(SessionRefusal::Ended { reason }),
            None => Ok(()),
        }
    }

    /// Ends the session at `at` if any session's absolute lifetime, or a full session's idle
    /// timeout, has passed.
    fn lapse(&mut self, at: UtcNanos) -> Result<(), SessionRefusal> {
        let idle_lapsed = self.reach == Reach::Full && at >= self.idle_until;
        if at >= self.absolute_until || idle_lapsed {
            return Err(self.expire());
        }
        Ok(())
    }

    /// Counts an admitted request or a granted refresh at `now` as activity, restarting the idle
    /// timer, which never runs past the absolute lifetime.
    fn active_at(&mut self, now: UtcNanos) {
        self.last_activity = now;
        self.idle_until = capped(now, self.idle_s, self.absolute_until);
    }

    /// Admits `request` at `now`, and on success counts it as activity for the idle timeout. A
    /// request at or after the absolute lifetime, or a full session's idle timeout, ends the
    /// session with [`EndReason::Expired`] (DEC-816 item 8). A [`Request::Pause`] or
    /// [`Request::KillSwitch`] at a `now` earlier than the last activity is judged at the last
    /// activity instead of `now`, never [`SessionRefusal::ClockBehind`], and moves no timer; every
    /// other rule still applies at that instant. A live session refuses any other request at such
    /// a `now` with [`SessionRefusal::ClockBehind`], whatever its reach (DEC-652 item 5).
    pub fn authorize(&mut self, request: Request, now: UtcNanos) -> Result<(), SessionRefusal> {
        self.still_open()?;
        let judged_at = if now >= self.last_activity {
            now
        } else if request == Request::Other {
            return Err(SessionRefusal::ClockBehind);
        } else {
            self.last_activity
        };
        self.lapse(judged_at)?;
        if self.reach == Reach::Full {
            if judged_at >= self.access_until {
                return Err(SessionRefusal::AccessExpired);
            }
        } else if request == Request::Other {
            return Err(SessionRefusal::RiskReductionOnly);
        }
        self.active_at(judged_at);
        Ok(())
    }

    /// Refreshes with the `presented` refresh token, given the provider's `answer`, rotating to
    /// `next` when it grants. A rotated token presented again ends the session with
    /// [`EndReason::RefreshReuse`]; a deprovision signal ends it with
    /// [`EndReason::Deprovisioned`], and any other answer that is not an outage with
    /// [`EndReason::RefreshFailed`]. A refresh at or after the absolute lifetime or a full
    /// session's idle timeout, or a grant to an outage session whose idle timeout lapsed, ends it
    /// with [`EndReason::Expired`] (DEC-816 item 8). A `now` earlier than the last activity is
    /// refused [`SessionRefusal::ClockBehind`] and changes nothing. A deprovision or a failed
    /// answer ends the session before `next` is checked, so a `next` used before is refused
    /// [`SessionRefusal::RefreshSecretReused`] only on a grant or an outage (DEC-658).
    pub fn refresh(
        &mut self,
        presented: &RefreshSecret,
        answer: ProviderAnswer,
        next: &RefreshSecret,
        now: UtcNanos,
    ) -> Result<Refreshed, SessionRefusal> {
        self.still_open()?;
        if now < self.last_activity {
            return Err(SessionRefusal::ClockBehind);
        }
        self.lapse(now)?;
        let Some(current) = self.current else {
            return Err(SessionRefusal::NotRefreshable);
        };
        let presented = digest_of(presented);
        if presented != current {
            if self.rotated.contains(&presented) {
                return Err(self.revoke(EndReason::RefreshReuse));
            }
            return Err(SessionRefusal::UnknownRefreshToken);
        }
        match answer {
            ProviderAnswer::Deprovision => return Err(self.revoke(EndReason::Deprovisioned)),
            ProviderAnswer::Status(code) if !OUTAGE_STATUS.contains(&code) => {
                return Err(self.revoke(EndReason::RefreshFailed));
            }
            ProviderAnswer::Granted | ProviderAnswer::Unreachable | ProviderAnswer::Status(_) => {}
        }
        let next = digest_of(next);
        if next == current || self.rotated.contains(&next) {
            return Err(SessionRefusal::RefreshSecretReused);
        }
        match answer {
            ProviderAnswer::Granted if now >= self.idle_until => Err(self.expire()),
            ProviderAnswer::Granted => {
                let access_until = access_deadline(now, self.absolute_until);
                self.access_until = access_until;
                self.rotated.insert(current);
                self.current = Some(next);
                self.reach = Reach::Full;
                self.active_at(now);
                Ok(Refreshed::Rotated {
                    access_expires_at: access_until,
                })
            }
            ProviderAnswer::Unreachable | ProviderAnswer::Status(_) => {
                self.reach = Reach::Outage;
                Ok(Refreshed::Outage)
            }
            ProviderAnswer::Deprovision => Err(self.revoke(EndReason::Deprovisioned)),
        }
    }

    /// Admits `request` at `now` as [`SessionRecord::authorize`] does, and on success builds the
    /// identity session `authorize` in `mandate-identity` reads: the `reference` the caller's store
    /// keeps for this record, its kind
    /// ([`Reach::Full`] is [`SessionKind::Full`]; an outage and a reduction-only session are
    /// [`SessionKind::ReductionOnly`]), and the roles snapshot the caller read for this request,
    /// passed through as read: the caller (H1) builds it, for route 2 from every workspace where
    /// the verified credential has an unsuspended row. A refusal builds nothing, so the request
    /// gets no session (DEC-652 item 9).
    pub fn admit(
        &mut self,
        request: Request,
        now: UtcNanos,
        reference: SessionRef,
        snapshot: Vec<Membership>,
    ) -> Result<Session, SessionRefusal> {
        self.authorize(request, now)?;
        let kind = match self.reach {
            Reach::Full => SessionKind::Full,
            Reach::Outage | Reach::ReductionOnly => SessionKind::ReductionOnly,
        };
        Ok(Session::new(Seal::grant(), reference, kind, snapshot))
    }

    /// Ends the session for `reason`: sign-out, deactivation, a back-channel logout, or an admin.
    /// Returns the reason to journal, or `None` when it had already ended, so it is journaled once.
    pub fn end(&mut self, reason: EndReason) -> Option<EndReason> {
        if self.ended.is_some() {
            return None;
        }
        self.ended = Some(reason);
        Some(reason)
    }
}
