//! Browser sessions (§3.11 items 5 and 6, DEC-978 items 5 and 6; identity spec §6.2, §6.4 route 1).
//!
//! A session is `mandate-authn`'s [`SessionRecord`], held in the process. The browser holds only
//! the `__Host-` cookie: an opaque 16-byte id and a 32-byte secret, authenticated with
//! HMAC-SHA256 under the session signing key, written as 160 lowercase hex digits. The record
//! keeps only the secret's digest, as a refresh token's. A cookie whose tag does not verify is
//! refused before the store is read.
//!
//! Every session runs under identity spec §6.2's business limits, the strictest, until the
//! organization's kind is read from its store. At most [`MAX_PER_PRINCIPAL`] sessions per
//! principal (a further one ends the oldest) and [`MAX_SESSIONS`] in all (a further one is
//! refused).

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use mandate_authn::{
    EndReason, OrgKind, ProviderAnswer, REDUCTION_ONLY_LIFETIME_S, Reach, RefreshSecret, Refreshed,
    Request, SessionLimits, SessionPolicy, SessionRecord, SessionRefusal, SubjectStanding,
};
use mandate_identity::{PrincipalId, SessionKind, SessionRef};
use mandate_time::UtcNanos;
use ring::rand::{SecureRandom, SystemRandom};
use ring::{digest, hmac};

use crate::config::SessionKey;

/// The session cookie's name (§3.3 item 5): host-only, `Secure`, `HttpOnly`, `SameSite=Strict`.
pub const COOKIE_NAME: &str = "__Host-mandate_session";

/// The most sessions one principal may hold; a further sign-in ends the oldest.
pub const MAX_PER_PRINCIPAL: usize = 16;

/// The most sessions the process holds; a further sign-in is refused.
pub const MAX_SESSIONS: usize = 4096;

/// How long the cookie lives in the browser, in seconds: the business absolute lifetime.
pub const COOKIE_MAX_AGE_S: i64 = 12 * 60 * 60;

/// Anything that refused authentication. It carries nothing: every refusal is the one 401.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unauthenticated;

/// Where the bytes of ids and secrets come from.
pub trait Random: Send + Sync {
    /// Fills `out`, or fails; a failure opens nothing.
    fn fill(&self, out: &mut [u8]) -> Result<(), Unauthenticated>;
}

/// The operating system's generator.
#[derive(Debug)]
pub struct OsRandom(SystemRandom);

impl OsRandom {
    pub fn new() -> Self {
        Self(SystemRandom::new())
    }
}

impl Default for OsRandom {
    fn default() -> Self {
        Self::new()
    }
}

impl Random for OsRandom {
    fn fill(&self, out: &mut [u8]) -> Result<(), Unauthenticated> {
        self.0.fill(out).map_err(|_| Unauthenticated)
    }
}

/// A cookie value to set.
#[derive(Clone, PartialEq, Eq)]
pub struct Cookie(String);

impl Cookie {
    /// The value, 160 hex digits.
    pub fn value(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Cookie {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cookie(..)")
    }
}

/// What a refresh presented (§3.11 item 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// A token that verified: the subject-link digest of its subject, and the principal the
    /// subject link maps it to now. Either differing from the session's ends it.
    Granted {
        subject: String,
        principal: PrincipalId,
    },
    /// `null`, or the issuer's keys could not be had: an outage.
    Unreachable,
    /// A token that did not verify, or a subject no longer linked.
    Failed,
}

/// Who a request is, once its session admits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Authenticated {
    pub principal: PrincipalId,
    pub reference: SessionRef,
    /// Full, or reduction-only once an outage narrowed it (identity spec §6.4 route 1).
    pub kind: SessionKind,
    /// The sign-in that opened the session; a refresh renews its tokens and leaves this
    /// ([DEC-2124](../../../docs/project/decisions/DEC-2124.md) item 4).
    pub signed_in: UtcNanos,
}

struct Entry {
    record: SessionRecord,
    principal: PrincipalId,
    /// The subject-link digest a sign-in opened it for; `None` for a route-2 session, which no
    /// subject opened.
    subject: Option<String>,
    /// The digest of a route-2 session's cookie secret, which its record keeps no refresh digest
    /// for; `Some` is also the marker that route 2 opened it, which no answer exposes.
    passkey: Option<[u8; 32]>,
    reference: SessionRef,
    opened: u64,
    /// When the sign-in opened it, which no refresh moves.
    signed_in: UtcNanos,
    absolute_until: i64,
    last_active: i64,
    /// The digest of the secret the last granted refresh rotated away, and when.
    previous: Option<([u8; 32], i64)>,
}

