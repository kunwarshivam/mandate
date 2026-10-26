//! A software authenticator for passkey step-up and enrolment (DEC-2108 items 4, 7 and 8,
//! DEC-2126): one key, its COSE_Key, §9.31's records for it, the registration responses that enrol
//! it, and assertions it signs over a challenge, as a browser would send them. The encoders and the
//! canonical form of a sent assertion are written out here, so the tests check the server with
//! oracles of their own.

#![allow(dead_code, reason = "each test file uses part of the shared helpers")]

use mandate_api::envelope::StepUpAssertion;
use mandate_canon::{Digest, encode_ulid};
use mandate_identity::PrincipalId;
use mandate_passkey::{Assertion, Credential, PublicKey, RelyingParty};
use ring::digest::{SHA256, digest};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, Ed25519KeyPair, KeyPair};
use serde_json::{Value as Json, json};

use crate::common::control::Rows;
use crate::common::{at, b64url};

/// The relying party the connect fixture's app origin names.
pub const RP_ID: &str = "app.owlhead.test";
pub const ORIGIN: &str = "https://app.owlhead.test";
/// A passkey id: the 16 bytes 0 to 15, unpadded base64url.
pub const CREDENTIAL: &str = "AAECAwQFBgcICQoLDA0ODw";
/// Another passkey id: the 16 bytes 16 to 31.
pub const OTHER_CREDENTIAL: &str = "EBESExQVFhcYGRobHB0eHw";
/// User presence and user verification, and neither backup flag.
const UP_UV: u8 = 0x05;
/// The backup-eligible flag (WebAuthn's BE bit).
const BE: u8 = 0x08;

pub fn rp() -> RelyingParty {
    RelyingParty {
        rp_id: RP_ID.to_owned(),
        origin: ORIGIN.to_owned(),
    }
}

fn sha256(bytes: &[u8]) -> Vec<u8> {
    digest(&SHA256, bytes).as_ref().to_vec()
}

/// The test's own decoding of unpadded base64url.
pub fn unb64url(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let sextets: Vec<u32> = text
        .bytes()
        .map(|c| {
            let at = ALPHABET.iter().position(|b| *b == c).expect("base64url");
            u32::try_from(at).expect("6 bits")
        })
        .collect();
    sextets
        .chunks(4)
        .flat_map(|chunk| {
            let n = chunk.iter().fold(0u32, |acc, s| (acc << 6) | s) << (6 * (4 - chunk.len()));
            n.to_be_bytes()[1..chunk.len()].to_vec()
        })
        .collect()
}

/// What the authenticator returned for one challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed {
    pub authenticator_data: Vec<u8>,
    pub client_data_json: Vec<u8>,
    pub signature: Vec<u8>,
}

impl Signed {
    /// The request's `step_up_assertion` for `passkey_id`.
    pub fn wire(&self, passkey_id: &str) -> StepUpAssertion {
        StepUpAssertion {
            passkey_id: passkey_id.to_owned(),
            authenticator_data: b64url(&self.authenticator_data),
            client_data_json: b64url(&self.client_data_json),
            signature: b64url(&self.signature),
        }
    }

    /// The `step_up_assertion` member as JSON.
    pub fn json(&self, passkey_id: &str) -> Json {
        json!({
            "passkey_id": passkey_id,
            "authenticator_data": b64url(&self.authenticator_data),
            "client_data_json": b64url(&self.client_data_json),
            "signature": b64url(&self.signature),
        })
    }

    /// The canonical bytes of [`Signed::wire`]: its four text members in byte order of their
    /// names, written out here (journal spec §4).
    pub fn canonical(&self, passkey_id: &str) -> Vec<u8> {
        format!(
            r#"{{"authenticator_data":"{}","client_data_json":"{}","passkey_id":"{passkey_id}","signature":"{}"}}"#,
            b64url(&self.authenticator_data),
            b64url(&self.client_data_json),
            b64url(&self.signature),
        )
        .into_bytes()
    }

    /// The assertion `mandate-passkey` verifies, naming `passkey_id`'s bytes.
    pub fn assertion<'a>(&'a self, credential_id: &'a [u8]) -> Assertion<'a> {
        Assertion {
            credential_id,
            authenticator_data: &self.authenticator_data,
            client_data_json: &self.client_data_json,
            signature: &self.signature,
        }
    }
}

/// A software authenticator holding one key.
pub enum Signer {
    Es256(EcdsaKeyPair),
    Ed25519(Ed25519KeyPair),
}

