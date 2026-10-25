//! What the stub answers (DEC-2100 item 7): `GET /jwks.json` and `POST /token`, and, when started
//! with an Alpaca client, the fixture provider's two routes (DEC-2625 item 6), only for its own
//! `Host`, with no CORS header on any answer.

use std::future::{Future, IntoFuture as _};
use std::io::Write as _;
use std::pin::pin;
use std::sync::Arc;
use std::task::Poll;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, HOST, LOCATION};
use axum::http::{Method, StatusCode, Version};
use axum::response::{IntoResponse as _, Response};
use tokio::net::TcpListener;

use crate::alpaca::{AUTHORIZE_PATH, Alpaca, Cause, Refusal, TOKEN_PATH, refusal_line};
use crate::issuer::{IssueError, Issuer, TOKEN_TTL_S};

/// The largest `POST /token` body accepted, in bytes.
pub const MAX_BODY_BYTES: usize = 4096;

/// The one media type `POST /token` takes.
const JSON: &str = "application/json";

/// The one media type `POST /alpaca/oauth/token` takes.
const FORM: &str = "application/x-www-form-urlencoded";

/// What the router serves from: the one `Host` it answers, the issuer unless the provider runs
/// alone, and the Alpaca provider when the stub was started with its client.
struct Served {
    host: String,
    issuer: Option<Arc<Issuer>>,
    alpaca: Option<Arc<Alpaca>>,
}

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
    serve_with(listener, issuer, None, stop).await
}

/// Serves `issuer` and, when given, `alpaca` on `listener` until `stop` completes.
///
/// # Errors
/// [`ServeError::Io`].
pub async fn serve_with(
    listener: TcpListener,
    issuer: Arc<Issuer>,
    alpaca: Option<Arc<Alpaca>>,
    stop: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServeError> {
    let served = Served {
        host: issuer.host.clone(),
        issuer: Some(issuer),
        alpaca,
    };
    run(listener, served, stop).await
}

/// Serves the fixture provider `alpaca` alone on `listener` until `stop` completes, answering only
/// requests whose one `Host` is `host` (DEC-1176 item 8): `/alpaca/oauth/*`, and 404 for every
/// other path, the issuer's included, since none runs.
///
/// # Errors
/// [`ServeError::Io`].
pub async fn serve_fixture(
    listener: TcpListener,
    host: String,
    alpaca: Arc<Alpaca>,
    stop: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServeError> {
    let served = Served {
        host,
        issuer: None,
        alpaca: Some(alpaca),
    };
    run(listener, served, stop).await
}

/// Serves `served` on `listener` until `stop` completes.
async fn run(
    listener: TcpListener,
    served: Served,
    stop: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServeError> {
    let router = Router::new().fallback(answer).with_state(Arc::new(served));
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
/// HTTP/1.1 only; then the routes, and 404 for the rest, the provider's included when it is not
/// served.
async fn answer(State(served): State<Arc<Served>>, request: Request) -> Response {
    let mut hosts = request.headers().get_all(HOST).iter();
    let ours = matches!(
        (hosts.next(), hosts.next()),
        (Some(host), None) if host.as_bytes() == served.host.as_bytes()
    );
    if !ours {
        return StatusCode::MISDIRECTED_REQUEST.into_response();
    }
    if request.version() != Version::HTTP_11 {
        return StatusCode::HTTP_VERSION_NOT_SUPPORTED.into_response();
    }
    match (
        request.method(),
        request.uri().path(),
        served.issuer.as_deref(),
    ) {
        (&Method::GET, "/jwks.json", Some(issuer)) => {
            ([(CONTENT_TYPE, JSON)], issuer.jwks.clone()).into_response()
        }
        (&Method::POST, "/token", Some(issuer)) => mint(issuer, request).await,
        (&Method::GET, AUTHORIZE_PATH, _) if served.alpaca.is_some() => {
            authorize(served.alpaca.as_deref(), &request)
        }
        (&Method::POST, TOKEN_PATH, _) if served.alpaca.is_some() => {
            exchange(served.alpaca.as_deref(), request).await
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `GET /alpaca/oauth/authorize`: 302 to the redirect URI, or the refusal with no `Location`.
fn authorize(alpaca: Option<&Alpaca>, request: &Request) -> Response {
    let (Some(alpaca), Some(now)) = (alpaca, now_s()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    match alpaca.authorize(request.uri().query().unwrap_or_default(), now) {
        Ok(location) => (StatusCode::FOUND, [(LOCATION, location)]).into_response(),
        Err(refusal) => refused(refusal),
    }
}

/// `POST /alpaca/oauth/token`: 415 unless a form, 413 over [`MAX_BODY_BYTES`], then the token or
/// the refusal, never stored by a cache.
async fn exchange(alpaca: Option<&Alpaca>, request: Request) -> Response {
    let form = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|media| media.to_str().ok())
        .is_some_and(|media| essence_is(media, FORM));
    if !form {
        write_line(MEDIA_TYPE_LINE);
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    let Ok(body) = to_bytes(request.into_body(), MAX_BODY_BYTES).await else {
        write_line(BODY_SIZE_LINE);
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let (Some(alpaca), Some(now)) = (alpaca, now_s()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(body) = std::str::from_utf8(&body) else {
        return token_refused(Refusal::InvalidRequest, Cause::Form);
    };
    match alpaca.exchange(body, now) {
        Ok(json) => ([(CONTENT_TYPE, JSON), (CACHE_CONTROL, "no-store")], json).into_response(),
        Err((refusal, cause)) => token_refused(refusal, cause),
    }
}

/// The line for a token request that is not a form (DEC-2127 item 2).
const MEDIA_TYPE_LINE: &str = "token_refused unsupported_media_type\n";

/// The line for a token request over [`MAX_BODY_BYTES`] (DEC-2127 item 2).
const BODY_SIZE_LINE: &str = "token_refused payload_too_large\n";

/// A token refusal, its line written to standard error first (DEC-2127).
fn token_refused(refusal: Refusal, cause: Cause) -> Response {
    write_line(&refusal_line(refusal, cause));
    refused(refusal)
}

/// Writes `line` to standard error; a closed standard error loses the line, never the answer.
fn write_line(line: &str) {
    let _ = std::io::stderr().lock().write_all(line.as_bytes());
}

/// A refusal as RFC 6749 §5.2 writes it, never stored by a cache.
fn refused(refusal: Refusal) -> Response {
    let status =
        StatusCode::from_u16(refusal.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = serde_json::json!({ "error": refusal.code() }).to_string();
    (
        status,
        [(CONTENT_TYPE, JSON), (CACHE_CONTROL, "no-store")],
        body,
    )
        .into_response()
}

/// `POST /token`: 415 unless JSON, 413 over [`MAX_BODY_BYTES`], 400 for a bad body or subject.
async fn mint(issuer: &Issuer, request: Request) -> Response {
    let json = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|media| media.to_str().ok())
        .is_some_and(|media| essence_is(media, JSON));
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

/// Whether `media` is `essence`, in any ASCII case, with or without parameters such as `charset`.
fn essence_is(media: &str, essence: &str) -> bool {
    media
        .split(';')
        .next()
        .is_some_and(|found| found.trim().eq_ignore_ascii_case(essence))
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
