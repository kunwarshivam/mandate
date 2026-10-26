//! The step-up challenge route (workspace API spec §3.6, identity spec §7.2 steps 1 and 2,
//! [DEC-2103](../../../docs/project/decisions/DEC-2103.md)): `POST
//! /v1/workspaces/{ws}/step-up/challenges`, and the store the challenges it issues are held in.
//!
//! A challenge is issued only through `mandate_passkey::stepup::issue_challenge`, under the
//! `TenantContext` `authorize` returned for the §4.2 row the action's kind needs (DEC-665), so a
//! principal that may not perform an action cannot obtain a challenge for it. Its id is §3.4's
//! derived id for the request's key, so a retry answers the challenge already issued. The store
//! is the server's memory: a challenge it does not hold can only make a step-up count as missing.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use axum::body::Body;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use mandate_api::envelope::StepUpAction;
use mandate_api::problem::ProblemCode;
use mandate_api::requests::StepUpChallengeRequest;
use mandate_api::wire::{decode, encode};
use mandate_authn::Request;
use mandate_canon::{Digest, Value};
use mandate_identity::{AssertionId, PrincipalId, StepUpActionKind, TenantContext, WorkspaceId};
use mandate_passkey::stepup::{
    self, ChallengeRecord, IssueRefusal, issue_challenge, permission_for,
};
use mandate_time::UtcNanos;

use crate::backtests::paper;
use crate::http::{App, invalid, problem, problem_body};
use crate::passkeys::{FRESH_S, TOMBSTONE_S, TOMBSTONES_PER_PRINCIPAL, later};
use crate::routes::{Refused, Unreadable, authorized, enveloped, path_refused, string};
use crate::session::{OsRandom, Random};
use crate::writes::{derived_for, json_string, ref_text, strict};

/// §3.4's operation name for the route (DEC-2103 item 1).
pub const OPERATION: &str = "step_up_challenge";

/// The most unexpired challenges one principal holds (DEC-2103 item 5).
pub const PER_PRINCIPAL: usize = 32;

/// The most unexpired challenges the store holds in all (DEC-2103 item 5).
pub const HELD: usize = 4_096;

/// Why the store issued no challenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Withheld {
    /// The context was not authorized for the row the action's kind needs (DEC-665).
    Refused,
    /// The id is held for another action or another principal: `idempotency_conflict`.
    Conflict,
    /// Past the store's bound for the principal or in all: `rate_limited`.
    Full,
    /// The challenge would expire after the last instant the journal can record.
    OutOfRange,
    /// The operating system's generator gave no bytes for a registration challenge.
    NoRandom,
}

/// A passkey registration challenge (workspace API §3.6, DEC-2108 item 4): 32 random bytes, bound
/// to one principal and workspace, held 300 s, and never journaled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    /// §3.4's derived id for the request's key.
    pub id: String,
    pub principal: PrincipalId,
    pub workspace: WorkspaceId,
    /// The bytes the authenticator signs.
    pub challenge: [u8; 32],
    pub issued_at: UtcNanos,
    pub expires_at: UtcNanos,
    /// The order the store issued it in, which orders tombstones dropped at one instant.
    order: u64,
}

/// What the store holds under one id.
#[derive(Debug, Clone)]
enum Held {
    StepUp(ChallengeRecord),
    Registration(Registration),
}

impl Held {
    fn principal(&self) -> PrincipalId {
        match self {
            Self::StepUp(record) => *record.principal_id(),
            Self::Registration(registration) => registration.principal,
        }
    }

    fn expires_at(&self) -> UtcNanos {
        match self {
            Self::StepUp(record) => record.expires_at(),
            Self::Registration(registration) => registration.expires_at,
        }
    }
}

/// A dropped or expired registration challenge's id, kept so its key never revives it (DEC-2118
/// item 1), and when it is forgotten.
#[derive(Debug, Clone)]
struct Tombstone {
    id: String,
    /// 24 hours after the instant its challenge was dropped; `None` when that is past the last
    /// instant the journal can record, so it is never forgotten.
    forgotten_at: Option<UtcNanos>,
}

