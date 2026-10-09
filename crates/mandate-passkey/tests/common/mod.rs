//! A software authenticator for the passkey tests: keys generated in the test, never a real
//! authenticator, account, or identity provider (identity spec §1.3, §14). Its CBOR writer,
//! base64url encoder, and SHA-256 (ring's) share no code with the crate, so the responses it
//! builds are an independent oracle for what a conforming browser sends.
//!
//! [`Ceremony::create`] and [`Ceremony::get`] describe a response every check passes; each test
//! changes the one field its title names.

#![allow(
    dead_code,
    reason = "each test binary uses a different subset of the fixtures"
)]

use mandate_passkey::{Assertion, Challenge, Credential, PublicKey, Registration, RelyingParty};
use ring::digest::{SHA256, digest};
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, Ed25519KeyPair, KeyPair};

/// The production RP ID and origin (DEC-820), as configuration the tests pass in.
pub const RP_ID: &str = "app.owlhead.ai";
pub const ORIGIN: &str = "https://app.owlhead.ai";

pub const UP: u8 = 0x01;
pub const UV: u8 = 0x04;
pub const BE: u8 = 0x08;
pub const BS: u8 = 0x10;
pub const AT: u8 = 0x40;
pub const ED: u8 = 0x80;

pub fn rp() -> RelyingParty {
    RelyingParty {
        rp_id: RP_ID.to_owned(),
        origin: ORIGIN.to_owned(),
    }
}

pub fn challenge(bytes: &[u8]) -> Challenge {
    Challenge::new(bytes).expect("a fixture challenge has at least 16 bytes")
}

pub fn sha256(bytes: &[u8]) -> Vec<u8> {
    digest(&SHA256, bytes).as_ref().to_vec()
}

/// RFC 4648 §5 base64url without padding, written out here rather than taken from a crate.
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

pub fn cbor_head(major: u8, n: u64) -> Vec<u8> {
    let major = major << 5;
    match n {
        0..=23 => vec![major | n as u8],
        24..=0xff => vec![major | 24, n as u8],
        0x100..=0xffff => [vec![major | 25], (n as u16).to_be_bytes().to_vec()].concat(),
        0x1_0000..=0xffff_ffff => [vec![major | 26], (n as u32).to_be_bytes().to_vec()].concat(),
        _ => [vec![major | 27], n.to_be_bytes().to_vec()].concat(),
    }
}

pub fn cbor_int(n: i64) -> Vec<u8> {
    if n >= 0 {
        cbor_head(0, n as u64)
    } else {
        cbor_head(1, (-1 - n) as u64)
    }
}

pub fn cbor_bytes(bytes: &[u8]) -> Vec<u8> {
    [cbor_head(2, bytes.len() as u64), bytes.to_vec()].concat()
}

pub fn cbor_text(text: &str) -> Vec<u8> {
    [cbor_head(3, text.len() as u64), text.as_bytes().to_vec()].concat()
}

pub fn cbor_map(entries: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut out = cbor_head(5, entries.len() as u64);
    for (key, value) in entries {
        out.extend_from_slice(key);
        out.extend_from_slice(value);
    }
    out
}

pub fn attestation_object(fmt: &str, att_stmt: &[u8], auth_data: &[u8]) -> Vec<u8> {
    cbor_map(&[
        (cbor_text("fmt"), cbor_text(fmt)),
        (cbor_text("attStmt"), att_stmt.to_vec()),
        (cbor_text("authData"), cbor_bytes(auth_data)),
    ])
}

/// What one ceremony's response says.
#[derive(Debug, Clone)]
pub struct Ceremony {
    pub kind: String,
    /// The challenge as it appears in `clientDataJSON`, already encoded.
    pub challenge: String,
    pub origin: String,
    pub cross_origin: Option<bool>,
    pub rp_id: String,
    pub flags: u8,
    pub counter: u32,
}

impl Ceremony {
    pub fn create(challenge: &[u8]) -> Self {
        Self {
            kind: "webauthn.create".to_owned(),
            challenge: b64url(challenge),
            origin: ORIGIN.to_owned(),
            cross_origin: Some(false),
            rp_id: RP_ID.to_owned(),
            flags: UP | UV | AT,
            counter: 0,
        }
    }

    pub fn get(challenge: &[u8], counter: u32) -> Self {
        Self {
            kind: "webauthn.get".to_owned(),
            flags: UP | UV,
            counter,
            ..Self::create(challenge)
        }
    }

