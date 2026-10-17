//! The two routers (§3.11, §3.3 item 5, API-1, DEC-978 items 3 and 4).
//!
//! The API router answers the one 401 for every path whose route has not landed, and adds CORS
//! headers only for the configured app origin. The probes' router answers `/livez` and `/readyz`
//! and a bodiless 404 for anything else.

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use mandate_api::problem::{Effect, Problem, ProblemCode};
use mandate_api::wire::encode;

use crate::clock::Clock;
use crate::lifecycle::{Drain, Lifecycle};
use crate::session::Sessions;
use crate::token::TokenCheck;

/// The methods a preflight from the app origin allows (§3.3 item 5).
pub const ALLOW_METHODS: &str = "GET, POST, PUT, DELETE, PATCH";

/// The request headers a preflight from the app origin allows (§3.3 item 5).
pub const ALLOW_HEADERS: &str = "Content-Type, Idempotency-Key, If-Match, X-Mandate-Request";

/// The response headers the app origin may read (§3.3 item 5).
pub const EXPOSE_HEADERS: &str = "ETag, Location, Retry-After";

/// How long a browser may cache a preflight, in seconds (§3.3 item 5).
pub const MAX_AGE_S: u32 = 600;

/// The media type of every problem document (RFC 9457).
pub const PROBLEM_JSON: &str = "application/problem+json";

/// What the API router needs from the deployment's configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The one origin CORS allows, exactly as the browser sends it (`https://app.owlhead.ai`).
    pub app_origin: String,
}

/// The custom header a cross-site form cannot set (§3.3 item 1).
pub const CSRF_HEADER: &str = "x-mandate-request";

/// Everything the API router serves from.
#[derive(Clone)]
pub struct App {
    pub settings: Settings,
    pub sessions: Arc<Sessions>,
    pub tokens: Arc<dyn TokenCheck>,
    /// The subject-link file (§3.11 item 7).
    pub links: PathBuf,
    pub clock: Arc<dyn Clock>,
}

/// The API listener's router: every path answers the one 401 until its route lands (API-1).
/// Each request is counted in flight on `drain` until its response is ready.
pub fn api(app: App, drain: Arc<Drain>) -> Router {
    let settings = app.settings.clone();
    let unauthenticated = problem_body(ProblemCode::Unauthenticated);
    let routes = Router::new()
        .fallback(move || {
            let body = unauthenticated.clone();
            async move { problem(StatusCode::UNAUTHORIZED, body) }
        })
        .layer(middleware::from_fn_with_state(Arc::new(settings), cors));
    counted(routes, drain)
}

/// `router`, with each request counted in flight on `drain` until its response is ready, so a
/// drain waits for it (§3.11 item 4).
pub fn counted(router: Router, drain: Arc<Drain>) -> Router {
    router.layer(middleware::from_fn_with_state(drain, count))
}

async fn count(State(drain): State<Arc<Drain>>, request: Request, next: Next) -> Response {
    let _in_flight = drain.enter();
    next.run(request).await
}

/// The probes' listener's router: `GET /livez`, `GET /readyz`, and a bodiless 404 for anything
/// else, `HEAD` included.
pub fn probes(lifecycle: Arc<dyn Lifecycle>) -> Router {
    Router::new().fallback(probe).with_state(lifecycle)
}

async fn probe(State(lifecycle): State<Arc<dyn Lifecycle>>, method: Method, uri: Uri) -> Response {
    match (method == Method::GET, uri.path()) {
        (true, "/livez") => status(StatusCode::OK, "live"),
        (true, "/readyz") if lifecycle.ready() => status(StatusCode::OK, "ready"),
        (true, "/readyz") => status(StatusCode::SERVICE_UNAVAILABLE, "not_ready"),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `{"status": <word>}`: a fixed word, never data.
fn status(code: StatusCode, word: &'static str) -> Response {
    let body = format!(r#"{{"status":"{word}"}}"#);
    with_type(code, "application/json", Body::from(body))
}

fn with_type(code: StatusCode, media: &'static str, body: Body) -> Response {
    let mut response = Response::new(body);
    *response.status_mut() = code;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(media));
    response
}

/// The problem document for `code` with nothing recorded, as bytes. A code `mandate-api` cannot
/// encode leaves the body empty; the status still says what happened.
fn problem_body(code: ProblemCode) -> Bytes {
    Problem::of(code, Effect::None, None, None)
        .ok()
        .and_then(|problem| encode(&problem).ok())
        .map(Bytes::from)
        .unwrap_or_default()
}

fn problem(code: StatusCode, body: Bytes) -> Response {
    with_type(code, PROBLEM_JSON, Body::from(body))
}

/// §3.3 item 5. A preflight (`OPTIONS` with `Origin` and `Access-Control-Request-Method`) is
/// answered here, reads nothing, and carries CORS headers only for the app origin; every other
/// response carries the app origin's headers when it asked. Every response varies on `Origin`.
async fn cors(State(settings): State<Arc<Settings>>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let allowed = is_app_origin(headers, &settings.app_origin);
    let preflight = request.method() == Method::OPTIONS
        && headers.contains_key(header::ORIGIN)
        && headers.contains_key(header::ACCESS_CONTROL_REQUEST_METHOD);
    let mut response = if preflight {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::NO_CONTENT;
        if allowed {
            let out = response.headers_mut();
            out.insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static(ALLOW_METHODS),
            );
            out.insert(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static(ALLOW_HEADERS),
            );
            out.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from(MAX_AGE_S));
        }
        response
    } else {
        let mut response = next.run(request).await;
        if allowed {
            response.headers_mut().insert(
                header::ACCESS_CONTROL_EXPOSE_HEADERS,
                HeaderValue::from_static(EXPOSE_HEADERS),
            );
        }
        response
    };
    let out = response.headers_mut();
    if allowed && let Ok(origin) = HeaderValue::from_str(&settings.app_origin) {
        out.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        out.insert(
            header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
            HeaderValue::from_static("true"),
        );
    }
    out.insert(header::VARY, HeaderValue::from_static("Origin"));
    response
}

/// Whether the request's one `Origin` is exactly the app origin, byte for byte.
fn is_app_origin(headers: &HeaderMap, app_origin: &str) -> bool {
    let mut origins = headers.get_all(header::ORIGIN).iter();
    match (origins.next(), origins.next()) {
        (Some(origin), None) => origin.as_bytes() == app_origin.as_bytes(),
        _ => false,
    }
}