/// The store's state behind its lock.
#[derive(Debug, Default)]
struct Store {
    held: BTreeMap<String, Held>,
    tombstones: BTreeMap<PrincipalId, VecDeque<Tombstone>>,
    issued: u64,
}

impl Store {
    /// Drops what has expired at `now`: a registration challenge becomes a tombstone at the
    /// instant it expired, oldest first, and a tombstone is forgotten 24 hours after its drop or
    /// past a principal's 64.
    fn sweep(&mut self, now: UtcNanos) {
        let expired: Vec<String> = self
            .held
            .iter()
            .filter(|(_, held)| now >= held.expires_at())
            .map(|(id, _)| id.clone())
            .collect();
        let mut dropped: Vec<Registration> = expired
            .iter()
            .filter_map(|id| match self.held.remove(id) {
                Some(Held::Registration(registration)) => Some(registration),
                Some(Held::StepUp(_)) | None => None,
            })
            .collect();
        dropped.sort_by_key(|registration| (registration.expires_at, registration.order));
        for registration in dropped {
            self.bury(
                registration.principal,
                registration.id,
                registration.expires_at,
            );
        }
        for stones in self.tombstones.values_mut() {
            stones.retain(|stone| stone.forgotten_at.is_none_or(|forgotten| now < forgotten));
        }
        self.tombstones.retain(|_, stones| !stones.is_empty());
    }

    fn bury(&mut self, principal: PrincipalId, id: String, dropped: UtcNanos) {
        let stones = self.tombstones.entry(principal).or_default();
        stones.push_back(Tombstone {
            id,
            forgotten_at: later(dropped, TOMBSTONE_S),
        });
        while stones.len() > TOMBSTONES_PER_PRINCIPAL {
            stones.pop_front();
        }
    }

    fn buried(&self, principal: PrincipalId, id: &str) -> bool {
        self.tombstones
            .get(&principal)
            .is_some_and(|stones| stones.iter().any(|stone| stone.id == id))
    }

    /// Whether one more for `principal` would pass either bound.
    fn full(&self, principal: PrincipalId, per_principal: usize, in_all: usize) -> bool {
        let theirs = self
            .held
            .values()
            .filter(|held| held.principal() == principal)
            .count();
        theirs >= per_principal || self.held.len() >= in_all
    }
}

/// The challenges the server has issued and not yet seen expire, by challenge id, with the most
/// one principal and all of them may hold: step-up challenges and passkey registration
/// challenges, counted together (DEC-2115 item 5), and the registration challenges' tombstones.
pub struct Challenges {
    store: Mutex<Store>,
    per_principal: usize,
    in_all: usize,
    random: Box<dyn Random>,
}

impl Default for Challenges {
    /// The deployment's store: [`PER_PRINCIPAL`] and [`HELD`].
    fn default() -> Self {
        Self::bounded(PER_PRINCIPAL, HELD)
    }
}

impl Challenges {
    /// An empty store holding at most `per_principal` unexpired challenges for one principal and
    /// `in_all` in all.
    pub fn bounded(per_principal: usize, in_all: usize) -> Self {
        Self {
            store: Mutex::new(Store::default()),
            per_principal,
            in_all,
            random: Box::new(OsRandom::new()),
        }
    }

    fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The challenge `id` names for `action` under `context` at `now`: the one already held when it
    /// binds the same principal, workspace, and action, else a new one from `issue_challenge`.
    /// Expired challenges are dropped first, so a bound counts only unexpired ones.
    ///
    /// # Errors
    /// [`Withheld`], with nothing held changed but the expired dropped.
    pub fn issue(
        &self,
        context: &TenantContext,
        action: stepup::Action,
        id: AssertionId,
        now: UtcNanos,
    ) -> Result<ChallengeRecord, Withheld> {
        let mut store = self.store();
        store.sweep(now);
        if let Some(held) = store.held.get(&id.0) {
            return match held {
                Held::StepUp(record)
                    if record.principal_id() == &context.principal()
                        && record.workspace_id() == &context.workspace()
                        && record.action() == action =>
                {
                    Ok(record.clone())
                }
                Held::StepUp(_) | Held::Registration(_) => Err(Withheld::Conflict),
            };
        }
        if store.full(context.principal(), self.per_principal, self.in_all) {
            return Err(Withheld::Full);
        }
        let issued =
            issue_challenge(context, action, id, now).map_err(|refusal| match refusal {
                IssueRefusal::Refused(_) => Withheld::Refused,
                IssueRefusal::Issue(_) => Withheld::OutOfRange,
            })?;
        let record = issued.record().clone();
        store.held.insert(
            record.challenge_id().0.clone(),
            Held::StepUp(record.clone()),
        );
        Ok(record)
    }

    /// The registration challenge `id` names for `principal` in `workspace` at `now`: the one
    /// held when it binds the same principal and workspace, unchanged, else 32 fresh bytes held
    /// 300 s (DEC-2108 item 4, DEC-2118 item 1). An id held for anything else, or kept as a
    /// tombstone of `principal`'s, is a conflict, so a key never revives a dropped challenge.
    ///
    /// # Errors
    /// [`Withheld`], with nothing held changed but the expired dropped.
    pub fn register(
        &self,
        principal: PrincipalId,
        workspace: WorkspaceId,
        id: AssertionId,
        now: UtcNanos,
    ) -> Result<Registration, Withheld> {
        let mut store = self.store();
        store.sweep(now);
        if let Some(held) = store.held.get(&id.0) {
            return match held {
                Held::Registration(registration)
                    if registration.principal == principal
                        && registration.workspace == workspace =>
                {
                    Ok(registration.clone())
                }
                Held::Registration(_) | Held::StepUp(_) => Err(Withheld::Conflict),
            };
        }
        if store.buried(principal, &id.0) {
            return Err(Withheld::Conflict);
        }
        if store.full(principal, self.per_principal, self.in_all) {
            return Err(Withheld::Full);
        }
        let expires_at = later(now, FRESH_S).ok_or(Withheld::OutOfRange)?;
        let mut challenge = [0_u8; 32];
        self.random
            .fill(&mut challenge)
            .map_err(|_| Withheld::NoRandom)?;
        store.issued = store.issued.saturating_add(1);
        let registration = Registration {
            id: id.0.clone(),
            principal,
            workspace,
            challenge,
            issued_at: now,
            expires_at,
            order: store.issued,
        };
        store
            .held
            .insert(id.0, Held::Registration(registration.clone()));
        Ok(registration)
    }

    /// The challenge held under `id`, expired or not: an expired challenge is still found here, so
    /// a step-up presenting it is stale rather than missing ([DEC-2105] item 3).
    ///
    /// [DEC-2105]: ../../../docs/project/decisions/DEC-2105.md
    pub fn held(&self, id: &str) -> Option<ChallengeRecord> {
        match self.store().held.get(id) {
            Some(Held::StepUp(record)) => Some(record.clone()),
            Some(Held::Registration(_)) | None => None,
        }
    }

    /// The registration challenge held under `id` for `principal` in `workspace`, unexpired at
    /// `now`, taken out of the store and kept as a tombstone, so it serves one enrolment whatever
    /// follows and its key never revives it (DEC-2108 item 5, DEC-2118 item 1); `None` for an id
    /// held for anything else, which stays held, or for none.
    pub fn take_registration(
        &self,
        id: &str,
        principal: PrincipalId,
        workspace: WorkspaceId,
        now: UtcNanos,
    ) -> Option<Registration> {
        let mut store = self.store();
        store.sweep(now);
        let bound = matches!(
            store.held.get(id),
            Some(Held::Registration(registration))
                if registration.principal == principal && registration.workspace == workspace
        );
        if !bound {
            return None;
        }
        let Some(Held::Registration(registration)) = store.held.remove(id) else {
            return None;
        };
        store.bury(principal, registration.id.clone(), now);
        Some(registration)
    }