impl Signer {
    pub fn es256() -> Self {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng)
            .expect("a P-256 key");
        Self::Es256(
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
                .expect("the key reads"),
        )
    }

    pub fn ed25519() -> Self {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).expect("an Ed25519 key");
        Self::Ed25519(Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("the key reads"))
    }

    /// §9.31's name for the key's algorithm.
    pub fn alg(&self) -> &'static str {
        match self {
            Self::Es256(_) => "es256",
            Self::Ed25519(_) => "eddsa",
        }
    }

    /// The key as a COSE_Key (RFC 9053), the bytes an enrolment stores as an artifact.
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

    /// The credential an auditor re-verifies against: the key, no counter, no backup.
    pub fn credential(&self, passkey_id: &str) -> Credential {
        let public_key = match self {
            Self::Es256(key) => {
                let point = key.public_key().as_ref();
                PublicKey::Es256 {
                    x: point[1..33].try_into().expect("32 bytes"),
                    y: point[33..65].try_into().expect("32 bytes"),
                }
            }
            Self::Ed25519(key) => PublicKey::Ed25519 {
                x: key.public_key().as_ref().try_into().expect("32 bytes"),
            },
        };
        Credential {
            id: unb64url(passkey_id),
            public_key,
            sign_count: 0,
            backup_eligible: false,
        }
    }

    fn sign(&self, message: &[u8]) -> Vec<u8> {
        match self {
            Self::Es256(key) => key
                .sign(&SystemRandom::new(), message)
                .expect("signs")
                .as_ref()
                .to_vec(),
            Self::Ed25519(key) => key.sign(message).as_ref().to_vec(),
        }
    }

    /// An assertion over `challenge` for [`rp`], carrying `counter`, from an authenticator that is
    /// not backup-eligible.
    pub fn assert(&self, challenge: &[u8], counter: u32) -> Signed {
        self.assert_flagged(challenge, counter, UP_UV)
    }

    /// [`Signer::assert`] from a backup-eligible authenticator, not backed up.
    pub fn assert_backup_eligible(&self, challenge: &[u8], counter: u32) -> Signed {
        self.assert_flagged(challenge, counter, UP_UV | BE)
    }

    fn assert_flagged(&self, challenge: &[u8], counter: u32, flags: u8) -> Signed {
        let authenticator_data = [
            sha256(RP_ID.as_bytes()),
            vec![flags],
            counter.to_be_bytes().to_vec(),
        ]
        .concat();
        let client_data_json = format!(
            r#"{{"type":"webauthn.get","challenge":"{}","origin":"{ORIGIN}","crossOrigin":false}}"#,
            b64url(challenge)
        )
        .into_bytes();
        let signature =
            self.sign(&[authenticator_data.clone(), sha256(&client_data_json)].concat());
        Signed {
            authenticator_data,
            client_data_json,
            signature,
        }
    }
}

/// A registration response as a browser sends it (DEC-2108 item 4): the attestation object and the
/// client data, each the bytes before base64url.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    pub attestation_object: Vec<u8>,
    pub client_data_json: Vec<u8>,
}

/// What a registration response carries that a refusal case changes; [`Shape::good`] is the one
/// `mandate_passkey::enrol` accepts.
#[derive(Debug, Clone)]
pub struct Shape {
    pub ceremony: &'static str,
    pub origin: String,
    pub rp_id: String,
    pub flags: u8,
    pub fmt: &'static str,
    pub counter: u32,
}

impl Shape {
    /// A `webauthn.create` for [`rp`], user present and verified, attested, attestation `none`.
    pub fn good() -> Self {
        Self {
            ceremony: "webauthn.create",
            origin: ORIGIN.to_owned(),
            rp_id: RP_ID.to_owned(),
            flags: UP_UV | AT,
            fmt: "none",
            counter: 0,
        }
    }

    /// [`Shape::good`] from a backup-eligible authenticator.
    pub fn backup_eligible() -> Self {
        Self {
            flags: UP_UV | AT | BE,
            ..Self::good()
        }
    }
}

/// The attested-credential-data flag (WebAuthn's AT bit).
pub const AT: u8 = 0x40;

/// A CBOR head of `major` type with `len`, written out here (RFC 8949 §3).
fn cbor_head(major: u8, len: usize) -> Vec<u8> {
    let top = major << 5;
    match u16::try_from(len) {
        Ok(small) if small < 24 => vec![top | u8::try_from(small).expect("under 24")],
        Ok(small) if small < 256 => vec![top | 24, u8::try_from(small).expect("one byte")],
        Ok(wide) => [vec![top | 25], wide.to_be_bytes().to_vec()].concat(),
        Err(_) => panic!("a test registration fits in 65,535 bytes"),
    }
}

fn cbor_text(text: &str) -> Vec<u8> {
    [cbor_head(3, text.len()), text.as_bytes().to_vec()].concat()
}

fn cbor_bytes(bytes: &[u8]) -> Vec<u8> {
    [cbor_head(2, bytes.len()), bytes.to_vec()].concat()
}

