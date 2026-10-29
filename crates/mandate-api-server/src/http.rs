//! The two routers (§3.11, §3.3 item 5, API-1, DEC-978 items 3 and 4).
//!
//! The API router answers the one 401 for every path whose route has not landed, and adds CORS
//! headers only for the configured app origin. The probes' router answers `/livez` and `/readyz`
//! and a bodiless 404 for anything else.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use mandate_api::problem::{Effect, Problem, ProblemCode, Violation};
use mandate_api::wire::encode;
use mandate_canon::Value;
use mandate_journal::ArtifactSource;

use crate::catalogue;
use crate::challenges::{self, Challenges};
use crate::clock::Clock;
use crate::folds::Folds;
use crate::journal::Journal;
use crate::journal_reads;
use crate::lifecycle::{Drain, Lifecycle};
use crate::me;
use crate::members::{Hosted, ParseMembers};
use crate::objects::ObjectStore;
use crate::passkeys;
use crate::reduction::{self, Reduction};
use crate::routes;
use crate::session::{Answer, COOKIE_MAX_AGE_S, COOKIE_NAME, Sessions};
use crate::status;
use crate::stepup::StepUp;
use crate::subject::{self, LinkRefusal, SubjectLinks};
use crate::token::{Checked, TokenCheck};
use crate::version_routes;
use crate::writes;

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
    /// The subject links (§3.11 item 7): [`subject::Fixed`] in the deployment.
    pub links: Arc<dyn SubjectLinks>,
    pub clock: Arc<dyn Clock>,
    pub journal: Arc<dyn Journal>,
    /// How the control stream's rows become membership records.
    pub members: Arc<ParseMembers>,
    /// The one workspace this deployment hosts.
    pub hosted: Hosted,
    /// The content-addressed store the agent reads take mandate documents from, read only
    /// (DEC-1020 item 1); `None` when the deployment names none.
    pub artifacts: Option<Arc<dyn ArtifactSource + Send + Sync>>,
    /// The same store, which the version routes add objects to (DEC-2101 item 8); `None` when
    /// the deployment names none, and those routes then answer `journal_unavailable`.
    pub objects: Option<Arc<dyn ObjectStore>>,
    /// The step-up port (DEC-2101 item 2): the deployment binds [`crate::methods::Methods`]
    /// (DEC-2126: passkey and DEC-2104's `cli_confirm`), and tests bind [`crate::stepup::Unbound`] or
    /// their own.
    pub step_up: Arc<dyn StepUp>,
    /// The step-up challenges issued and not yet expired (DEC-2103 item 5).
    pub challenges: Arc<Challenges>,
    /// The connect start's OAuth client and `state` values (DEC-2623); `None` when the deployment
    /// offers no connect, and the start answers `not_found`.
    pub connect: Option<Arc<crate::connect::Connect>>,
    /// Route 2's challenges, limits, and ingress peers (DEC-2117, DEC-2119).
    pub reduction: Arc<Reduction>,
    /// The account reads' folds, resumed past the rows they folded (DEC-2145 item 10).
    pub folds: Arc<Folds>,
}

