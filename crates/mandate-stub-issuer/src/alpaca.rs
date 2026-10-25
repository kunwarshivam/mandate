//! The fixture Alpaca OAuth provider for dev and CI cells
//! ([DEC-2625](../../../docs/project/decisions/DEC-2625.md)): `GET /alpaca/oauth/authorize` and
//! `POST /alpaca/oauth/token`, answering as connections spec §5.2 and §5.5 say Alpaca's do, for one
//! configured client and redirect URI. Codes live in memory only, at most [`MAX_CODES`]; tokens are
//! not kept. Nothing here reads a clock or logs: the router passes the time in.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mandate_connections::start::RedirectUri;
use ring::digest::{SHA256, digest};
use ring::rand::{SecureRandom, SystemRandom};

use crate::start::Refused;

/// The authorization page's path.
pub const AUTHORIZE_PATH: &str = "/alpaca/oauth/authorize";

/// The token endpoint's path.
pub const TOKEN_PATH: &str = "/alpaca/oauth/token";

/// The one scope request accepted and granted, as `start::REQUESTED_SCOPES` sends it.
pub const SCOPE: &str = "trading data";

/// How long a code may be redeemed, in seconds (connections spec §5.5).
pub const CODE_TTL_S: i64 = 600;

/// The most codes held at once (DEC-2625 item 5).
pub const MAX_CODES: usize = 4096;

/// The bytes of a code or a token before base64url.
pub const RANDOM_BYTES: usize = 32;

/// Where codes and tokens come from: the operating system's random source in the stub, a seeded one
/// in tests.
pub trait Random: Send + Sync {
    /// Fills `out`; `false` when the source failed.
    fn fill(&self, out: &mut [u8]) -> bool;
}

impl Random for SystemRandom {
    fn fill(&self, out: &mut [u8]) -> bool {
        SecureRandom::fill(self, out).is_ok()
    }
}

/// The client the provider serves (DEC-2625 item 2). It has no `Debug` form that could print the
/// secret.
pub struct Settings {
    pub client_id: String,
    pub client_secret: Vec<u8>,
    pub redirect_uri: String,
}

/// Why a request is refused: RFC 6749 §5.2's codes, and a full store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    InvalidRequest,
    InvalidClient,
    InvalidGrant,
    Unavailable,
}

impl Refusal {
    /// The `error` member of the answer.
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidClient => "invalid_client",
            Self::InvalidGrant => "invalid_grant",
            Self::Unavailable => "temporarily_unavailable",
        }
    }

    /// The status of the answer: 503 for a full store, 400 otherwise.
    pub fn status(self) -> u16 {
        match self {
            Self::Unavailable => 503,
            _ => 400,
        }
    }
}

/// Which check refused a token request, for the stub's standard error (DEC-2127): a fixed word,
/// never a value the request carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// Not exactly DEC-2625 item 4's form, or not UTF-8.
    Form,
    /// A grant type other than `authorization_code`.
    GrantType,
    /// Another client id or client secret.
    Client,
    /// A code the store does not hold.
    CodeUnknown,
    /// A held code past its expiry.
    CodeExpired,
    /// Another redirect URI than the code's.
    RedirectUri,
    /// A verifier that is not the code's challenge.
    CodeVerifier,
    /// The token store is full, or the random source failed.
    StoreFull,
}

impl Cause {
    /// The cause's word in the stub's standard error.
    pub fn word(self) -> &'static str {
        match self {
            Self::Form => "form",
            Self::GrantType => "grant_type",
            Self::Client => "client",
            Self::CodeUnknown => "code_unknown",
            Self::CodeExpired => "code_expired",
            Self::RedirectUri => "redirect_uri",
            Self::CodeVerifier => "code_verifier",
            Self::StoreFull => "store_full",
        }
    }
}

/// The line the stub writes to standard error for `refusal` by `cause`: `token_refused`, the
/// answer's `error`, and the cause's word (DEC-2127).
pub fn refusal_line(refusal: Refusal, cause: Cause) -> String {
    format!("token_refused {} {}\n", refusal.code(), cause.word())
}

/// Why the client secret file is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretFileError {
    /// It cannot be read, or is not a regular file.
    Unreadable,
    /// A group or other permission bit is set.
    Mode,
    /// It is empty.
    Empty,
}

/// The authorization request's parameters, each exactly once (connections spec §5.2 step 2).
const AUTHORIZE_PARAMS: [&str; 8] = [
    "response_type",
    "client_id",
    "redirect_uri",
    "state",
    "code_challenge",
    "code_challenge_method",
    "scope",
    "env",
];

