//! The connect start and the callback's `state` check (connections spec §5.2 steps 1 to 3).
//! **API process only.** Nothing here names a token, and nothing here reaches a broker.

use std::collections::BTreeMap;
use std::fmt;

use mandate_time::UtcNanos;
use secrecy::SecretString;

use crate::ConnectError;
use crate::hosts::is_unreserved;
use crate::record::ConnectionId;

/// The callback's path, fixed outside the workspace prefix because a registered redirect URI
/// cannot carry a workspace id; the workspace travels in the `state` binding (workspace API §4.5).
pub const CALLBACK_PATH: &str = "/v1/oauth/alpaca/callback";
/// Production's redirect URI, on the workspace API's domain (DEC-820 item 1).
const PRODUCTION_REDIRECT_URI: &str = "https://api.owlhead.ai/v1/oauth/alpaca/callback";
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

/// Who started the connect, for which pending connection, and for which environment. Bound to the
/// `state` server-side, so the callback writes the code under that connection's vault entry
/// ([DEC-2623](../../../docs/project/decisions/DEC-2623.md) item 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub workspace_id: String,
    pub user_id: String,
    pub connection_id: ConnectionId,
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

/// The deployment's registered redirect URI: an `https` origin, or `http` on a loopback host for
/// the cell's fixture provider, with exactly [`CALLBACK_PATH`] and no query, fragment, or user
/// information (DEC-2623 item 2). Each deployment registers its own with the broker (connections
/// spec §13 question 5), and the exchange sends the same one (RFC 6749 §4.1.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedirectUri(String);

impl RedirectUri {
    /// Production's, `https://api.owlhead.ai/v1/oauth/alpaca/callback`.
    pub fn production() -> Self {
        Self(PRODUCTION_REDIRECT_URI.to_owned())
    }

    /// Accepts `text` only in the form the type states.
    ///
    /// # Errors
    /// [`ConnectError::RedirectUriInvalid`] for any other text.
    pub fn parse(text: &str) -> Result<Self, ConnectError> {
        let (loopback_only, rest) = if let Some(rest) = text.strip_prefix("https://") {
            (false, rest)
        } else if let Some(rest) = text.strip_prefix("http://") {
            (true, rest)
        } else {
            return Err(ConnectError::RedirectUriInvalid);
        };
        let authority = rest
            .strip_suffix(CALLBACK_PATH)
            .filter(|authority| !authority.contains('/'))
            .ok_or(ConnectError::RedirectUriInvalid)?;
        let (host, port) = split_authority(authority).ok_or(ConnectError::RedirectUriInvalid)?;
        let port_fits = port.is_none_or(|port| {
            port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok_and(|n| n != 0)
        });
        let host_fits = match host {
            Host::Bracketed(inner) => loopback_only && inner == "::1",
            Host::Named(name) if loopback_only => LOOPBACK_HOSTS.contains(&name),
            Host::Named(name) => is_dns_name(name),
        };
        if port_fits && host_fits {
            Ok(Self(text.to_owned()))
        } else {
            Err(ConnectError::RedirectUriInvalid)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The named hosts on which a redirect URI may be `http`, beside `[::1]`: the cell's fixture
/// provider runs on loopback.
const LOOPBACK_HOSTS: [&str; 2] = ["localhost", "127.0.0.1"];

/// An authority's host: a bracketed IPv6 literal's inside, or a name or IPv4 address.
enum Host<'a> {
    Bracketed(&'a str),
    Named(&'a str),
}

/// Splits `authority` into its host and its port's text, `None` without a port; `None` when a
/// bracket is unclosed, or something other than a port follows one.
fn split_authority(authority: &str) -> Option<(Host<'_>, Option<&str>)> {
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (inner, tail) = bracketed.split_once(']')?;
        let port = match tail {
            "" => None,
            tail => Some(tail.strip_prefix(':')?),
        };
        return Some((Host::Bracketed(inner), port));
    }
    Some(match authority.split_once(':') {
        Some((name, port)) => (Host::Named(name), Some(port)),
        None => (Host::Named(authority), None),
    })
}

/// A lowercase DNS name: dot-separated labels of `a-z`, `0-9`, and `-`, none empty and none
/// starting or ending with `-`.
fn is_dns_name(host: &str) -> bool {
    !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// The platform's OAuth client as one deployment registered it: its client id and redirect URI.
/// Neither is a secret; the client secret never reaches the API process (infrastructure §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthClient {
    pub client_id: ClientId,
    pub redirect_uri: RedirectUri,
}

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
    /// the client's id and redirect URI, the `state`, the PKCE challenge (`S256`),
    /// [`REQUESTED_SCOPES`], and `env` set to the binding's environment. Only a paper binding
    /// is accepted (DEC-821 item 3; DEC-441 item 21). A `state` already issued and not yet
    /// redeemed is refused, and the binding and verifier issued with it are kept unchanged.
    pub fn begin(
        &mut self,
        client: &OAuthClient,
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
            ("client_id", client.client_id.0.as_str()),
            ("redirect_uri", client.redirect_uri.as_str()),
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

    /// Drops every `state` expired at `now`, so a `state` never redeemed does not stay held
    /// (DEC-2623 item 4). One not yet expired is kept unchanged.
    pub fn prune(&mut self, now: UtcNanos) {
        self.issued
            .retain(|_, issued| now < expiry(issued.issued_at));
    }

    /// Whether `state` is held and not expired at `now`, without spending it (DEC-2623 item 8).
    pub fn holds(&self, state: &str, now: UtcNanos) -> bool {
        self.issued
            .get(state)
            .is_some_and(|issued| now < expiry(issued.issued_at))
    }

    /// How many `state` values are held.
    pub fn len(&self) -> usize {
        self.issued.len()
    }

    /// Whether no `state` is held.
    pub fn is_empty(&self) -> bool {
        self.issued.is_empty()
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
