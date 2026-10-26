//! The step-up challenge route (workspace API spec §3.6, identity spec §7.2 steps 1 and 2,
//! [DEC-2103](../../../docs/project/decisions/DEC-2103.md)): `POST
//! /v1/workspaces/{ws}/step-up/challenges`, and the store the challenges it issues are held in.
//!
//! A challenge is issued only through `mandate_passkey::stepup::issue_challenge`, under the
//! `TenantContext` `authorize` returned for the §4.2 row the action's kind needs (DEC-665), so a
//! principal that may not perform an action cannot obtain a challenge for it. Its id is §3.4's
//! derived id for the request's key, so a retry answers the challenge already issued. The store
//! is the server's memory: a challenge it does not hold can only make a step-up count as missing.

use std::collections::BTreeMap;
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
use mandate_identity::{AssertionId, StepUpActionKind, TenantContext};
use mandate_passkey::stepup::{
    self, ChallengeRecord, IssueRefusal, issue_challenge, permission_for,
};
use mandate_time::UtcNanos;

use crate::backtests::paper;
use crate::http::{App, invalid, problem, problem_body};
use crate::routes::{Refused, Unreadable, authorized, enveloped, path_refused, string};
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
}

/// The challenges the server has issued and not yet seen expire, by challenge id, with the most
/// one principal and all of them may hold.
#[derive(Debug)]
pub struct Challenges {
    held: Mutex<BTreeMap<String, ChallengeRecord>>,
    per_principal: usize,
    in_all: usize,
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
            held: Mutex::new(BTreeMap::new()),
            per_principal,
            in_all,
        }
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
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.retain(|_, record| now < record.expires_at());
        if let Some(record) = held.get(&id.0) {
            let same = record.principal_id() == &context.principal()
                && record.workspace_id() == &context.workspace()
                && record.action() == action;
            return if same {
                Ok(record.clone())
            } else {
                Err(Withheld::Conflict)
            };
        }
        let principal = context.principal();
        let theirs = held
            .values()
            .filter(|record| record.principal_id() == &principal)
            .count();
        if theirs >= self.per_principal || held.len() >= self.in_all {
            return Err(Withheld::Full);
        }
        let issued =
            issue_challenge(context, action, id, now).map_err(|refusal| match refusal {
                IssueRefusal::Refused(_) => Withheld::Refused,
                IssueRefusal::Issue(_) => Withheld::OutOfRange,
            })?;
        let record = issued.record().clone();
        held.insert(record.challenge_id().0.clone(), record.clone());
        Ok(record)
    }

    /// The challenge held under `id`, expired or not: an expired challenge is still found here, so
    /// a step-up presenting it is stale rather than missing ([DEC-2105] item 3).
    ///
    /// [DEC-2105]: ../../../docs/project/decisions/DEC-2105.md
    pub fn held(&self, id: &str) -> Option<ChallengeRecord> {
        let held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.get(id).cloned()
    }

    /// Drops the challenge held under `id` once a grant verifies with it (DEC-2104 item 6), and
    /// whether it was still held: a second grant racing the first finds it gone.
    pub fn take(&self, id: &str) -> bool {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.remove(id).is_some()
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
        Err(Withheld::OutOfRange) => Refused::JournalUnavailable.response(),
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
