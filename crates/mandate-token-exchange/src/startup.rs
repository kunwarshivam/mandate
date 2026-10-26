//! The token-exchange process's start settings (connections spec §5.2 step 4; DEC-2629 item 3;
//! DEC-2633 item 2): the connection it exchanges for, and where its one request goes.
//!
//! The token URL is read first and refused before anything else: a build without debug assertions
//! has no code that could follow one, so it refuses to start whenever `MANDATE_TOKEN_URL` is set at
//! all. A debug build admits one only through [`crate::cell::parse`], in the `dev` and `ci` tiers.
//! Then the process must be the one the deployment manager starts as `token_exchange` (DEC-1175),
//! for one connection, with the platform's OAuth client id and this deployment's redirect URI
//! (DEC-2636 item 3).

use std::ffi::{OsStr, OsString};

use mandate_connections::record::ConnectionId;
use mandate_connections::start::{ClientId, OAuthClient, RedirectUri, STATE_LIFETIME_SECS};
use mandate_time::UtcNanos;

#[cfg(debug_assertions)]
use crate::cell::{self, CellTokenUrl};

/// The connection the deployment manager starts the process for (DEC-1175 item 1).
pub const SUBJECT: &str = "MANDATE_SUBJECT";

/// The process the deployment manager starts, by the journal's spelling (DEC-1175 item 1).
pub const PROCESS: &str = "MANDATE_PROCESS";

/// The only [`PROCESS`] this binary runs as.
pub const TOKEN_EXCHANGE: &str = "token_exchange";

/// The ServiceAccount the pod runs as: `x` and the lowercase hex of the subject's bytes, which the
/// deployment manager's admission policy pins (DEC-2060, DEC-1177). The value is self-reported:
/// checking it only catches a misconfigured pod early. What binds the process to its connection
/// is the vault's Kubernetes-auth login, which admits only that ServiceAccount, together with the
/// admission policy that pins the ServiceAccount to the subject (DEC-2330 item 4).
pub const SUBJECT_ACCOUNT: &str = "MANDATE_SUBJECT_ACCOUNT";

/// The platform's OAuth client id, public (DEC-2636 item 3).
pub const CLIENT_ID: &str = "MANDATE_ALPACA_CLIENT_ID";

/// This deployment's registered redirect URI, which the start sent too (DEC-2636 item 3).
pub const REDIRECT_URI: &str = "MANDATE_ALPACA_REDIRECT_URI";

/// The longest client id accepted, as the chart's schema bounds it.
pub const MAX_CLIENT_ID: usize = 128;

/// The deployment's tier, read as `mandate_release::Tier::from_setting` reads it.
pub const TIER: &str = "MANDATE_DEPLOYMENT_TIER";

/// A dev or CI cell's token URL (DEC-2629 item 3, DEC-2633 item 1).
pub const TOKEN_URL: &str = "MANDATE_TOKEN_URL";

/// The request's deadline, which the deployment manager renders from the `ConnectionRequested`
/// row's `recorded_at` and the pending state's lifetime, the pod's own deadline (DEC-2363 item 1).
pub const REQUEST_EXPIRES_AT: &str = "MANDATE_REQUEST_EXPIRES_AT";

/// Where the one token request goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// The URL `mandate_connections::hosts::LiveTokenRequest` pins.
    Live,
    /// A dev or CI cell's fixture provider.
    #[cfg(debug_assertions)]
    Cell(CellTokenUrl),
}

/// What the process starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub connection: ConnectionId,
    pub endpoint: Endpoint,
    pub client: OAuthClient,
}

/// Why the process refuses to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartRefused {
    /// A build without debug assertions was given a token URL, which it cannot follow.
    TokenUrlInRelease,
    /// The token URL [`crate::cell::parse`] refused.
    #[cfg(debug_assertions)]
    Cell(cell::Refused),
    /// `MANDATE_PROCESS` is not exactly `token_exchange`.
    Process,
    /// `MANDATE_SUBJECT` is unset or names no connection.
    Subject,
    /// `MANDATE_SUBJECT_ACCOUNT` is not `x` and the lowercase hex of the subject's bytes.
    SubjectAccount,
    /// `MANDATE_ALPACA_CLIENT_ID` is unset, empty, or not 1 to 128 of `A-Z a-z 0-9 . _ -`.
    ClientId,
    /// `MANDATE_ALPACA_REDIRECT_URI` is unset or not a redirect URI `RedirectUri::parse` admits.
    RedirectUri,
    /// `MANDATE_REQUEST_EXPIRES_AT` is unset (DEC-2363 item 2).
    RequestDeadlineMissing,
    /// `MANDATE_REQUEST_EXPIRES_AT` is not the journal's timestamp form (DEC-2363 item 2).
    RequestDeadlineMalformed,
    /// `MANDATE_REQUEST_EXPIRES_AT` is at or before the pod's clock (DEC-2363 item 2).
    RequestExpired,
}

