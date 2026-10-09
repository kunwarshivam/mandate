//! COSE keys (RFC 9052, RFC 9053) for the two algorithms accepted (DEC-660 item 3), and
//! signature verification with them.

use ring::signature::{ECDSA_P256_SHA256_ASN1, ED25519, UnparsedPublicKey};

use crate::cbor::Cbor;
use crate::{PublicKey, Refusal};

const KTY: i64 = 1;
const ALG: i64 = 3;
const CRV: i64 = -1;
const X: i64 = -2;
const Y: i64 = -3;

const KTY_OKP: i64 = 1;
const KTY_EC2: i64 = 2;
const ALG_ES256: i64 = -7;
const ALG_EDDSA: i64 = -8;
const CRV_P256: i64 = 1;
const CRV_ED25519: i64 = 6;

fn label(n: i64) -> Cbor {
    match u64::try_from(n) {
        Ok(n) => Cbor::Uint(n),
        Err(_) => Cbor::Nint(n.unsigned_abs().saturating_sub(1)),
    }
}

pub(crate) fn public_key(key: &Cbor) -> Result<PublicKey, Refusal> {
    const MALFORMED: Refusal = Refusal::PublicKeyMalformed;
    let int = |n: i64| key.get(&label(n)).and_then(Cbor::as_int);
    let coordinate = |n: i64| match key.get(&label(n)) {
        Some(Cbor::Bytes(bytes)) => <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| MALFORMED),
        _ => Err(MALFORMED),
    };
    match int(ALG) {
        Some(ALG_ES256) => {
            if int(KTY) != Some(KTY_EC2) || int(CRV) != Some(CRV_P256) {
                return Err(MALFORMED);
            }
            Ok(PublicKey::Es256 {
                x: coordinate(X)?,
                y: coordinate(Y)?,
            })
        }
        Some(ALG_EDDSA) => {
            if int(KTY) != Some(KTY_OKP) || int(CRV) != Some(CRV_ED25519) {
                return Err(MALFORMED);
            }
            Ok(PublicKey::Ed25519 { x: coordinate(X)? })
        }
        Some(_) => Err(Refusal::AlgorithmUnsupported),
        None => Err(MALFORMED),
    }
}

pub(crate) fn verify(key: &PublicKey, message: &[u8], signature: &[u8]) -> Result<(), Refusal> {
    let checked = match key {
        PublicKey::Es256 { x, y } => {
            let point = [&[0x04][..], x, y].concat();
            UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, point).verify(message, signature)
        }
        PublicKey::Ed25519 { x } => UnparsedPublicKey::new(&ED25519, x).verify(message, signature),
    };
    checked.map_err(|_| Refusal::SignatureInvalid)
}