/// How long, in seconds, a cookie a granted refresh just rotated away still serves a pause or a
/// kill switch (DEC-1022). Both instants are compared in whole seconds, as the session's other
/// lifetimes are, so the window ends at the first second past this many after the rotation's.
pub const ROTATION_GRACE_S: i64 = 30;

const ID_BYTES: usize = 16;
const SECRET_BYTES: usize = 32;
const TAG_BYTES: usize = 32;
const COOKIE_HEX: usize = (ID_BYTES + SECRET_BYTES + TAG_BYTES) * 2;

type Id = [u8; ID_BYTES];
type Secret = [u8; SECRET_BYTES];

/// The live sessions, and a counter that orders their openings.
struct Store {
    opened: u64,
    entries: BTreeMap<Id, Entry>,
}

/// The sessions this process holds.
pub struct Sessions {
    key: hmac::Key,
    random: Box<dyn Random>,
    limits: Result<SessionLimits, SessionRefusal>,
    store: Mutex<Store>,
}

impl Sessions {
    /// An empty store whose cookies are authenticated under `key`.
    pub fn new(key: &SessionKey, random: Box<dyn Random>) -> Self {
        Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()),
            random,
            limits: SessionLimits::resolve(OrgKind::Business, SessionPolicy::default()),
            store: Mutex::new(Store {
                opened: 0,
                entries: BTreeMap::new(),
            }),
        }
    }

    /// The store, recovered if a panic poisoned its lock: it holds plain data, and refusing on a
    /// poisoned lock would refuse pause and the kill switch (`AGENTS.md` rule 13).
    fn store(&self) -> MutexGuard<'_, Store> {
        self.store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Opens a session for `principal`, whose subject's link digest is `subject`, at `now`, ending
    /// the session `replaced` names, if any: its tag verifying shows the server issued it to this
    /// browser, current or rotated. Everything that can fail is done before the store
    /// changes, so a refused open ends no session.
    ///
    /// # Errors
    /// [`Unauthenticated`] when the store is full or no randomness or record could be had.
    pub fn open(
        &self,
        principal: PrincipalId,
        subject: String,
        replaced: Option<&str>,
        now: UtcNanos,
    ) -> Result<Cookie, Unauthenticated> {
        let limits = self.limits.map_err(|_| Unauthenticated)?;
        let id: Id = self.fresh()?;
        let secret: Secret = self.fresh()?;
        let reference: [u8; 16] = self.fresh()?;
        let record = SessionRecord::open(limits, &RefreshSecret(secret), now)
            .map_err(|_| Unauthenticated)?;
        let entry = Entry {
            record,
            principal,
            subject: Some(subject),
            passkey: None,
            reference: SessionRef(u128::from_be_bytes(reference)),
            opened: 0,
            signed_in: now,
            absolute_until: now.secs().saturating_add(limits.absolute_s()),
            last_active: now.secs(),
            previous: None,
        };
        self.insert(id, entry, replaced, now)?;
        Ok(self.encode(&id, &secret))
    }

    /// Opens a reduction-only session (§4.10 route 2 item 5) for `principal`, whose passkey
    /// assertion the caller verified, at `now`, unless `standing` shows a deprovision signal at or
    /// after the subject's last successful sign-in. It lasts
    /// [`mandate_authn::REDUCTION_ONLY_LIFETIME_S`] from `now`, absolute, is never refreshed,
    /// reaches only pause, the kill switch, and sign-out, and counts toward `principal`'s
    /// [`MAX_PER_PRINCIPAL`]. It ends the session `replaced` names, as [`Sessions::open`] does, and
    /// only once everything that can fail has passed, so a refused open ends nothing.
    ///
    /// # Errors
    /// [`Unauthenticated`] for a deprovisioned subject, a full store, or no randomness.
    pub fn open_reduction_only(
        &self,
        principal: PrincipalId,
        standing: SubjectStanding,
        replaced: Option<&str>,
        now: UtcNanos,
    ) -> Result<Cookie, Unauthenticated> {
        let id: Id = self.fresh()?;
        let secret: Secret = self.fresh()?;
        let reference: [u8; 16] = self.fresh()?;
        let record =
            SessionRecord::open_reduction_only(standing, now).map_err(|_| Unauthenticated)?;
        let entry = Entry {
            record,
            principal,
            subject: None,
            passkey: Some(digest_of(&secret)),
            reference: SessionRef(u128::from_be_bytes(reference)),
            opened: 0,
            signed_in: now,
            absolute_until: now.secs().saturating_add(REDUCTION_ONLY_LIFETIME_S),
            last_active: now.secs(),
            previous: None,
        };
        self.insert(id, entry, replaced, now)?;
        Ok(self.encode(&id, &secret))
    }

    /// Holds `entry` under `id` at `now`, ending the session `replaced` names, if any, and
    /// `entry`'s principal's oldest when it already holds [`MAX_PER_PRINCIPAL`]. Nothing changes
    /// when the store is full.
    fn insert(
        &self,
        id: Id,
        mut entry: Entry,
        replaced: Option<&str>,
        now: UtcNanos,
    ) -> Result<(), Unauthenticated> {
        let limits = self.limits.map_err(|_| Unauthenticated)?;
        let replaced = replaced.and_then(|cookie| self.decode(cookie));
        let mut store = self.store();
        let idle_s = limits.idle_s();
        store
            .entries
            .retain(|_, held| live(held, now.secs(), idle_s));
        let replaced = replaced
            .map(|(id, _)| id)
            .filter(|id| store.entries.contains_key(id));
        let mut held: Vec<(u64, Id)> = store
            .entries
            .iter()
            .filter(|(id, held)| held.principal == entry.principal && Some(**id) != replaced)
            .map(|(id, held)| (held.opened, *id))
            .collect();
        held.sort_unstable();
        let evicted = (held.len() >= MAX_PER_PRINCIPAL)
            .then(|| held.first().map(|(_, id)| *id))
            .flatten();
        let freed = usize::from(replaced.is_some()).saturating_add(usize::from(evicted.is_some()));
        if store.entries.len().saturating_sub(freed) >= MAX_SESSIONS {
            return Err(Unauthenticated);
        }
        for id in [replaced, evicted].into_iter().flatten() {
            store.entries.remove(&id);
        }
        store.opened = store.opened.saturating_add(1);
        entry.opened = store.opened;
        store.entries.insert(id, entry);
        Ok(())
    }

    /// Whether the session `cookie` names is live at `now` and was opened by route 2, the marker
    /// no answer exposes: `POST /v1/sessions/refresh` answers it `reduction_only` and keeps it
    /// (§4.10 route 2 item 5). It changes nothing.
    pub fn opened_by_passkey(&self, cookie: &str, now: UtcNanos) -> bool {
        let Ok(limits) = self.limits else {
            return false;
        };
        let Some((id, secret)) = self.decode(cookie) else {
            return false;
        };
        self.store().entries.get(&id).is_some_and(|entry| {
            entry.passkey.is_some()
                && holds(entry, &secret)
                && live(entry, now.secs(), limits.idle_s())
        })
    }

    /// Refreshes the session `cookie` names with `answer` at `now`: a new cookie when its secret
    /// rotated, none when it now holds only pause and the kill switch.
    ///
    /// # Errors
    /// [`Unauthenticated`] for a cookie that names no live session, and for a refresh that ends
    /// it.
    pub fn refresh(
        &self,
        cookie: &str,
        answer: Answer,
        now: UtcNanos,
    ) -> Result<Option<Cookie>, Unauthenticated> {
        let (id, secret) = self.decode(cookie).ok_or(Unauthenticated)?;
        let next: Secret = self.fresh()?;
        let mut store = self.store();
        let entry = store.entries.get_mut(&id).ok_or(Unauthenticated)?;
        let provider = match answer {
            Answer::Granted { subject, principal }
                if Some(&subject) == entry.subject.as_ref() && principal == entry.principal =>
            {
                ProviderAnswer::Granted
            }
            Answer::Granted { .. } | Answer::Failed => ProviderAnswer::Status(FAILED_STATUS),
            Answer::Unreachable => ProviderAnswer::Unreachable,
        };
        let refreshed =
            entry
                .record
                .refresh(&RefreshSecret(secret), provider, &RefreshSecret(next), now);
        match refreshed {
            Ok(Refreshed::Rotated { .. }) => {
                entry.last_active = now.secs();
                entry.previous = Some((digest_of(&secret), now.secs()));
                Ok(Some(self.encode(&id, &next)))
            }
            Ok(Refreshed::Outage) => Ok(None),
            Err(_) => {
                if entry.record.ended().is_some() {
                    store.entries.remove(&id);
                }
                Err(Unauthenticated)
            }
        }
    }

    /// Ends the session `cookie` names (`sign_out`).
    ///
    /// # Errors
    /// [`Unauthenticated`] for a cookie that names no live session.
    pub fn end(&self, cookie: &str) -> Result<(), Unauthenticated> {
        let (id, secret) = self.decode(cookie).ok_or(Unauthenticated)?;
        let mut store = self.store();
        let mut entry = store.entries.remove(&id).ok_or(Unauthenticated)?;
        if !holds(&entry, &secret) {
            store.entries.insert(id, entry);
            return Err(Unauthenticated);
        }
        entry.record.end(EndReason::SignOut);
        Ok(())
    }

    /// Admits `request` on the session `cookie` names at `now`. A pause or kill switch on a full
    /// session whose access lapsed is served after an outage refresh the server records itself
    /// (§3.11 item 6); that refresh needs no randomness and the store's lock is recovered if
    /// poisoned, so neither can refuse risk reduction. A cookie whose tag verifies but whose
    /// secret is not the current one can only be one this session rotated away (the tag proves the
    /// server issued that id and secret together), and it ends the session, as reuse, with one
    /// exception (DEC-1022): for [`ROTATION_GRACE_S`] after a granted refresh, the secret that
    /// refresh rotated away still serves a pause or a kill switch in flight across it (with no
    /// outage refresh, which needs the current secret), and any other call with it is refused with
    /// the session kept. [`Sessions::refresh`] treats it as reuse even then.
    ///
    /// # Errors
    /// [`Unauthenticated`] for every refusal.
    pub fn authenticate(
        &self,
        cookie: &str,
        request: Request,
        now: UtcNanos,
    ) -> Result<Authenticated, Unauthenticated> {
        let (id, secret) = self.decode(cookie).ok_or(Unauthenticated)?;
        let mut store = self.store();
        let entry = store.entries.get_mut(&id).ok_or(Unauthenticated)?;
        let current = holds(entry, &secret);
        let graced = entry.previous.is_some_and(|(digest, rotated)| {
            digest == digest_of(&secret) && now.secs() <= rotated.saturating_add(ROTATION_GRACE_S)
        });
        let reducing = request != Request::Other;
        match (current, graced, reducing) {
            (true, _, _) | (false, true, true) => {}
            (false, true, false) => return Err(Unauthenticated),
            (false, false, _) => {
                store.entries.remove(&id);
                return Err(Unauthenticated);
            }
        }
        let admitted = if current {
            admitted_with_outage(entry, &secret, request, now)
        } else {
            entry.record.authorize(request, now)
        };
        match admitted {
            Ok(()) => {
                entry.last_active = entry.last_active.max(now.secs());
                Ok(Authenticated {
                    principal: entry.principal,
                    reference: entry.reference,
                    signed_in: entry.signed_in,
                    kind: match entry.record.reach() {
                        Reach::Full => SessionKind::Full,
                        Reach::Outage | Reach::ReductionOnly => SessionKind::ReductionOnly,
                    },
                })
            }
            Err(_) => {
                if entry.record.ended().is_some() {
                    store.entries.remove(&id);
                }
                Err(Unauthenticated)
            }
        }
    }

    /// Whether the session `cookie` names is live but reaches only pause and the kill switch (an
    /// outage or reduction-only session), so a refused ordinary request answers `reduction_only`
    /// rather than the one 401 (§3.5). It changes nothing.
    pub fn reduction_only(&self, cookie: &str, now: UtcNanos) -> bool {
        let Ok(limits) = self.limits else {
            return false;
        };
        let Some((id, secret)) = self.decode(cookie) else {
            return false;
        };
        self.store().entries.get(&id).is_some_and(|entry| {
            holds(entry, &secret)
                && entry.record.reach() != Reach::Full
                && live(entry, now.secs(), limits.idle_s())
        })
    }

    /// When the session `cookie` names ends if nothing touches it (DEC-2113 item 2): the earlier of
    /// its absolute lifetime and, for a full session, the idle timeout from its last admitted
    /// request or granted refresh, in whole seconds as its other lifetimes are. It changes nothing.
    ///
    /// # Errors
    /// [`Unauthenticated`] for a cookie that names no session live at `now` by its current secret.
    pub fn expires_at(&self, cookie: &str, now: UtcNanos) -> Result<UtcNanos, Unauthenticated> {
        let limits = self.limits.map_err(|_| Unauthenticated)?;
        let (id, secret) = self.decode(cookie).ok_or(Unauthenticated)?;
        let store = self.store();
        let entry = store
            .entries
            .get(&id)
            .filter(|entry| holds(entry, &secret) && live(entry, now.secs(), limits.idle_s()))
            .ok_or(Unauthenticated)?;
        let idle = if entry.record.reach() == Reach::Full {
            entry.last_active.saturating_add(limits.idle_s())
        } else {
            entry.absolute_until
        };
        UtcNanos::from_parts(entry.absolute_until.min(idle), 0).map_err(|_| Unauthenticated)
    }

    /// How many sessions are held, for the tests' independent count.
    pub fn len(&self) -> usize {
        self.store().entries.len()
    }

    /// Whether no session is held.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The subject-link digest the session `cookie` names was opened for.
    ///
    /// # Errors
    /// [`Unauthenticated`] for a cookie that names no live session.
    pub fn subject(&self, cookie: &str) -> Result<String, Unauthenticated> {
        let (id, secret) = self.decode(cookie).ok_or(Unauthenticated)?;
        let store = self.store();
        store
            .entries
            .get(&id)
            .filter(|entry| holds(entry, &secret))
            .and_then(|entry| entry.subject.clone())
            .ok_or(Unauthenticated)
    }

    fn fresh<const N: usize>(&self) -> Result<[u8; N], Unauthenticated> {
        let mut out = [0; N];
        self.random.fill(&mut out)?;
        Ok(out)
    }

    fn encode(&self, id: &Id, secret: &Secret) -> Cookie {
        let mut bytes = Vec::with_capacity(COOKIE_HEX / 2);
        bytes.extend_from_slice(id);
        bytes.extend_from_slice(secret);
        let tag = hmac::sign(&self.key, &bytes);
        bytes.extend_from_slice(tag.as_ref());
        Cookie(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }

    /// The id and secret of a cookie whose tag verifies under the session key; `None` for
    /// anything else, before any store is read.
    fn decode(&self, cookie: &str) -> Option<(Id, Secret)> {
        if cookie.len() != COOKIE_HEX
            || !cookie
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return None;
        }
        let bytes: Vec<u8> = cookie
            .as_bytes()
            .chunks(2)
            .filter_map(|pair| std::str::from_utf8(pair).ok())
            .filter_map(|pair| u8::from_str_radix(pair, 16).ok())
            .collect();
        let (body, tag) = bytes.split_at_checked(ID_BYTES + SECRET_BYTES)?;
        hmac::verify(&self.key, body, tag).ok()?;
        let (id, secret) = body.split_at_checked(ID_BYTES)?;
        Some((id.try_into().ok()?, secret.try_into().ok()?))
    }
}