/// The API listener's router: every path answers the one 401 until its route lands (API-1).
/// Each request is counted in flight on `drain` until its response is ready.
pub fn api(app: App, drain: Arc<Drain>) -> Router {
    let settings = app.settings.clone();
    let unauthenticated = problem_body(ProblemCode::Unauthenticated);
    let refused = unauthenticated.clone();
    let otherwise = move || {
        let body = refused.clone();
        async move { problem(StatusCode::UNAUTHORIZED, body) }
    };
    let routes = Router::new()
        .route(
            "/v1/sessions",
            post(open_session).fallback(otherwise.clone()),
        )
        .route(
            "/v1/sessions/refresh",
            post(refresh_session).fallback(otherwise.clone()),
        )
        .route(
            "/v1/sessions/current",
            delete(end_session).fallback(otherwise.clone()),
        )
        .route(
            "/v1/oauth/alpaca/callback",
            get(crate::connect::callback).fallback(otherwise.clone()),
        )
        .route(
            "/v1/reduction-sessions/challenges",
            post(reduction::challenge).fallback(otherwise.clone()),
        )
        .route(
            "/v1/reduction-sessions",
            post(reduction::open).fallback(otherwise.clone()),
        )
        .route(
            "/v1/me/session",
            get(me::session).fallback(otherwise.clone()),
        )
        .route(
            "/v1/me/workspaces",
            get(me::workspaces).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/agents",
            get(routes::list_agents)
                .post(version_routes::deploy)
                .fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/agents/{agent}",
            get(routes::one_agent).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/agents/{agent}/orders",
            get(routes::agent_orders_read).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/agents/{agent}/positions",
            get(routes::agent_positions_read).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/alerts",
            get(routes::list_alerts).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/approvals",
            get(routes::list_approvals).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/approvals/{approval}",
            get(routes::one_approval).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/journal/events/{event}",
            get(journal_reads::one_event).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/journal/streams/{stream}/events",
            get(journal_reads::events).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/backtests/{backtest}",
            get(routes::one_backtest).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/evaluations/{evaluation}",
            get(routes::one_evaluation).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/agents/{agent}/pause",
            post(writes::pause).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/approvals/{approval}/responses",
            post(writes::respond).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/kill-switch",
            post(writes::kill_switch).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/mandate-versions",
            post(version_routes::create_version).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/instruments",
            get(catalogue::instruments).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/signal-models",
            get(catalogue::signal_models).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/mandate-versions/{version}/confirmation",
            get(version_routes::confirmation).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/mandate-versions/{version}/confirm",
            post(version_routes::confirm).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/agents/{agent}/resume",
            post(writes::resume).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/agents/{agent}/stop",
            post(writes::stop).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/agents/{agent}/exits",
            post(writes::owner_exit).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/commands/{command}",
            get(status::status).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/step-up/challenges",
            post(challenges::challenge).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/passkeys/registration-challenges",
            post(passkeys::registration_challenge).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/passkeys",
            get(passkeys::list)
                .post(passkeys::enrol)
                .fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/connections",
            get(routes::list_connections).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/connections/{connection}",
            get(routes::one_connection).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/connections/oauth/start",
            post(crate::connect::start).fallback(otherwise.clone()),
        )
        .route(
            "/v1/workspaces/{workspace}/agents/{agent}/go-live",
            post(version_routes::go_live).fallback(otherwise.clone()),
        );
    #[cfg(debug_assertions)]
    let routes = routes.route(
        "/v1/workspaces/{workspace}/agents/{agent}/mandate-versions",
        get(crate::mandate_versions::agent_mandate_versions).fallback(otherwise.clone()),
    );
    let routes = routes
        .fallback(otherwise)
        .with_state(Arc::new(app))
        .layer(middleware::from_fn_with_state(Arc::new(settings), cors));
    counted(routes, drain)
}

/// The one 401 (API-1).
pub(crate) fn unauthenticated() -> Response {
    problem(
        StatusCode::UNAUTHORIZED,
        problem_body(ProblemCode::Unauthenticated),
    )
}

/// §3.3 item 1's check, which the browser-only session routes make first and refuse with 403: one
/// `Origin`, exactly the app origin, and one `X-Mandate-Request: 1`.
pub(crate) fn browser(headers: &HeaderMap, app_origin: &str) -> bool {
    let mut marks = headers.get_all(CSRF_HEADER).iter();
    let marked = matches!((marks.next(), marks.next()), (Some(mark), None) if mark == "1");
    marked && is_app_origin(headers, app_origin)
}

/// The 403 a failed [`browser`] check answers, before any cookie or token is read.
pub(crate) fn forbidden() -> Response {
    problem(StatusCode::FORBIDDEN, problem_body(ProblemCode::Forbidden))
}

/// The session cookie's value when the request carries it exactly once.
pub(crate) fn session_cookie(headers: &HeaderMap) -> Option<String> {
    let mut found = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter(|(name, _)| *name == COOKIE_NAME)
        .map(|(_, value)| value.to_owned());
    match (found.next(), found.next()) {
        (Some(value), None) => Some(value),
        _ => None,
    }
}

/// `{"access_token": ...}` and nothing else: `Some(Some(token))`, `Some(None)` for `null`, and
/// `None` for anything else.
fn access_token(body: &[u8]) -> Option<Option<String>> {
    let Ok(Value::Object(members)) = mandate_canon::parse(body) else {
        return None;
    };
    let mut members = members.into_iter();
    match (members.next(), members.next()) {
        (Some((key, value)), None) if key.as_str() == "access_token" => match value {
            Value::Str(token) => Some(Some(token)),
            Value::Null => Some(None),
            _ => None,
        },
        _ => None,
    }
}

fn set_cookie(value: &str, max_age_s: i64) -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    let cookie = format!(
        "{COOKIE_NAME}={value}; Path=/; Max-Age={max_age_s}; Secure; HttpOnly; SameSite=Strict"
    );
    if let Ok(cookie) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}

