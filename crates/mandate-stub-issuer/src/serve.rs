//! What the stub answers (DEC-2100 item 7): `GET /jwks.json` and `POST /token`, only for its own
//! `Host`, with no CORS header on any answer.
//!
//! One handler answers every request, so the order is explicit: an HTTP version other than 1.1,
//! then any `Host` but exactly the bind address (one header, byte for byte), then the route, and
//! for `POST /token` the media type, the body's size, and the subject. A refusal has no body.

use std::future::Future;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::header::{CONTENT_TYPE, HOST};
use axum::http::{Method, StatusCode, Version};
use axum::response::{IntoResponse, Response};
use serde_json::json;
use tokio::net::TcpListener;

use crate::issuer::{IssueError, Issuer, TOKEN_TTL_S};

/// The largest `POST /token` body accepted, in bytes.
pub const MAX_BODY_BYTES: usize = 4096;

/// Why serving stopped other than by `stop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeError {
    /// The listener failed.
    Io,
}

/// Serves `issuer` on `listener` until `stop` completes.
///
/// # Errors
/// [`ServeError::Io`].
pub async fn serve(
    listener: TcpListener,
    issuer: Arc<Issuer>,
    stop: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServeError> {
    let router = Router::new().fallback(answer).with_state(issuer);
    axum::serve(listener, router)
        .with_graceful_shutdown(stop)
        .await
        .map_err(|_| ServeError::Io)
}

fn json_answer(body: String) -> Response {
    (StatusCode::OK, [(CONTENT_TYPE, "application/json")], body).into_response()
}

async fn answer(State(issuer): State<Arc<Issuer>>, request: Request) -> Response {
    if request.version() != Version::HTTP_11 {
        return StatusCode::HTTP_VERSION_NOT_SUPPORTED.into_response();
    }
    let own = issuer.issuer.strip_prefix("http://").unwrap_or_default();
    let mut hosts = request.headers().get_all(HOST).iter();
    let one_host = match (hosts.next(), hosts.next()) {
        (Some(host), None) => host.as_bytes() == own.as_bytes(),
        _ => false,
    };
    if !one_host {
        return StatusCode::MISDIRECTED_REQUEST.into_response();
    }
    match (request.method(), request.uri().path()) {
        (&Method::GET, "/jwks.json") => json_answer(issuer.jwks.clone()),
        (&Method::POST, "/token") => {
            let json = json_media(&request);
            token(&issuer, request.into_body(), json).await
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Whether `request` says its body is `application/json`, with any parameters, in any case.
fn json_media(request: &Request) -> bool {
    request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"))
}

#[allow(
    clippy::disallowed_methods,
    reason = "the stub signs at the wall clock, as a real issuer does; it is a dev tool, outside the core (ES-05)"
)]
fn now_s() -> Option<i64> {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    i64::try_from(since.as_secs()).ok()
}

async fn token(issuer: &Issuer, body: Body, json: bool) -> Response {
    if !json {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    let Ok(bytes) = to_bytes(body, MAX_BODY_BYTES).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let subject = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|value| value.get("subject")?.as_str().map(str::to_owned));
    let (Some(subject), Some(now)) = (subject, now_s()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match issuer.token(&subject, now) {
        Ok(token) => {
            json_answer(json!({"access_token": token, "expires_in": TOKEN_TTL_S}).to_string())
        }
        Err(IssueError::Subject) => StatusCode::BAD_REQUEST.into_response(),
        Err(IssueError::Key) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