    /// Drops the challenge held under `id` once a grant verifies with it (DEC-2104 item 6), and
    /// whether it was still held: a second grant racing the first finds it gone.
    pub fn take(&self, id: &str) -> bool {
        let mut store = self.store();
        match store.held.get(id) {
            Some(Held::StepUp(_)) => store.held.remove(id).is_some(),
            Some(Held::Registration(_)) | None => false,
        }
    }
}

/// `POST /v1/workspaces/{ws}/step-up/challenges` (DEC-2103 items 1 to 4).
pub async fn challenge(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    body: Body,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let (gate, body) = match strict(&app, &headers, body).await {
        Ok(admitted) => admitted,
        Err(refusal) => return refusal.response(),
    };
    let request = match decode::<StepUpChallengeRequest>(&body) {
        Ok(request) => request,
        Err(mandate_api::wire::Refused::Invalid { violations }) => return invalid(violations),
    };
    let Some(action) = action_of(&request.action) else {
        return Refused::Invalid.response();
    };
    let reader = match authorized(
        &app,
        &gate.admitted,
        gate.now,
        &workspace,
        permission_for(action.kind),
        Unreadable::Command,
    )
    .await
    {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    if !paper(&reader.control) {
        return problem(
            StatusCode::CONFLICT,
            problem_body(ProblemCode::LiveUnavailable),
        );
    }
    let Some((_, id)) = derived_for(&gate, &reader.workspace, OPERATION) else {
        return Refused::JournalUnavailable.response();
    };
    let issued = app
        .challenges
        .issue(&reader.tenant, action, AssertionId(id), gate.now);
    match issued {
        Ok(record) => answered(&record, &request.action, gate.now),
        Err(Withheld::Refused) => Refused::Forbidden.response(),
        Err(Withheld::Conflict) => problem(
            StatusCode::CONFLICT,
            problem_body(ProblemCode::IdempotencyConflict),
        ),
        Err(Withheld::Full) => problem(
            StatusCode::TOO_MANY_REQUESTS,
            problem_body(ProblemCode::RateLimited),
        ),
        Err(Withheld::OutOfRange | Withheld::NoRandom) => Refused::JournalUnavailable.response(),
    }
}

/// The request's action as `mandate-passkey` binds it: the kind of the same wire name and the
/// digest the reference names.
fn action_of(action: &StepUpAction) -> Option<stepup::Action> {
    let name = json_string(&encode(&action.kind).ok()?)?;
    let kind = StepUpActionKind::ALL
        .into_iter()
        .find(|kind| kind.code() == name)?;
    let reference = ref_text(&action.digest)?;
    let digest = Digest::from_hex(reference.strip_prefix("sha256:")?)?;
    Some(stepup::Action { kind, digest })
}

/// The `201` (DEC-2103 item 3): the record's id, the challenge the authenticator signs, the
/// action as requested, and the record's instants; it reads no stream, so `as_of` is empty.
fn answered(record: &ChallengeRecord, action: &StepUpAction, now: UtcNanos) -> Response {
    let (Ok(challenge), Ok(action)) = (record.webauthn_challenge(), encode(action)) else {
        return Refused::JournalUnavailable.response();
    };
    let Ok(action) = mandate_canon::parse(&action) else {
        return Refused::JournalUnavailable.response();
    };
    let members = vec![
        ("challenge_id", string(&record.challenge_id().0)),
        ("webauthn_challenge", string(&challenge.base64url())),
        ("action", action),
        ("issued_at", string(&record.issued_at().to_string())),
        ("expires_at", string(&record.expires_at().to_string())),
        ("as_of", Value::Array(Vec::new())),
    ];
    let mut response = enveloped(StatusCode::CREATED, now, members);
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