/// The token request's parameters, each exactly once (connections spec §5.2 step 4).
const TOKEN_PARAMS: [&str; 6] = [
    "grant_type",
    "code",
    "client_id",
    "client_secret",
    "redirect_uri",
    "code_verifier",
];

/// The length of an S256 challenge: a SHA-256 digest in base64url without padding (RFC 7636).
const CHALLENGE_LEN: usize = 43;

/// What a code was issued for.
struct Issued {
    redirect_uri: String,
    challenge: String,
    issued_at: i64,
}

/// The provider: its client and the codes it issued.
pub struct Alpaca {
    settings: Settings,
    random: Box<dyn Random>,
    codes: Mutex<BTreeMap<String, Issued>>,
}

impl Alpaca {
    pub fn new(settings: Settings, random: Box<dyn Random>) -> Self {
        Self {
            settings,
            random,
            codes: Mutex::new(BTreeMap::new()),
        }
    }

    /// 32 random bytes in base64url, or `None` when the source failed.
    fn fresh(&self) -> Option<String> {
        let mut bytes = [0_u8; RANDOM_BYTES];
        self.random
            .fill(&mut bytes)
            .then(|| URL_SAFE_NO_PAD.encode(bytes))
    }

    /// The `Location` to answer `GET /alpaca/oauth/authorize?<query>` with at `now` (Unix
    /// seconds): the redirect URI with a fresh `code` and the request's `state`.
    ///
    /// # Errors
    /// [`Refusal::InvalidRequest`] for anything but exactly DEC-2625 item 3's parameters, and
    /// [`Refusal::Unavailable`] for a full store or a failed random source.
    pub fn authorize(&self, query: &str, now: i64) -> Result<String, Refusal> {
        let params = parse(query)
            .and_then(|params| exactly(&params, &AUTHORIZE_PARAMS))
            .ok_or(Refusal::InvalidRequest)?;
        let value = |name: &str| params.get(name).map(String::as_str).unwrap_or_default();
        let challenge = value("code_challenge");
        let fits = value("response_type") == "code"
            && value("client_id") == self.settings.client_id
            && value("redirect_uri") == self.settings.redirect_uri
            && !value("state").is_empty()
            && is_challenge(challenge)
            && value("code_challenge_method") == "S256"
            && value("scope") == SCOPE
            && value("env") == "paper";
        if !fits {
            return Err(Refusal::InvalidRequest);
        }
        let mut codes = self.codes.lock().unwrap_or_else(PoisonError::into_inner);
        codes.retain(|_, issued| now < expiry(issued.issued_at));
        if codes.len() >= MAX_CODES {
            return Err(Refusal::Unavailable);
        }
        let code = self.fresh().ok_or(Refusal::Unavailable)?;
        codes.insert(
            code.clone(),
            Issued {
                redirect_uri: self.settings.redirect_uri.clone(),
                challenge: challenge.to_owned(),
                issued_at: now,
            },
        );
        Ok(format!(
            "{}?code={code}&state={}",
            self.settings.redirect_uri,
            percent_encode(value("state"))
        ))
    }

    /// The JSON body to answer `POST /alpaca/oauth/token` with, for the form `body` at `now`.
    ///
    /// # Errors
    /// DEC-2625 item 4's refusals.
    pub fn token(&self, body: &str, now: i64) -> Result<String, Refusal> {
        self.exchange(body, now).map_err(|(refusal, _)| refusal)
    }

    /// [`Alpaca::token`], with the [`Cause`] of a refusal (DEC-2127).
    ///
    /// # Errors
    /// [`Alpaca::token`]'s refusals.
    pub fn exchange(&self, body: &str, now: i64) -> Result<String, (Refusal, Cause)> {
        let params = parse(body)
            .and_then(|params| exactly(&params, &TOKEN_PARAMS))
            .ok_or((Refusal::InvalidRequest, Cause::Form))?;
        let value = |name: &str| params.get(name).map(String::as_str).unwrap_or_default();
        if value("grant_type") != "authorization_code" {
            return Err((Refusal::InvalidRequest, Cause::GrantType));
        }
        let issued = self
            .codes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(value("code"));
        let client = value("client_id") == self.settings.client_id
            && same_secret(
                value("client_secret").as_bytes(),
                &self.settings.client_secret,
            );
        if !client {
            return Err((Refusal::InvalidClient, Cause::Client));
        }
        let issued = issued.ok_or((Refusal::InvalidGrant, Cause::CodeUnknown))?;
        let refused = if now >= expiry(issued.issued_at) {
            Some(Cause::CodeExpired)
        } else if value("redirect_uri") != issued.redirect_uri {
            Some(Cause::RedirectUri)
        } else if s256(value("code_verifier")) != issued.challenge {
            Some(Cause::CodeVerifier)
        } else {
            None
        };
        if let Some(cause) = refused {
            return Err((Refusal::InvalidGrant, cause));
        }
        let token = self
            .fresh()
            .ok_or((Refusal::Unavailable, Cause::StoreFull))?;
        Ok(serde_json::json!({
            "access_token": token,
            "scope": SCOPE,
            "token_type": "bearer",
        })
        .to_string())
    }