/// `POST /v1/sessions` (§3.11 item 5).
async fn open_session(State(app): State<Arc<App>>, headers: HeaderMap, body: Bytes) -> Response {
    if !browser(&headers, &app.settings.app_origin) {
        return forbidden();
    }
    let opened = (|| {
        let token = access_token(&body)??;
        let now = app.clock.now()?;
        let Checked::Subject { issuer, subject } = app.tokens.check(&token, now) else {
            return None;
        };
        let principal = app.links.principal(&issuer, &subject).ok()?;
        let replaced = session_cookie(&headers);
        app.sessions
            .open(
                principal,
                subject::digest(&issuer, &subject),
                replaced.as_deref(),
                now,
            )
            .ok()
    })();
    match opened {
        Some(cookie) => set_cookie(cookie.value(), COOKIE_MAX_AGE_S),
        None => unauthenticated(),
    }
}

/// `POST /v1/sessions/refresh` (§3.11 item 6). A session route 2 opened is refused
/// `reduction_only` and kept, whatever the provider answers (§4.10 route 2 item 5).
async fn refresh_session(State(app): State<Arc<App>>, headers: HeaderMap, body: Bytes) -> Response {
    if !browser(&headers, &app.settings.app_origin) {
        return forbidden();
    }
    let presented = session_cookie(&headers);
    if let (Some(cookie), Some(now)) = (presented.as_deref(), app.clock.now())
        && app.sessions.opened_by_passkey(cookie, now)
    {
        return routes::Refused::ReductionOnly.response();
    }
    let refreshed = (|| {
        let cookie = session_cookie(&headers)?;
        let token = access_token(&body)?;
        let now = app.clock.now()?;
        let answer = match token.map(|token| app.tokens.check(&token, now)) {
            None | Some(Checked::Unreachable) => Answer::Unreachable,
            Some(Checked::Refused) => Answer::Failed,
            Some(Checked::Subject { issuer, subject }) => {
                match app.links.principal(&issuer, &subject) {
                    Ok(principal) => Answer::Granted {
                        subject: subject::digest(&issuer, &subject),
                        principal,
                    },
                    Err(LinkRefusal::Unreadable | LinkRefusal::Exposed) => Answer::Unreachable,
                    Err(_) => Answer::Failed,
                }
            }
        };
        app.sessions.refresh(&cookie, answer, now).ok()
    })();
    match refreshed {
        Some(Some(cookie)) => set_cookie(cookie.value(), COOKIE_MAX_AGE_S),
        Some(None) => StatusCode::NO_CONTENT.into_response(),
        None => unauthenticated(),
    }
}

/// `DELETE /v1/sessions/current` (§3.11 item 6).
async fn end_session(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !browser(&headers, &app.settings.app_origin) {
        return forbidden();
    }
    let presented = session_cookie(&headers);
    match presented.as_deref().map(|cookie| app.sessions.end(cookie)) {
        Some(Ok(())) => set_cookie("", 0),
        Some(Err(_)) => {
            let mut refused = unauthenticated();
            if let Some(cleared) = set_cookie("", 0).headers().get(header::SET_COOKIE) {
                refused
                    .headers_mut()
                    .insert(header::SET_COOKIE, cleared.clone());
            }
            refused
        }
        None => unauthenticated(),
    }
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

pub(crate) fn with_type(code: StatusCode, media: &'static str, body: Body) -> Response {
    let mut response = Response::new(body);
    *response.status_mut() = code;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(media));
    response
}

/// The problem document for `code` with nothing recorded, as bytes. A code `mandate-api` cannot
/// encode leaves the body empty; the status still says what happened.
pub(crate) fn problem_body(code: ProblemCode) -> Bytes {
    Problem::of(code, Effect::None, None, None)
        .ok()
        .and_then(|problem| encode(&problem).ok())
        .map(Bytes::from)
        .unwrap_or_default()
}

/// The 422 `invalid` naming `violations`, with nothing recorded.
pub(crate) fn invalid(violations: Vec<Violation>) -> Response {
    let body = Problem::of(ProblemCode::Invalid, Effect::None, None, None)
        .ok()
        .map(|problem| Problem {
            violations,
            ..problem
        })
        .and_then(|problem| encode(&problem).ok())
        .map(Bytes::from)
        .unwrap_or_default();
    problem(StatusCode::UNPROCESSABLE_ENTITY, body)
}

pub(crate) fn problem(code: StatusCode, body: Bytes) -> Response {
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