    pub fn client_data_json(&self) -> Vec<u8> {
        let mut json = format!(
            r#"{{"type":"{}","challenge":"{}","origin":"{}""#,
            self.kind, self.challenge, self.origin
        );
        if let Some(cross) = self.cross_origin {
            json.push_str(&format!(r#","crossOrigin":{cross}"#));
        }
        json.push('}');
        json.into_bytes()
    }

    /// RP ID hash, flags, and counter: the whole of an assertion's authenticator data.
    pub fn auth_data_head(&self) -> Vec<u8> {
        [
            sha256(self.rp_id.as_bytes()),
            vec![self.flags],
            self.counter.to_be_bytes().to_vec(),
        ]
        .concat()
    }
}

pub struct OwnedRegistration {
    pub attestation_object: Vec<u8>,
    pub client_data_json: Vec<u8>,
}

impl OwnedRegistration {
    pub fn view(&self) -> Registration<'_> {
        Registration {
            attestation_object: &self.attestation_object,
            client_data_json: &self.client_data_json,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OwnedAssertion {
    pub credential_id: Vec<u8>,
    pub authenticator_data: Vec<u8>,
    pub client_data_json: Vec<u8>,
    pub signature: Vec<u8>,
}

impl OwnedAssertion {
    pub fn view(&self) -> Assertion<'_> {
        Assertion {
            credential_id: &self.credential_id,
            authenticator_data: &self.authenticator_data,
            client_data_json: &self.client_data_json,
            signature: &self.signature,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alg {
    Es256,
    Ed25519,
}

enum Signer {
    Es256(EcdsaKeyPair),
    Ed25519(Ed25519KeyPair),
}

pub struct Authenticator {
    signer: Signer,
    rng: SystemRandom,
    pub credential_id: Vec<u8>,
}

impl Authenticator {
    pub fn new(alg: Alg) -> Self {
        let rng = SystemRandom::new();
        let signer = match alg {
            Alg::Es256 => {
                let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng)
                    .expect("generate a P-256 key");
                Signer::Es256(
                    EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
                        .expect("load the P-256 key"),
                )
            }
            Alg::Ed25519 => {
                let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).expect("generate an Ed25519 key");
                Signer::Ed25519(
                    Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("load the Ed25519 key"),
                )
            }
        };
        let mut credential_id = vec![0; 16];
        rng.fill(&mut credential_id).expect("draw a credential ID");
        Self {
            signer,
            rng,
            credential_id,
        }
    }

    pub fn public_key(&self) -> PublicKey {
        match &self.signer {
            Signer::Es256(key) => {
                let point = key.public_key().as_ref();
                PublicKey::Es256 {
                    x: point[1..33].try_into().expect("x"),
                    y: point[33..65].try_into().expect("y"),
                }
            }
            Signer::Ed25519(key) => PublicKey::Ed25519 {
                x: key.public_key().as_ref().try_into().expect("x"),
            },
        }
    }

    /// The COSE_Key (RFC 9053) a conforming authenticator reports for this key.
    pub fn cose_key(&self) -> Vec<u8> {
        match self.public_key() {
            PublicKey::Es256 { x, y } => cbor_map(&[
                (cbor_int(1), cbor_int(2)),
                (cbor_int(3), cbor_int(-7)),
                (cbor_int(-1), cbor_int(1)),
                (cbor_int(-2), cbor_bytes(&x)),
                (cbor_int(-3), cbor_bytes(&y)),
            ]),
            PublicKey::Ed25519 { x } => cbor_map(&[
                (cbor_int(1), cbor_int(1)),
                (cbor_int(3), cbor_int(-8)),
                (cbor_int(-1), cbor_int(6)),
                (cbor_int(-2), cbor_bytes(&x)),
            ]),
        }
    }

    /// Authenticator data with attested credential data carrying `cose_key`.
    pub fn attested_auth_data(&self, ceremony: &Ceremony, cose_key: &[u8]) -> Vec<u8> {
        [
            ceremony.auth_data_head(),
            vec![0; 16],
            (self.credential_id.len() as u16).to_be_bytes().to_vec(),
            self.credential_id.clone(),
            cose_key.to_vec(),
        ]
        .concat()
    }

    pub fn register(&self, ceremony: &Ceremony) -> OwnedRegistration {
        let auth_data = self.attested_auth_data(ceremony, &self.cose_key());
        OwnedRegistration {
            attestation_object: attestation_object("none", &cbor_map(&[]), &auth_data),
            client_data_json: ceremony.client_data_json(),
        }
    }

    /// The credential the relying party stores for this authenticator.
    pub fn credential(&self, sign_count: u32) -> Credential {
        Credential {
            id: self.credential_id.clone(),
            public_key: self.public_key(),
            sign_count,
            backup_eligible: false,
        }
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        match &self.signer {
            Signer::Es256(key) => key
                .sign(&self.rng, message)
                .expect("sign")
                .as_ref()
                .to_vec(),
            Signer::Ed25519(key) => key.sign(message).as_ref().to_vec(),
        }
    }

    /// Signs `authenticator_data || SHA-256(clientDataJSON)` as WebAuthn §6.3.3 says.
    pub fn sign_response(&self, authenticator_data: &[u8], client_data_json: &[u8]) -> Vec<u8> {
        self.sign(&[authenticator_data, &sha256(client_data_json)].concat())
    }

    pub fn assert(&self, ceremony: &Ceremony) -> OwnedAssertion {
        let authenticator_data = ceremony.auth_data_head();
        let client_data_json = ceremony.client_data_json();
        OwnedAssertion {
            credential_id: self.credential_id.clone(),
            signature: self.sign_response(&authenticator_data, &client_data_json),
            authenticator_data,
            client_data_json,
        }
    }
}
