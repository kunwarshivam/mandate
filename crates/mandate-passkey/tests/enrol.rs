//! Enrolment ([`enrol`]): identity spec §6.1 (attestation not required in v1, user verification
//! required) and the strict readings of DEC-660. Each refusal test changes one field of a response
//! every check passes.

mod common;

use common::{
    AT, Alg, Authenticator, BE, BS, Ceremony, ED, OwnedRegistration, UP, UV, attestation_object,
    cbor_bytes, cbor_int, cbor_map, cbor_text, challenge, rp,
};
use mandate_passkey::{Credential, Refusal, enrol};

const CHALLENGE: [u8; 32] = [0x5a; 32];

fn enrol_with(registration: &OwnedRegistration) -> Result<Credential, Refusal> {
    enrol(&rp(), &challenge(&CHALLENGE), &registration.view())
}

fn refused(change: impl FnOnce(&mut Ceremony)) -> Result<Credential, Refusal> {
    let mut ceremony = Ceremony::create(&CHALLENGE);
    change(&mut ceremony);
    enrol_with(&Authenticator::new(Alg::Es256).register(&ceremony))
}

#[test]
#[ignore = "pending E9-1"]
fn an_es256_passkey_enrols_with_none_attestation() {
    let authenticator = Authenticator::new(Alg::Es256);
    let registration = authenticator.register(&Ceremony::create(&CHALLENGE));
    assert_eq!(enrol_with(&registration), Ok(authenticator.credential(0)));
}

#[test]
#[ignore = "pending E9-1"]
fn an_ed25519_passkey_enrols_with_its_counter_and_backup_eligibility() {
    let authenticator = Authenticator::new(Alg::Ed25519);
    let mut ceremony = Ceremony::create(&CHALLENGE);
    ceremony.counter = 7;
    ceremony.flags |= BE | BS;
    let expected = Credential {
        backup_eligible: true,
        ..authenticator.credential(7)
    };
    assert_eq!(enrol_with(&authenticator.register(&ceremony)), Ok(expected));
}

#[test]
#[ignore = "pending E9-1"]
fn a_get_ceremony_is_not_an_enrolment() {
    let result = refused(|c| c.kind = "webauthn.get".to_owned());
    assert_eq!(result, Err(Refusal::ClientDataType));
}

#[test]
#[ignore = "pending E9-1"]
fn another_challenge_is_refused() {
    let result = refused(|c| c.challenge = common::b64url(&[0x5b; 32]));
    assert_eq!(result, Err(Refusal::ChallengeMismatch));
}

#[test]
#[ignore = "pending E9-1"]
fn an_origin_that_only_shares_the_prefix_is_refused() {
    for origin in [
        "https://app.owlhead.ai.evil.example",
        "https://app.owlhead.ai:8443",
        "https://app.owlhead.ai/",
        "http://app.owlhead.ai",
        "https://api.owlhead.ai",
        "https://evil.owlhead.ai",
        "https://owlhead.ai",
    ] {
        let result = refused(|c| c.origin = origin.to_owned());
        assert_eq!(result, Err(Refusal::OriginMismatch), "{origin}");
    }
}

#[test]
#[ignore = "pending E9-1"]
fn a_cross_origin_ceremony_is_refused() {
    assert_eq!(
        refused(|c| c.cross_origin = Some(true)),
        Err(Refusal::CrossOrigin)
    );
}

#[test]
#[ignore = "pending E9-1"]
fn another_rp_id_hash_is_refused() {
    for rp_id in ["owlhead.ai", "api.owlhead.ai", "evil.example"] {
        let result = refused(|c| c.rp_id = rp_id.to_owned());
        assert_eq!(result, Err(Refusal::RpIdMismatch), "{rp_id}");
    }
}

#[test]
#[ignore = "pending E9-1"]
fn enrolment_without_user_presence_or_verification_is_refused() {
    assert_eq!(refused(|c| c.flags &= !UP), Err(Refusal::UserNotPresent));
    assert_eq!(refused(|c| c.flags &= !UV), Err(Refusal::UserNotVerified));
}

#[test]
#[ignore = "pending E9-1"]
fn backup_state_without_backup_eligibility_is_refused() {
    assert_eq!(refused(|c| c.flags |= BS), Err(Refusal::BackupFlags));
}

#[test]
#[ignore = "pending E9-1"]
fn authenticator_data_without_a_credential_is_refused() {
    let result = refused(|c| c.flags &= !AT);
    assert_eq!(result, Err(Refusal::AuthenticatorDataMalformed));
}

#[test]
#[ignore = "pending E9-1"]
fn bytes_after_the_public_key_need_the_extension_flag_and_a_map() {
    let authenticator = Authenticator::new(Alg::Es256);
    let key = authenticator.cose_key();
    let register = |flags: u8, tail: &[u8]| {
        let mut ceremony = Ceremony::create(&CHALLENGE);
        ceremony.flags |= flags;
        let auth_data =
            authenticator.attested_auth_data(&ceremony, &[key.clone(), tail.to_vec()].concat());
        OwnedRegistration {
            attestation_object: attestation_object("none", &cbor_map(&[]), &auth_data),
            client_data_json: ceremony.client_data_json(),
        }
    };
    let extensions = cbor_map(&[(cbor_text("credProtect"), cbor_int(2))]);
    assert_eq!(
        enrol_with(&register(0, &extensions)),
        Err(Refusal::AuthenticatorDataMalformed)
    );
    assert_eq!(
        enrol_with(&register(ED, &[])),
        Err(Refusal::AuthenticatorDataMalformed)
    );
    assert_eq!(
        enrol_with(&register(ED, &cbor_int(2))),
        Err(Refusal::AuthenticatorDataMalformed)
    );
    assert_eq!(
        enrol_with(&register(ED, &extensions)),
        Ok(authenticator.credential(0))
    );
}