/// The `next` secret an outage refresh requires and never installs: an outage keeps the current
/// secret, so this one is never presented and needs no randomness, which could otherwise fail and
/// refuse risk reduction.
const OUTAGE_NEXT: Secret = [0; SECRET_BYTES];

/// Whether the session can still admit anything at `now`: before its absolute end, and, while it
/// is full, before its idle timeout from its last admitted request or granted refresh. An outage
/// session has no idle timeout (identity spec §6.4 route 1).
fn live(entry: &Entry, now: i64, idle_s: i64) -> bool {
    let idle =
        entry.record.reach() == Reach::Full && now >= entry.last_active.saturating_add(idle_s);
    now < entry.absolute_until && !idle && entry.record.ended().is_none()
}

/// The status a failed refresh is recorded with: an answer that is neither a grant nor an outage.
const FAILED_STATUS: u16 = 401;

/// `request` on the session whose current secret is `secret`: a pause or kill switch on lapsed
/// access is served after an outage refresh the server records itself (§3.11 item 6).
fn admitted_with_outage(
    entry: &mut Entry,
    secret: &Secret,
    request: Request,
    now: UtcNanos,
) -> Result<(), SessionRefusal> {
    let admitted = entry.record.authorize(request, now);
    if admitted != Err(SessionRefusal::AccessExpired) || request == Request::Other {
        return admitted;
    }
    let outage = entry.record.refresh(
        &RefreshSecret(*secret),
        ProviderAnswer::Unreachable,
        &RefreshSecret(OUTAGE_NEXT),
        now,
    );
    match outage {
        Ok(Refreshed::Outage) => entry.record.authorize(request, now),
        _ => admitted,
    }
}

/// Whether `secret` is the session's current one: its refresh token's, or a route-2 session's
/// cookie secret.
fn holds(entry: &Entry, secret: &Secret) -> bool {
    entry.record.refresh_digest().or(entry.passkey) == Some(digest_of(secret))
}

fn digest_of(secret: &Secret) -> [u8; 32] {
    let mut out = [0; 32];
    out.copy_from_slice(digest::digest(&digest::SHA256, secret).as_ref());
    out
}
