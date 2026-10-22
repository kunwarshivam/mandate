//! Route 2's verifier (workspace API spec §4.10 route 2 items 2 and 4, DEC-2324): a known passkey
//! and an unknown one, checked against a decoy, take the same path up to the signature's result,
//! and that result is the answer whenever the signature is bad, whatever else is wrong.

mod common;

use common::{
    AT, Alg, Authenticator, BE, BS, Ceremony, OwnedAssertion, UP, UV, cbor_int, cbor_map,
    challenge, rp,
};
use mandate_passkey::reduction::{
    DECOY_ED25519, DECOY_ES256, cose_key, decoy_for, verify_reduction,
};
use mandate_passkey::{PublicKey, Refusal};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, Ed25519KeyPair};

const CHALLENGE: [u8; 32] = [0xfb; 32];

fn judged(key: &PublicKey, assertion: &OwnedAssertion) -> Result<(), Refusal> {
    verify_reduction(&rp(), &challenge(&CHALLENGE), key, &assertion.view())
}

/// One way an assertion can be wrong besides its signature: its name, the change, and the refusal
/// it gets once signed well.
type Malformation = (&'static str, Box<dyn Fn(&mut Ceremony)>, Refusal);

/// Each way an assertion can be wrong besides its signature.
fn malformed() -> Vec<Malformation> {
    vec![
        (
            "another challenge",
            Box::new(|c: &mut Ceremony| c.challenge = "AAAA".to_owned()),
            Refusal::ChallengeMismatch,
        ),
        (
            "another origin",
            Box::new(|c: &mut Ceremony| c.origin = "https://evil.example".to_owned()),
            Refusal::OriginMismatch,
        ),
        (
            "a create ceremony",
            Box::new(|c: &mut Ceremony| c.kind = "webauthn.create".to_owned()),
            Refusal::ClientDataType,
        ),
        (
            "a cross-origin frame",
            Box::new(|c: &mut Ceremony| c.cross_origin = Some(true)),
            Refusal::CrossOrigin,
        ),
        (
            "another RP ID",
            Box::new(|c: &mut Ceremony| c.rp_id = "evil.example".to_owned()),
            Refusal::RpIdMismatch,
        ),
        (
            "no user presence",
            Box::new(|c: &mut Ceremony| c.flags = UV),
            Refusal::UserNotPresent,
        ),
        (
            "no user verification",
            Box::new(|c: &mut Ceremony| c.flags = UP),
            Refusal::UserNotVerified,
        ),
        (
            "backed up but not eligible",
            Box::new(|c: &mut Ceremony| c.flags = UP | UV | BS),
            Refusal::BackupFlags,
        ),
        (
            "attested credential data claimed",
            Box::new(|c: &mut Ceremony| c.flags = UP | UV | AT),
            Refusal::AuthenticatorDataMalformed,
        ),
    ]
}

#[test]
fn a_held_key_verifies_whatever_id_flag_or_counter_the_assertion_carries() {
    for alg in [Alg::Es256, Alg::Ed25519] {
        let authenticator = Authenticator::new(alg);
        for (flags, counter) in [(UP | UV, 0), (UP | UV | BE, 7), (UP | UV | BE | BS, 1)] {
            let mut ceremony = Ceremony::get(&CHALLENGE, counter);
            ceremony.flags = flags;
            let mut assertion = authenticator.assert(&ceremony);
            assertion.credential_id = vec![0x5a; 40];
            assert_eq!(
                judged(&authenticator.public_key(), &assertion),
                Ok(()),
                "{alg:?}, flags {flags:#04x}, counter {counter}: no id, backup flag, or counter compared"
            );
        }
    }
}

#[test]
fn a_bad_signature_is_the_answer_whatever_else_is_wrong_for_a_held_key_or_a_decoy() {
    for alg in [Alg::Es256, Alg::Ed25519] {
        let held = Authenticator::new(alg);
        let stranger = Authenticator::new(alg);
        for (name, change, refusal) in malformed() {
            let mut ceremony = Ceremony::get(&CHALLENGE, 5);
            change(&mut ceremony);
            let signed = held.assert(&ceremony);
            assert_eq!(
                judged(&held.public_key(), &signed),
                Err(refusal),
                "{alg:?}, {name}, signed well"
            );
            let decoy = decoy_for(&signed.signature);
            for key in [&stranger.public_key(), decoy] {
                assert_eq!(
                    judged(key, &signed),
                    Err(Refusal::SignatureInvalid),
                    "{alg:?}, {name}: the signature's result first, for any other key"
                );
            }
            let mut forged = signed.clone();
            forged.signature[10] ^= 1;
            assert_eq!(
                judged(&held.public_key(), &forged),
                Err(Refusal::SignatureInvalid),
                "{alg:?}, {name}: a bad signature over a held key"
            );
        }
        let mut garbled = held.assert(&Ceremony::get(&CHALLENGE, 5));
        garbled.authenticator_data.truncate(20);
        garbled.client_data_json = b"{".to_vec();
        for key in [&held.public_key(), decoy_for(&garbled.signature)] {
            assert_eq!(
                judged(key, &garbled),
                Err(Refusal::SignatureInvalid),
                "{alg:?}: unparsable data is still signed over, and the signature answers first"
            );
        }
        garbled.signature = held.sign_response(&garbled.authenticator_data, b"{");
        assert_eq!(
            judged(&held.public_key(), &garbled),
            Err(Refusal::ClientDataMalformed),
            "{alg:?}: signed well, the client data is judged next"
        );
        garbled.client_data_json = Ceremony::get(&CHALLENGE, 5).client_data_json();
        garbled.signature =
            held.sign_response(&garbled.authenticator_data, &garbled.client_data_json);
        assert_eq!(
            judged(&held.public_key(), &garbled),
            Err(Refusal::AuthenticatorDataMalformed),
            "{alg:?}: then the authenticator data"
        );
    }
}

/// A DER `SEQUENCE` of two `INTEGER`s of `r` and `s` bytes, as an ECDSA signature is written.
fn der(r: usize, s: usize) -> Vec<u8> {
    let int = |n: usize| [vec![0x02, n as u8], vec![0x11; n]].concat();
    let body = [int(r), int(s)].concat();
    [vec![0x30, body.len() as u8], body].concat()
}

#[test]
fn the_decoy_is_chosen_by_the_signatures_encoding_der_first() {
    let es256 = Authenticator::new(Alg::Es256);
    let signature = es256.sign(b"anything");
    assert_eq!(decoy_for(&signature), &DECOY_ES256, "an ECDSA signature");
    assert_eq!(der(29, 29).len(), 64);
    assert_eq!(decoy_for(&der(29, 29)), &DECOY_ES256, "64 bytes of DER");
    for (r, s) in [(32, 32), (33, 33), (1, 1), (33, 31)] {
        assert_eq!(decoy_for(&der(r, s)), &DECOY_ES256, "DER of {r} and {s}");
    }
    let mut raw = vec![0x30; 64];
    raw[1] = 0x3e;
    assert_eq!(
        decoy_for(&raw),
        &DECOY_ED25519,
        "64 bytes that open like DER but are not"
    );
    assert_eq!(decoy_for(&[0x5a; 64]), &DECOY_ED25519, "64 raw bytes");
    for len in [0, 1, 8, 63, 65, 72, 128] {
        assert_eq!(
            decoy_for(&vec![0x5a; len]),
            &DECOY_ES256,
            "{len} raw bytes are any other shape"
        );
    }
    let mut trailing = der(29, 28);
    trailing.push(0);
    assert_eq!(trailing.len(), 64);
    assert_eq!(
        decoy_for(&trailing),
        &DECOY_ED25519,
        "64 bytes of DER with a trailing byte are not DER"
    );
    let mut past_the_end = der(29, 29);
    past_the_end[3] = 70;
    assert_eq!(
        decoy_for(&past_the_end),
        &DECOY_ED25519,
        "64 bytes whose inner length runs past the end are not DER"
    );
    let mut not_an_integer = der(29, 29);
    not_an_integer[33] = 0x04;
    assert_eq!(
        decoy_for(&not_an_integer),
        &DECOY_ED25519,
        "64 bytes whose second member is not an INTEGER are not DER"
    );
}

/// Ed25519 and P-256 signers for the two decoys, from their published secrets: RFC 8032 §7.1's
/// first test key, and the scalar 1, whose public key is the base point.
fn decoy_signers() -> (EcdsaKeyPair, Ed25519KeyPair) {
    let PublicKey::Es256 { x, y } = DECOY_ES256 else {
        panic!("an ES256 decoy");
    };
    let mut one = [0; 32];
    one[31] = 1;
    let point = [&[0x04][..], &x, &y].concat();
    let p256 = EcdsaKeyPair::from_private_key_and_public_key(
        &ECDSA_P256_SHA256_ASN1_SIGNING,
        &one,
        &point,
        &SystemRandom::new(),
    )
    .expect("the scalar 1 and the base point");
    let PublicKey::Ed25519 { x } = DECOY_ED25519 else {
        panic!("an Ed25519 decoy");
    };
    let seed = [
        0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c,
        0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae,
        0x7f, 0x60,
    ];
    let ed25519 =
        Ed25519KeyPair::from_seed_and_public_key(&seed, &x).expect("RFC 8032's first test key");
    (p256, ed25519)
}

#[test]
fn each_decoy_is_a_valid_key_so_its_check_is_a_full_verification() {
    let (p256, ed25519) = decoy_signers();
    let ceremony = Ceremony::get(&CHALLENGE, 1);
    let authenticator_data = ceremony.auth_data_head();
    let client_data_json = ceremony.client_data_json();
    let message = [
        authenticator_data.clone(),
        common::sha256(&client_data_json),
    ]
    .concat();
    let signatures = [
        p256.sign(&SystemRandom::new(), &message)
            .expect("sign")
            .as_ref()
            .to_vec(),
        ed25519.sign(&message).as_ref().to_vec(),
    ];
    for signature in signatures {
        let decoy = decoy_for(&signature);
        let assertion = OwnedAssertion {
            credential_id: vec![1; 16],
            authenticator_data: authenticator_data.clone(),
            client_data_json: client_data_json.clone(),
            signature,
        };
        assert_eq!(
            judged(decoy, &assertion),
            Ok(()),
            "a signature its published secret makes verifies, so the key parsed and the check ran to the end"
        );
    }
}

#[test]
fn a_cose_key_reads_as_its_algorithms_key_and_nothing_else_does() {
    for alg in [Alg::Es256, Alg::Ed25519] {
        let authenticator = Authenticator::new(alg);
        assert_eq!(
            cose_key(&authenticator.cose_key()),
            Ok(authenticator.public_key()),
            "{alg:?}"
        );
        let mut trailing = authenticator.cose_key();
        trailing.push(0);
        assert_eq!(
            cose_key(&trailing),
            Err(Refusal::PublicKeyMalformed),
            "{alg:?}: nothing after the key"
        );
        let cut = authenticator.cose_key();
        assert_eq!(
            cose_key(&cut[..cut.len() - 1]),
            Err(Refusal::PublicKeyMalformed),
            "{alg:?}: a key cut short"
        );
    }
    assert_eq!(cose_key(b""), Err(Refusal::PublicKeyMalformed));
    assert_eq!(cose_key(&cbor_int(7)), Err(Refusal::PublicKeyMalformed));
    let rs256 = cbor_map(&[(cbor_int(1), cbor_int(3)), (cbor_int(3), cbor_int(-257))]);
    assert_eq!(cose_key(&rs256), Err(Refusal::AlgorithmUnsupported));
}
