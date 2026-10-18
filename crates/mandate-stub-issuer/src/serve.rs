//! What the stub answers (DEC-2100 item 7): `GET /jwks.json` and `POST /token`, only for its own
//! `Host`, with no CORS header on any answer.

use std::future::{Future, IntoFuture as _};
use std::pin::pin;
use std::sync::Arc;
use std::task::Poll;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::header::{CONTENT_TYPE, HOST};
use axum::http::{Method, StatusCode, Version};
use axum::response::{IntoResponse as _, Response};
use tokio::net::TcpListener;

use crate::issuer::{IssueError, Issuer, TOKEN_TTL_S};

/// The largest `POST /token` body accepted, in bytes.
pub const MAX_BODY_BYTES: usize = 4096;

/// The one media type `POST /token` takes.
const JSON: &str = "application/json";

/// Why serving stopped other than by `stop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeError {
    /// The listener failed.
    Io,
}

/// Serves `issuer` on `listener` until `stop` completes. Stopping drops the server at once: a
/// request in flight may get no answer, and its caller asks again (DEC-2100 item 10).
///
/// # Errors
/// [`ServeError::Io`].
pub async fn serve(
    listener: TcpListener,
    issuer: Arc<Issuer>,
    stop: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServeError> {
    let router = Router::new().fallback(answer).with_state(issuer);
    let mut server = pin!(axum::serve(listener, router).into_future());
    let mut stop = pin!(stop);
    std::future::poll_fn(|cx| {
        if stop.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Ok(()));
        }
        server
            .as_mut()
            .poll(cx)
            .map(|served| served.map_err(|_| ServeError::Io))
    })
    .await
}

/// Every request: 421 for any `Host` but the bind exactly, before anything else is read; then
/// HTTP/1.1 only; then the two routes, and 404 for the rest.
async fn answer(State(issuer): State<Arc<Issuer>>, request: Request) -> Response {
    let mut hosts = request.headers().get_all(HOST).iter();
    let ours = matches!(
        (hosts.next(), hosts.next()),
        (Some(host), None) if host.as_bytes() == issuer.host.as_bytes()
    );
    if !ours {
        return StatusCode::MISDIRECTED_REQUEST.into_response();
    }
    if request.version() != Version::HTTP_11 {
        return StatusCode::HTTP_VERSION_NOT_SUPPORTED.into_response();
    }
    match (request.method(), request.uri().path()) {
        (&Method::GET, "/jwks.json") => {
            ([(CONTENT_TYPE, JSON)], issuer.jwks.clone()).into_response()
        }
        (&Method::POST, "/token") => mint(&issuer, request).await,
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `POST /token`: 415 unless JSON, 413 over [`MAX_BODY_BYTES`], 400 for a bad body or subject.
async fn mint(issuer: &Issuer, request: Request) -> Response {
    let json = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|media| media.to_str().ok())
        .is_some_and(is_json);
    if !json {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    let Ok(body) = to_bytes(request.into_body(), MAX_BODY_BYTES).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let subject = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| value.get("subject")?.as_str().map(str::to_owned));
    let Some(subject) = subject else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(now) = now_s() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    match issuer.token(&subject, now) {
        Ok(token) => axum::Json(serde_json::json!({
            "access_token": token,
            "expires_in": TOKEN_TTL_S,
        }))
        .into_response(),
        Err(IssueError::Subject) => StatusCode::BAD_REQUEST.into_response(),
        Err(IssueError::Key) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// `application/json`, in any ASCII case, with or without parameters such as `charset`.
fn is_json(media: &str) -> bool {
    media
        .split(';')
        .next()
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case(JSON))
}

/// Unix seconds now, the clock the server verifies the token's times against.
#[expect(
    clippy::disallowed_methods,
    reason = "a token issuer signs at the wall clock; the stub is a dev and CI tool, not core code"
)]
fn now_s() -> Option<i64> {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_secs()).ok()
}