impl StartRefused {
    /// The code the process writes, alone, before it exits.
    pub fn code(self) -> &'static str {
        match self {
            Self::TokenUrlInRelease => "token_exchange_token_url_in_release",
            #[cfg(debug_assertions)]
            Self::Cell(refused) => refused.code(),
            Self::Process => "token_exchange_process",
            Self::Subject => "token_exchange_subject",
            Self::SubjectAccount => "token_exchange_subject_account",
            Self::ClientId => "token_exchange_client_id",
            Self::RedirectUri => "token_exchange_redirect_uri",
            Self::RequestDeadlineMissing => "request_deadline_missing",
            Self::RequestDeadlineMalformed => "request_deadline_malformed",
            Self::RequestExpired => "request_expired",
        }
    }
}

/// The settings `var` gives, read as the process's environment is: raw, so a value that is not
/// UTF-8 is still a value set, and any other value that is not UTF-8 reads as none.
///
/// # Errors
/// In this order: [`endpoint`]'s refusal, then [`StartRefused::Process`],
/// [`StartRefused::Subject`], [`StartRefused::SubjectAccount`], [`StartRefused::ClientId`] and
/// [`StartRefused::RedirectUri`]. The
/// token URL is judged first, so a release build refuses it before anything else is read.
pub fn settings(var: impl Fn(&str) -> Option<OsString>) -> Result<Settings, StartRefused> {
    let text = |name: &str| var(name).and_then(|value| value.into_string().ok());
    let endpoint = endpoint(text(TIER).as_deref(), var(TOKEN_URL).as_deref())?;
    if text(PROCESS).as_deref() != Some(TOKEN_EXCHANGE) {
        return Err(StartRefused::Process);
    }
    let connection = text(SUBJECT)
        .and_then(|subject| ConnectionId::new(&subject).ok())
        .ok_or(StartRefused::Subject)?;
    if text(SUBJECT_ACCOUNT).as_deref() != Some(subject_account(&connection).as_str()) {
        return Err(StartRefused::SubjectAccount);
    }
    let client_id = text(CLIENT_ID)
        .filter(|id| client_id_admitted(id))
        .ok_or(StartRefused::ClientId)?;
    let redirect_uri = text(REDIRECT_URI)
        .and_then(|uri| RedirectUri::parse(&uri).ok())
        .ok_or(StartRefused::RedirectUri)?;
    Ok(Settings {
        connection,
        endpoint,
        client: OAuthClient {
            client_id: ClientId(client_id),
            redirect_uri,
        },
    })
}

/// The instant the wait for the code ends (DEC-2363 items 2 and 3): `value`, the request's
/// deadline, read as the journal's timestamp form exactly, when it is after `now`, the pod's
/// start; but never later than `now` plus `STATE_LIFETIME_SECS`, so a deadline far ahead still ends
/// within one lifetime.
///
/// # Errors
/// [`StartRefused::RequestDeadlineMissing`], [`StartRefused::RequestDeadlineMalformed`] (a value
/// that is not UTF-8 included), or [`StartRefused::RequestExpired`] for a deadline at or before
/// `now`.
pub fn request_deadline(value: Option<&OsStr>, now: UtcNanos) -> Result<UtcNanos, StartRefused> {
    let text = value.ok_or(StartRefused::RequestDeadlineMissing)?;
    let deadline = text
        .to_str()
        .and_then(|text| UtcNanos::parse(text).ok())
        .ok_or(StartRefused::RequestDeadlineMalformed)?;
    if deadline <= now {
        return Err(StartRefused::RequestExpired);
    }
    let one_lifetime = now
        .secs()
        .checked_add(STATE_LIFETIME_SECS)
        .and_then(|secs| UtcNanos::from_parts(secs, now.nanos()).ok())
        .unwrap_or(deadline);
    Ok(deadline.min(one_lifetime))
}

/// The ServiceAccount name of `connection`'s pod: `x` and its bytes in lowercase hex (DEC-2060).
pub fn subject_account(connection: &ConnectionId) -> String {
    let hex: String = connection
        .as_str()
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("x{hex}")
}

/// 1 to [`MAX_CLIENT_ID`] bytes of `A-Z a-z 0-9 . _ -`, the chart schema's pattern.
fn client_id_admitted(id: &str) -> bool {
    (1..=MAX_CLIENT_ID).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// The live endpoint when no token URL is set. A debug build admits a cell's when `tier` admits
/// `token_url`; a build without debug assertions refuses any token URL, whatever the tier.
///
/// # Errors
/// [`StartRefused::Cell`] in a debug build for a token URL [`crate::cell::parse`] refuses;
/// [`StartRefused::TokenUrlInRelease`] in any other build for any token URL, even an empty one or
/// one that is not UTF-8. A debug build judges one that is not UTF-8 with its bytes replaced, which
/// the cell check never admits.
pub fn endpoint(tier: Option<&str>, token_url: Option<&OsStr>) -> Result<Endpoint, StartRefused> {
    let Some(text) = token_url else {
        return Ok(Endpoint::Live);
    };
    #[cfg(debug_assertions)]
    let judged = cell::parse(tier, &text.to_string_lossy())
        .map(Endpoint::Cell)
        .map_err(StartRefused::Cell);
    #[cfg(not(debug_assertions))]
    let judged = {
        let _unread_in_release = (tier, text);
        Err(StartRefused::TokenUrlInRelease)
    };
    judged
}
