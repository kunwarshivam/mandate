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
use std::sync::Mutex;

use mandate_authn::{
    EndReason, OrgKind, ProviderAnswer, RefreshSecret, Refreshed, Request, SessionLimits,
    SessionPolicy, SessionRecord, SessionRefusal,
};
use mandate_identity::{PrincipalId, SessionRef};
use mandate_time::UtcNanos;
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

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
    /// A token that verified, and the subject-link digest of its subject.
    Granted { subject: String },
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
}

struct Entry {
    record: SessionRecord,
    principal: PrincipalId,
    subject: String,
    reference: SessionRef,
    opened: u64,
}

/// The sessions this process holds.
pub struct Sessions {
    key: hmac::Key,
    random: Box<dyn Random>,
    limits: Result<SessionLimits, SessionRefusal>,
    entries: Mutex<(u64, BTreeMap<[u8; 16], Entry>)>,
}

impl Sessions {
    /// An empty store whose cookies are authenticated under `key`.
    pub fn new(key: &SessionKey, random: Box<dyn Random>) -> Self {
        Self {
            key: hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()),
            random,
            limits: SessionLimits::resolve(OrgKind::Business, SessionPolicy::default()),
            entries: Mutex::new((0, BTreeMap::new())),
        }
    }

    /// Opens a session for `principal`, whose subject's link digest is `subject`, at `now`, ending
    /// the session `replaced` names, if any.
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
        let _ = (
            principal,
            subject,
            replaced,
            now,
            &self.key,
            &self.random,
            &self.limits,
        );
        let _ = (EndReason::SignOut, ProviderAnswer::Granted, Request::Other);
        let _ = |_: Refreshed, _: RefreshSecret| ();
        Err(Unauthenticated)
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
        let _ = (cookie, answer, now);
        Err(Unauthenticated)
    }

    /// Ends the session `cookie` names (`sign_out`).
    ///
    /// # Errors
    /// [`Unauthenticated`] for a cookie that names no live session.
    pub fn end(&self, cookie: &str) -> Result<(), Unauthenticated> {
        let _ = cookie;
        Err(Unauthenticated)
    }

    /// Admits `request` on the session `cookie` names at `now`. A pause or kill switch on a full
    /// session whose access lapsed is served after an outage refresh the server records itself
    /// (§3.11 item 6).
    ///
    /// # Errors
    /// [`Unauthenticated`] for every refusal.
    pub fn authenticate(
        &self,
        cookie: &str,
        request: Request,
        now: UtcNanos,
    ) -> Result<Authenticated, Unauthenticated> {
        let _ = (cookie, request, now);
        Err(Unauthenticated)
    }

    /// How many sessions are live, for the tests' independent count.
    pub fn len(&self) -> usize {
        0
    }

    /// Whether no session is live.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The subject-link digest the session `cookie` names was opened for.
    ///
    /// # Errors
    /// [`Unauthenticated`] for a cookie that names no live session.
    pub fn subject(&self, cookie: &str) -> Result<String, Unauthenticated> {
        let _ = cookie;
        Err(Unauthenticated)
    }
}

/// Unused: keeps the entry's fields named until the implementation reads them.
fn _fields(entry: &Entry) -> (&SessionRecord, PrincipalId, &str, SessionRef, u64) {
    (
        &entry.record,
        entry.principal,
        &entry.subject,
        entry.reference,
        entry.opened,
    )
}
