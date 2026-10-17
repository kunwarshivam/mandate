//! A deployment's signed call to the plane (DEC-984 item 2): Ed25519 by the key bound at
//! enrollment, over the method, path, deployment, time and body digest, within a skew of the
//! plane's clock. Every call from a deployment is one; the plane never calls a deployment.

use mandate_canon::Digest;
use mandate_time::UtcNanos;
use ring::signature::{ED25519, UnparsedPublicKey};

use crate::directory::{Ident, Refused};

/// How far a call's time may be from the plane's, either way.
pub const MAX_SKEW_SECS: i64 = 300;

/// One signed call, as the server extracts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed {
    pub deployment_id: Ident,
    pub method: String,
    pub path: String,
    /// The deployment's clock when it signed, in whole seconds.
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
    let skew = now
        .secs()
        .checked_sub(signed.at.secs())
        .and_then(i64::checked_abs)
        .ok_or(Refused::Stale)?;
    if skew > MAX_SKEW_SECS {
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