    /// How many codes are held.
    pub fn held(&self) -> usize {
        self.codes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// The first second at which a code issued at `issued_at` is expired.
fn expiry(issued_at: i64) -> i64 {
    issued_at.saturating_add(CODE_TTL_S)
}

/// An S256 challenge's form: 43 base64url characters.
fn is_challenge(challenge: &str) -> bool {
    challenge.len() == CHALLENGE_LEN
        && challenge
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The S256 challenge of `verifier` (RFC 7636 §4.2).
fn s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(digest(&SHA256, verifier.as_bytes()))
}

/// Whether `given` is `secret`, compared through their SHA-256 digests, so how long the comparison
/// takes can show only where two digests differ, nothing of the secret.
fn same_secret(given: &[u8], secret: &[u8]) -> bool {
    digest(&SHA256, given).as_ref() == digest(&SHA256, secret).as_ref()
}

/// A query or form's pairs, percent-decoded with `+` as a space; `None` for a pair without `=`, a
/// bad escape, or bytes that are not UTF-8.
fn parse(text: &str) -> Option<Vec<(String, String)>> {
    if text.is_empty() {
        return Some(Vec::new());
    }
    text.split('&')
        .map(|pair| {
            let (name, value) = pair.split_once('=')?;
            Some((percent_decode(name)?, percent_decode(value)?))
        })
        .collect()
}

/// `params` as a map when it names each of `names` exactly once and nothing else.
fn exactly(params: &[(String, String)], names: &[&str]) -> Option<BTreeMap<String, String>> {
    let map: BTreeMap<String, String> = params.iter().cloned().collect();
    let fits = map.len() == params.len()
        && map.len() == names.len()
        && names.iter().all(|name| map.contains_key(*name));
    fits.then_some(map)
}

fn percent_decode(text: &str) -> Option<String> {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.bytes();
    while let Some(byte) = bytes.next() {
        out.push(match byte {
            b'+' => b' ',
            b'%' => {
                let high = char::from(bytes.next()?).to_digit(16)?;
                let low = char::from(bytes.next()?).to_digit(16)?;
                u8::try_from(high.checked_mul(16)?.checked_add(low)?).ok()?
            }
            other => other,
        });
    }
    String::from_utf8(out).ok()
}

/// RFC 3986 percent-encoding of everything but the unreserved characters.
fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// The client secret from `path`: a regular file with no group or other permission bit.
///
/// # Errors
/// [`SecretFileError`].
pub fn read_secret(path: &Path) -> Result<Vec<u8>, SecretFileError> {
    let metadata = std::fs::metadata(path).map_err(|_| SecretFileError::Unreadable)?;
    if !metadata.is_file() {
        return Err(SecretFileError::Unreadable);
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(SecretFileError::Mode);
    }
    let secret = std::fs::read(path).map_err(|_| SecretFileError::Unreadable)?;
    if secret.is_empty() {
        return Err(SecretFileError::Empty);
    }
    Ok(secret)
}

/// The provider's settings from its three flags' values (DEC-2625 items 1 and 2): `None` with none
/// of them, the settings with all three, the client secret read from its file.
///
/// # Errors
/// [`Refused::Alpaca`] for some but not all three, an empty client id or redirect URI, or a secret
/// file [`read_secret`] refuses, or a redirect URI outside DEC-2623 item 2's shape.
pub fn settings(
    client_id: Option<&str>,
    secret_file: Option<&str>,
    redirect_uri: Option<&str>,
) -> Result<Option<Settings>, Refused> {
    match (client_id, secret_file, redirect_uri) {
        (None, None, None) => Ok(None),
        (Some(client_id), Some(secret_file), Some(redirect_uri)) if !client_id.is_empty() => {
            RedirectUri::parse(redirect_uri).map_err(|_| Refused::Alpaca)?;
            let client_secret = read_secret(Path::new(secret_file)).map_err(|_| Refused::Alpaca)?;
            Ok(Some(Settings {
                client_id: client_id.to_owned(),
                client_secret,
                redirect_uri: redirect_uri.to_owned(),
            }))
        }
        _ => Err(Refused::Alpaca),
    }
}
