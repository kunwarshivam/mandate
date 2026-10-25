//! A software authenticator for route 2's tests: ES256 and Ed25519 keys generated in the test,
//! the decoys' published secrets (DEC-2324 item 4), their COSE keys, and the assertions a
//! conforming browser sends for `rp()`, with an independent SHA-256 and base64url.

use mandate_canon::{Digest, encode_ulid};
use mandate_identity::PrincipalId;
use mandate_passkey::RelyingParty;
use ring::digest::{SHA256, digest};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, Ed25519KeyPair, KeyPair};
use serde_json::{Value as Json, json};

pub const RP_ID: &str = "app.example";
pub const ORIGIN: &str = "https://app.example";
const UP_UV: u8 = 0x05;

pub fn rp() -> RelyingParty {
    RelyingParty {
        rp_id: RP_ID.to_owned(),
        origin: ORIGIN.to_owned(),
    }
}

pub fn sha256(bytes: &[u8]) -> Vec<u8> {
    digest(&SHA256, bytes).as_ref().to_vec()
}

pub fn b64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let byte = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let n = (byte(0) << 16) | (byte(1) << 8) | byte(2);
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
        }
    }
    out
}

/// A software authenticator: one key, its COSE_Key, and §9.31's name for its algorithm.
pub enum Signer {
    Es256(EcdsaKeyPair),
    Ed25519(Ed25519KeyPair),
}

impl Signer {
    pub fn es256() -> Self {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
        Self::Es256(
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
                .unwrap(),
        )
    }

    pub fn ed25519() -> Self {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        Self::Ed25519(Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap())
    }

    /// The two decoys' signers, from their published secrets (DEC-2324 item 4).
    pub fn decoys() -> [Self; 2] {
        let mut one = [0; 32];
        one[31] = 1;
        let base_point = hex_bytes(
            "046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
        );
        let p256 = EcdsaKeyPair::from_private_key_and_public_key(
            &ECDSA_P256_SHA256_ASN1_SIGNING,
            &one,
            &base_point,
            &SystemRandom::new(),
        )
        .unwrap();
        let ed25519 = Ed25519KeyPair::from_seed_and_public_key(
            &hex_bytes("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"),
            &hex_bytes("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"),
        )
        .unwrap();
        [Self::Es256(p256), Self::Ed25519(ed25519)]
    }

    pub fn alg(&self) -> &'static str {
        match self {
            Self::Es256(_) => "es256",
            Self::Ed25519(_) => "eddsa",
        }
    }

    pub fn cose_key(&self) -> Vec<u8> {
        match self {
            Self::Es256(key) => {
                let point = key.public_key().as_ref();
                let mut out = vec![0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x58, 0x20];
                out.extend_from_slice(&point[1..33]);
                out.extend_from_slice(&[0x22, 0x58, 0x20]);
                out.extend_from_slice(&point[33..65]);
                out
            }
            Self::Ed25519(key) => {
                let mut out = vec![0xa4, 0x01, 0x01, 0x03, 0x27, 0x20, 0x06, 0x21, 0x58, 0x20];
                out.extend_from_slice(key.public_key().as_ref());
                out
            }
        }
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        match self {
            Self::Es256(key) => key
                .sign(&SystemRandom::new(), message)
                .unwrap()
                .as_ref()
                .to_vec(),
            Self::Ed25519(key) => key.sign(message).as_ref().to_vec(),
        }
    }

    /// An assertion over `challenge` for `rp()`: authenticator data, client data, signature.
    pub fn assert(&self, challenge: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let authenticator_data = [sha256(b"app.example"), vec![UP_UV], vec![0, 0, 0, 9]].concat();
        let client_data_json = format!(
            r#"{{"type":"webauthn.get","challenge":"{}","origin":"https://app.example","crossOrigin":false}}"#,
            b64url(challenge)
        )
        .into_bytes();
        let signature =
            self.sign(&[authenticator_data.clone(), sha256(&client_data_json)].concat());
        (authenticator_data, client_data_json, signature)
    }
}

pub fn hex_bytes(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

pub fn enrolled(member: PrincipalId, credential: &str, key: &[u8], alg: &str) -> Json {
    json!({
        "member": encode_ulid(member.0), "kind": "passkey", "credential": credential,
        "public_key": format!("sha256:{}", Digest::of(key)), "alg": alg, "sign_count": 0,
        "cool_off_ends": "2030-01-01T00:00:00.000000000Z", "step_up": null,
    })
}

pub fn removed(member: PrincipalId, credential: &str) -> Json {
    json!({
        "member": encode_ulid(member.0), "credential": credential,
        "step_up": {"assertion_id": "01JAA3P4A000000000000000S1", "authenticated_at": "2026-10-19T15:00:05.250000000Z", "method": "passkey"},
    })
}
