//! A deployment's signed call to the plane (DEC-984 item 2): Ed25519 by the key bound at
//! enrollment, over the method, path, deployment, time and body digest, within a skew of the
//! plane's clock. Every call from a deployment is one; the plane never calls a deployment.

use mandate_canon::Digest;
use mandate_time::UtcNanos;
use ring::signature::{ED25519, UnparsedPublicKey};

use crate::directory::{Ident, Refused};

/// How far a call's time may be behind the plane's.
pub const MAX_SKEW_SECS: i64 = 300;

/// How far a call's time may be ahead of the plane's. Small, because each call must be later than
/// the last: a call far in the future would lock its deployment out until real time caught up.
pub const MAX_AHEAD_SECS: i64 = 5;

/// The calls a deployment makes, each on its one route. A call's method and path are checked
/// against its operation, and both are fixed text, so the signed bytes are never ambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Heartbeat,
    Directory,
    Routes,
}

impl Operation {
    /// The method and path the call is sent to.
    pub fn route(self) -> (&'static str, &'static str) {
        match self {
            Self::Heartbeat => ("POST", "/v1/heartbeat"),
            Self::Directory => ("POST", "/v1/directory"),
            Self::Routes => ("POST", "/v1/routes"),
        }
    }
}

/// One signed call, as the server extracts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed {
    pub deployment_id: Ident,
    pub method: String,
    pub path: String,
    /// The deployment's clock when it signed; each call's time is after the last one's.
    pub at: UtcNanos,
    pub body: Vec<u8>,
    pub signature: Vec<u8>,
}

/// The bytes a deployment signs: a domain tag, then the method, path, deployment, time and the
/// body's SHA-256, each separated by a zero byte.
pub fn signed_request_bytes(
    method: &str,
    path: &str,
    deployment_id: &Ident,
    at: UtcNanos,
    body: &[u8],
) -> Vec<u8> {
    let mut out = b"mandate-cp-req/1\0".to_vec();
    for part in [
        method,
        path,
        deployment_id.as_str(),
        &at.to_string(),
        &Digest::of(body).to_hex(),
    ] {
        out.extend_from_slice(part.as_bytes());
        out.push(0);
    }
    out
}

pub(crate) fn verify(signed: &Signed, key: &[u8; 32], now: UtcNanos) -> Result<(), Refused> {
    let behind = now
        .secs()
        .checked_sub(signed.at.secs())
        .ok_or(Refused::Stale)?;
    if behind > MAX_SKEW_SECS || behind < MAX_AHEAD_SECS.saturating_neg() {
        return Err(Refused::Stale);
    }
    let bytes = signed_request_bytes(
        &signed.method,
        &signed.path,
        &signed.deployment_id,
        signed.at,
        &signed.body,
    );
    UnparsedPublicKey::new(&ED25519, key)
        .verify(&bytes, &signed.signature)
        .map_err(|_| Refused::BadSignature)
}
