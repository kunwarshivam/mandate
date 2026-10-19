//! The connect start (§4.5, [DEC-2623](../../../docs/project/decisions/DEC-2623.md)):
//! `POST /v1/workspaces/{ws}/connections/oauth/start` commits the pending connection's
//! `ConnectionRequested` version 2 with the step-up it spent, then issues the single-use `state`
//! the callback redeems. It reaches no broker (§1.4): the browser goes to the authorization page.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use mandate_connections::start::{OAuthClient, PendingStates};

use crate::http::App;
use crate::session::Random;

/// The token-breadth disclosure's document (DEC-2621 item 5).
pub const DISCLOSURE_DOCUMENT: &str = "alpaca_oauth_token_breadth";

/// The disclosure's current version; a start naming another is `invalid` at `/disclosure`
/// (DEC-2623 item 6).
pub const DISCLOSURE_VERSION: &str = "1";

/// The §3.4 operation the start's `ConnectionRequested` event id is derived under.
pub const OPERATION: &str = "connect";

/// The operation the pending `connection_id` is derived under: `conn_` and that ULID (DEC-2623
/// item 5).
pub const CONNECTION_OPERATION: &str = "connect_connection";

/// The operation the pending connection's `account_ref` ULID is derived under.
pub const ACCOUNT_OPERATION: &str = "connect_account";

/// How many bytes the `state` and the PKCE verifier each draw (DEC-2623 item 7).
pub const RANDOM_BYTES: usize = 32;

/// What a deployment that offers a connect holds: its registered OAuth client, the `state`
/// values issued and not yet redeemed, and the random source they are drawn from (DEC-2623 items
/// 2, 4 and 7).
pub struct Connect {
    pub client: OAuthClient,
    pub states: Mutex<PendingStates>,
    pub random: Box<dyn Random>,
}

impl Connect {
    pub fn new(client: OAuthClient, random: Box<dyn Random>) -> Self {
        Self {
            client,
            states: Mutex::new(PendingStates::default()),
            random,
        }
    }
}

/// `POST /v1/workspaces/{ws}/connections/oauth/start` (DEC-2623 items 1 and 5 to 9).
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub async fn start(
    State(_app): State<Arc<App>>,
    _headers: HeaderMap,
    _path: Result<Path<String>, PathRejection>,
    _body: Body,
) -> Response {
    todo!()
}
