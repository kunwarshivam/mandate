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
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use mandate_identity::{AssertionId, TenantContext};
use mandate_passkey::stepup::{self, ChallengeRecord};
use mandate_time::UtcNanos;

use crate::http::App;

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
#[expect(
    dead_code,
    reason = "the store's fields are read by `issue`, which the implementation fills in (DEC-77)"
)]
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
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn issue(
        &self,
        _context: &TenantContext,
        _action: stepup::Action,
        _id: AssertionId,
        _now: UtcNanos,
    ) -> Result<ChallengeRecord, Withheld> {
        todo!()
    }
}

/// `POST /v1/workspaces/{ws}/step-up/challenges` (DEC-2103 items 1 to 4).
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub async fn challenge(
    State(_app): State<Arc<App>>,
    _headers: HeaderMap,
    _path: Result<Path<String>, PathRejection>,
    _body: Body,
) -> Response {
    todo!()
}
