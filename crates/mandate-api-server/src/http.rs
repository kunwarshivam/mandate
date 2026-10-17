//! The two routers (§3.11, §3.3 item 5, API-1, DEC-978 items 3 and 4).
//!
//! The API router answers the one 401 for every path whose route has not landed, and adds CORS
//! headers only for the configured app origin. The probes' router answers `/livez` and `/readyz`
//! and a bodiless 404 for anything else.

use std::sync::Arc;

use axum::Router;

use crate::lifecycle::Lifecycle;

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

/// The API listener's router.
pub fn api(settings: Settings) -> Router {
    let _ = settings;
    Router::new()
}

/// The probes' listener's router.
pub fn probes(lifecycle: Arc<dyn Lifecycle>) -> Router {
    let _ = lifecycle;
    Router::new()
}
