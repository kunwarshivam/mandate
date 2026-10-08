//! `clientDataJSON` (WebAuthn §5.8.1): the ceremony type, the challenge, the origin, and the
//! cross-origin flag. Unknown members are ignored as the standard requires; a duplicate member,
//! a missing one, or anything but an object is refused.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;

use crate::{Challenge, Refusal, RelyingParty};

#[derive(Deserialize)]
struct ClientData {
    #[serde(rename = "type")]
    kind: String,
    challenge: String,
    origin: String,
    #[serde(rename = "crossOrigin", default)]
    cross_origin: Option<bool>,
}

/// The ceremony type a response must name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ceremony {
    Create,
    Get,
}

impl Ceremony {
    fn kind(self) -> &'static str {
        match self {
            Self::Create => "webauthn.create",
            Self::Get => "webauthn.get",
        }
    }
}

pub(crate) fn check(
    json: &[u8],
    ceremony: Ceremony,
    rp: &RelyingParty,
    challenge: &Challenge,
) -> Result<(), Refusal> {
    if json.first() != Some(&b'{') {
        return Err(Refusal::ClientDataMalformed);
    }
    let data: ClientData =
        serde_json::from_slice(json).map_err(|_| Refusal::ClientDataMalformed)?;
    if data.kind != ceremony.kind() {
        return Err(Refusal::ClientDataType);
    }
    if data.challenge != URL_SAFE_NO_PAD.encode(&challenge.0) {
        return Err(Refusal::ChallengeMismatch);
    }
    if data.origin != rp.origin {
        return Err(Refusal::OriginMismatch);
    }
    if data.cross_origin == Some(true) {
        return Err(Refusal::CrossOrigin);
    }
    Ok(())
}