#[test]
#[ignore = "pending E9-1"]
fn only_none_attestation_with_an_empty_statement_is_accepted() {
    let authenticator = Authenticator::new(Alg::Es256);
    let ceremony = Ceremony::create(&CHALLENGE);
    let auth_data = authenticator.attested_auth_data(&ceremony, &authenticator.cose_key());
    let with = |fmt: &str, att_stmt: Vec<u8>| OwnedRegistration {
        attestation_object: attestation_object(fmt, &att_stmt, &auth_data),
        client_data_json: ceremony.client_data_json(),
    };
    let signed = cbor_map(&[
        (cbor_text("alg"), cbor_int(-7)),
        (cbor_text("sig"), cbor_bytes(&[1; 70])),
    ]);
    assert_eq!(
        enrol_with(&with("packed", signed.clone())),
        Err(Refusal::AttestationFormat)
    );
    assert_eq!(
        enrol_with(&with("none", signed)),
        Err(Refusal::AttestationFormat)
    );
}

#[test]
#[ignore = "pending E9-1"]
fn malformed_cbor_is_refused() {
    let authenticator = Authenticator::new(Alg::Es256);
    let ceremony = Ceremony::create(&CHALLENGE);
    let auth_data = authenticator.attested_auth_data(&ceremony, &authenticator.cose_key());
    let good = attestation_object("none", &cbor_map(&[]), &auth_data);
    let duplicate_key = cbor_map(&[
        (cbor_text("fmt"), cbor_text("none")),
        (cbor_text("fmt"), cbor_text("none")),
        (cbor_text("attStmt"), cbor_map(&[])),
        (cbor_text("authData"), cbor_bytes(&auth_data)),
    ]);
    let indefinite = [vec![0xbf], good[1..].to_vec(), vec![0xff]].concat();
    let truncated = good[..good.len() - 1].to_vec();
    let trailing = [good.clone(), vec![0]].concat();
    for (name, object) in [
        ("duplicate key", duplicate_key),
        ("indefinite map", indefinite),
        ("truncated", truncated),
        ("trailing byte", trailing),
    ] {
        let registration = OwnedRegistration {
            attestation_object: object,
            client_data_json: ceremony.client_data_json(),
        };
        assert_eq!(
            enrol_with(&registration),
            Err(Refusal::AttestationMalformed),
            "{name}"
        );
    }
}

#[test]
#[ignore = "pending E9-1"]
fn only_es256_on_p256_and_eddsa_on_ed25519_keys_are_accepted() {
    let authenticator = Authenticator::new(Alg::Es256);
    let PublicKeyParts { x, y } = PublicKeyParts::of(&authenticator);
    let cases = [
        (
            cbor_map(&[
                (cbor_int(1), cbor_int(3)),
                (cbor_int(3), cbor_int(-257)),
                (cbor_int(-1), cbor_bytes(&[1; 256])),
                (cbor_int(-2), cbor_bytes(&[1, 0, 1])),
            ]),
            Refusal::AlgorithmUnsupported,
        ),
        (
            cbor_map(&[
                (cbor_int(1), cbor_int(2)),
                (cbor_int(3), cbor_int(-7)),
                (cbor_int(-1), cbor_int(2)),
                (cbor_int(-2), cbor_bytes(&x)),
                (cbor_int(-3), cbor_bytes(&y)),
            ]),
            Refusal::PublicKeyMalformed,
        ),
        (
            cbor_map(&[
                (cbor_int(1), cbor_int(2)),
                (cbor_int(3), cbor_int(-7)),
                (cbor_int(-1), cbor_int(1)),
                (cbor_int(-2), cbor_bytes(&x[1..])),
            ]),
            Refusal::PublicKeyMalformed,
        ),
        (
            cbor_map(&[
                (cbor_int(1), cbor_int(1)),
                (cbor_int(3), cbor_int(-7)),
                (cbor_int(-1), cbor_int(1)),
                (cbor_int(-2), cbor_bytes(&x)),
                (cbor_int(-3), cbor_bytes(&y)),
            ]),
            Refusal::PublicKeyMalformed,
        ),
    ];
    let ceremony = Ceremony::create(&CHALLENGE);
    for (n, (key, refusal)) in cases.into_iter().enumerate() {
        let auth_data = authenticator.attested_auth_data(&ceremony, &key);
        let registration = OwnedRegistration {
            attestation_object: attestation_object("none", &cbor_map(&[]), &auth_data),
            client_data_json: ceremony.client_data_json(),
        };
        assert_eq!(enrol_with(&registration), Err(refusal), "case {n}");
    }
}

struct PublicKeyParts {
    x: Vec<u8>,
    y: Vec<u8>,
}

impl PublicKeyParts {
    fn of(authenticator: &Authenticator) -> Self {
        match authenticator.public_key() {
            mandate_passkey::PublicKey::Es256 { x, y } => Self {
                x: x.to_vec(),
                y: y.to_vec(),
            },
            mandate_passkey::PublicKey::Ed25519 { x } => Self {
                x: x.to_vec(),
                y: Vec::new(),
            },
        }
    }
}