impl Signer {
    /// A registration of this key as `passkey_id` over `challenge`, shaped as [`Shape::good`].
    pub fn register(&self, challenge: &[u8], passkey_id: &str) -> Registered {
        self.register_shaped(challenge, passkey_id, &Shape::good())
    }

    /// A registration of this key as `passkey_id` over `challenge`, shaped as `shape`: the
    /// attested authenticator data (RP ID hash, flags, counter, a zero AAGUID, the credential id
    /// and its length, the COSE key) in a CBOR map of `fmt`, an empty `attStmt`, and `authData`.
    pub fn register_shaped(&self, challenge: &[u8], passkey_id: &str, shape: &Shape) -> Registered {
        let credential = unb64url(passkey_id);
        let auth_data = [
            sha256(shape.rp_id.as_bytes()),
            vec![shape.flags],
            shape.counter.to_be_bytes().to_vec(),
            vec![0; 16],
            u16::try_from(credential.len())
                .expect("a short id")
                .to_be_bytes()
                .to_vec(),
            credential,
            self.cose_key(),
        ]
        .concat();
        let attestation_object = [
            vec![0xa3],
            cbor_text("fmt"),
            cbor_text(shape.fmt),
            cbor_text("attStmt"),
            vec![0xa0],
            cbor_text("authData"),
            cbor_bytes(&auth_data),
        ]
        .concat();
        let client_data_json = format!(
            r#"{{"type":"{}","challenge":"{}","origin":"{}","crossOrigin":false}}"#,
            shape.ceremony,
            b64url(challenge),
            shape.origin
        )
        .into_bytes();
        Registered {
            attestation_object,
            client_data_json,
        }
    }
}

/// A `CredentialEnrolled` version 2 payload (journal spec §9.31, DEC-2122 item 2) for `member`'s
/// `credential` with `key`'s COSE bytes, its counter at enrolment, its cool-off end, or none, and
/// a registration that was not backup-eligible.
pub fn enrolled(
    member: PrincipalId,
    credential: &str,
    signer: &Signer,
    sign_count: u32,
    cool_off_ends: Option<i64>,
) -> Json {
    json!({
        "member": encode_ulid(member.0), "kind": "passkey", "credential": credential,
        "public_key": format!("sha256:{}", Digest::of(&signer.cose_key())), "alg": signer.alg(),
        "sign_count": sign_count, "backup_eligible": false,
        "cool_off_ends": cool_off_ends.map(|secs| at(secs).to_string()), "step_up": null,
    })
}

/// `payload` from a backup-eligible registration.
pub fn backup_eligible(mut payload: Json) -> Json {
    payload["backup_eligible"] = json!(true);
    payload
}

/// `payload` as version 1 writes it, with no backup-eligible flag (DEC-2122 item 3).
pub fn version_1(mut payload: Json) -> Json {
    if let Some(members) = payload.as_object_mut() {
        members.remove("backup_eligible");
    }
    payload
}

/// Appends a `CredentialEnrolled` of `payload` at `version` to `rows`, at `secs`.
pub fn enrol_at(rows: &mut Rows, version: u64, secs: i64, payload: Json) {
    rows.edited("CredentialEnrolled", secs, |envelope| {
        envelope["payload"] = payload;
        envelope["schema_version"] = json!(version);
    });
}

/// Appends a `CredentialEnrolled` version 2 of `payload` to `rows`, at `secs`.
pub fn enrol(rows: &mut Rows, secs: i64, payload: Json) {
    enrol_at(rows, 2, secs, payload);
}

/// A `CredentialRemoved` payload for `member`'s `credential`.
pub fn removed(member: PrincipalId, credential: &str) -> Json {
    json!({
        "member": encode_ulid(member.0), "credential": credential,
        "step_up": {"assertion_id": "01JAA3P4A000000000000000S1", "authenticated_at": "2026-10-19T15:00:05.250000000Z", "method": "passkey"},
    })
}

/// Appends a `StepUpAsserted` of `payload` to `rows`, at `secs`, written by `member`, the actor its
/// credential's holder is read from (journal spec §9.32).
pub fn assert_by(rows: &mut Rows, member: PrincipalId, secs: i64, payload: Json) {
    rows.edited("StepUpAsserted", secs, |envelope| {
        envelope["payload"] = payload;
        envelope["actor"] = json!({"kind": "user", "id": encode_ulid(member.0)});
    });
}

/// A `StepUpAsserted` payload (journal spec §9.32) for `credential` at `sign_count`.
pub fn asserted(challenge: &str, credential: &str, sign_count: u32) -> Json {
    json!({
        "challenge": challenge, "credential": credential, "sign_count": sign_count,
        "assertion": format!("sha256:{}", Digest::of(challenge.as_bytes())),
    })
}
