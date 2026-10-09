//! The connect start and the callback's `state` check (connections spec §5.2 steps 1 to 3).
//! **API process only.** Nothing here names a token, and nothing here reaches a broker.

use std::collections::BTreeMap;
use std::fmt;

use mandate_time::UtcNanos;
use secrecy::SecretString;

use crate::ConnectError;
use crate::hosts::is_unreserved;

/// The registered redirect URI (connections spec §5.2, DEC-820's domain). It is fixed because
/// a registered URI cannot carry a workspace id; the workspace travels in the `state` binding.
pub const REDIRECT_URI: &str = "https://api.owlhead.ai/v1/oauth/alpaca/callback";
/// Alpaca's authorization page.
pub const AUTHORIZE_URL: &str = "https://app.alpaca.markets/oauth/authorize";
/// The scopes requested, space-separated as Alpaca expects (connections spec §5.3).
pub const REQUESTED_SCOPES: &str = "trading data";
/// How long a `state` may be redeemed after it is issued (Proposed default, §5.2 step 1).
pub const STATE_LIFETIME_SECS: i64 = 600;
/// The `env` value of every authorization URL: only a paper binding is ever issued (DEC-821
/// item 3).
const PAPER_ENV: &str = "paper";

/// A connection's environment, fixed for its life (CN-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Paper,
    Live,
}

/// Who started the connect, and for which environment. Bound to the `state` server-side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub workspace_id: String,
    pub user_id: String,
    pub environment: Environment,
}

/// The PKCE verifier. Alpaca documents no PKCE, so it is sent but never relied on (DEC-690
/// item 9); it is still kept as a secret.
pub struct PkceVerifier(pub SecretString);

impl fmt::Debug for PkceVerifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PkceVerifier(redacted)")
    }
}

/// The platform's registered OAuth client id. Not a secret; the client secret never reaches
/// the API process (infrastructure §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientId(pub String);

/// The URL the browser is sent to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizeRedirect(pub String);

/// What the callback recovers from a redeemed `state`.
#[derive(Debug)]
pub struct Redeemed {
    pub binding: Binding,
    pub verifier: PkceVerifier,
}

#[derive(Debug)]
struct Issued {
    binding: Binding,
    verifier: PkceVerifier,
    issued_at: UtcNanos,
}

/// The single-use `state` values issued and not yet redeemed. The caller supplies the random
/// `state`, the verifier, its challenge, and the time, so this core reads no clock and draws no
/// randomness.
#[derive(Debug, Default)]
pub struct PendingStates {
    issued: BTreeMap<String, Issued>,
}

impl PendingStates {
    /// Issues `state` for `binding` and returns the authorization URL: `response_type=code`,
    /// the client id, [`REDIRECT_URI`], the `state`, the PKCE challenge (`S256`),
    /// [`REQUESTED_SCOPES`], and `env` set to the binding's environment. Only a paper binding
    /// is accepted (DEC-821 item 3; DEC-441 item 21). A `state` already issued and not yet
    /// redeemed is refused, and the binding and verifier issued with it are kept unchanged.
    pub fn begin(
        &mut self,
        client_id: &ClientId,
        binding: Binding,
        state: &str,
        verifier: PkceVerifier,
        challenge: &str,
        now: UtcNanos,
    ) -> Result<AuthorizeRedirect, ConnectError> {
        if binding.environment != Environment::Paper {
            return Err(ConnectError::EnvironmentRefused);
        }
        if self.issued.contains_key(state) {
            return Err(ConnectError::StateReused);
        }
        let parameters = [
            ("response_type", "code"),
            ("client_id", client_id.0.as_str()),
            ("redirect_uri", REDIRECT_URI),
            ("state", state),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
            ("scope", REQUESTED_SCOPES),
            ("env", PAPER_ENV),
        ];
        let query: Vec<String> = parameters
            .iter()
            .map(|(name, value)| format!("{name}={}", percent_encode(value)))
            .collect();
        self.issued.insert(
            state.to_owned(),
            Issued {
                binding,
                verifier,
                issued_at: now,
            },
        );
        Ok(AuthorizeRedirect(format!(
            "{AUTHORIZE_URL}?{}",
            query.join("&")
        )))
    }

    /// Redeems `state` once, returning the binding and the verifier issued with it. Unknown,
    /// expired, or another user's `state` is refused, and in every case a `state` that was found
    /// is spent, so it can never be tried again.
    pub fn redeem(
        &mut self,
        state: &str,
        user_id: &str,
        now: UtcNanos,
    ) -> Result<Redeemed, ConnectError> {
        let issued = self
            .issued
            .remove(state)
            .ok_or(ConnectError::StateUnknown)?;
        if now >= expiry(issued.issued_at) {
            return Err(ConnectError::StateExpired);
        }
        if issued.binding.user_id != user_id {
            return Err(ConnectError::StateUserMismatch);
        }
        Ok(Redeemed {
            binding: issued.binding,
            verifier: issued.verifier,
        })
    }
}

/// The first instant at which a `state` issued at `issued_at` is expired. Past the last
/// representable instant every `state` counts as expired, the reading that admits nothing.
fn expiry(issued_at: UtcNanos) -> UtcNanos {
    UtcNanos::from_parts(
        issued_at.secs().saturating_add(STATE_LIFETIME_SECS),
        issued_at.nanos(),
    )
    .unwrap_or(UtcNanos::EPOCH)
}

/// Percent-encodes every byte of `value` but RFC 3986 unreserved characters, so a space is
/// `%20` and no value can add or split a query parameter.
fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            let c = char::from(byte);
            if is_unreserved(c) {
                c.to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}
