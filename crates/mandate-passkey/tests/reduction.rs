//! Route 2's verifier (workspace API spec §4.10 route 2 items 2 and 4, DEC-2324): every call makes
//! exactly one ES256 and one Ed25519 check, whether a key is held, of either algorithm, or none, so
//! a held passkey and an unknown one do the same work; the signature's result is the answer
//! whenever it is bad, whatever else is wrong; and no key never verifies.

mod common;

use common::{
    AT, Alg, Authenticator, BE, BS, Ceremony, OwnedAssertion, UP, UV, cbor_int, cbor_map,
    challenge, rp,
};
use std::cell::RefCell;

use mandate_passkey::reduction::{
    DECOY_ED25519, DECOY_ES256, RingCheck, SignatureCheck, cose_key, verify_reduction,
    verify_reduction_with,
};
use mandate_passkey::{PublicKey, Refusal};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, Ed25519KeyPair};

const CHALLENGE: [u8; 32] = [0xfb; 32];

fn judged(key: Option<&PublicKey>, assertion: &OwnedAssertion) -> Result<(), Refusal> {
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
                judged(Some(&authenticator.public_key()), &assertion),
                Ok(()),
                "{alg:?}, flags {flags:#04x}, counter {counter}: no id, backup flag, or counter compared"
            );
        }
    }
}

#[test]
fn a_bad_signature_is_the_answer_whatever_else_is_wrong_held_key_or_none() {
    for alg in [Alg::Es256, Alg::Ed25519] {
        let held = Authenticator::new(alg);
        let stranger = Authenticator::new(alg);
        for (name, change, refusal) in malformed() {
            let mut ceremony = Ceremony::get(&CHALLENGE, 5);
            change(&mut ceremony);
            let signed = held.assert(&ceremony);
            assert_eq!(
                judged(Some(&held.public_key()), &signed),
                Err(refusal),
                "{alg:?}, {name}, signed well"
            );
            for key in [Some(&stranger.public_key()), None] {
                assert_eq!(
                    judged(key, &signed),
                    Err(Refusal::SignatureInvalid),
                    "{alg:?}, {name}: the signature's result first, for any other key"
                );
            }
            let mut forged = signed.clone();
            forged.signature[10] ^= 1;
            assert_eq!(
                judged(Some(&held.public_key()), &forged),
                Err(Refusal::SignatureInvalid),
                "{alg:?}, {name}: a bad signature over a held key"
            );
        }
        let mut garbled = held.assert(&Ceremony::get(&CHALLENGE, 5));
        garbled.authenticator_data.truncate(20);
        garbled.client_data_json = b"{".to_vec();
        for key in [Some(&held.public_key()), None] {
            assert_eq!(
                judged(key, &garbled),
                Err(Refusal::SignatureInvalid),
                "{alg:?}: unparsable data is still signed over, and the signature answers first"
            );
        }
        garbled.signature = held.sign_response(&garbled.authenticator_data, b"{");
        assert_eq!(
            judged(Some(&held.public_key()), &garbled),
            Err(Refusal::ClientDataMalformed),
            "{alg:?}: signed well, the client data is judged next"
        );
        garbled.client_data_json = Ceremony::get(&CHALLENGE, 5).client_data_json();
        garbled.signature =
            held.sign_response(&garbled.authenticator_data, &garbled.client_data_json);
        assert_eq!(
            judged(Some(&held.public_key()), &garbled),
            Err(Refusal::AuthenticatorDataMalformed),
            "{alg:?}: then the authenticator data"
        );
    }
}

/// A [`SignatureCheck`] that records the key of every check it makes, then makes it through
/// [`RingCheck`].
#[derive(Default)]
struct Counting(RefCell<Vec<PublicKey>>);

impl SignatureCheck for Counting {
    fn check(&self, key: &PublicKey, message: &[u8], signature: &[u8]) -> Result<(), Refusal> {
        self.0.borrow_mut().push(key.clone());
        RingCheck.check(key, message, signature)
    }
}

/// A DER `SEQUENCE` of two `INTEGER`s of `r` and `s` bytes, as an ECDSA signature is written.
fn der(r: usize, s: usize) -> Vec<u8> {
    let int = |n: usize| [vec![0x02, n as u8], vec![0x11; n]].concat();
    let body = [int(r), int(s)].concat();
    [vec![0x30, body.len() as u8], body].concat()
}

#[test]
fn every_call_checks_once_with_es256_then_once_with_ed25519_whatever_is_held() {
    let es256 = Authenticator::new(Alg::Es256);
    let ed25519 = Authenticator::new(Alg::Ed25519);
    let assertion = es256.assert(&Ceremony::get(&CHALLENGE, 5));
    let shapes: Vec<(&str, Vec<u8>)> = vec![
        ("an ECDSA signature", assertion.signature.clone()),
        (
            "an Ed25519 signature",
            ed25519.assert(&Ceremony::get(&CHALLENGE, 5)).signature,
        ),
        ("64 bytes of DER", der(29, 29)),
        ("64 raw bytes", vec![0x5a; 64]),
        ("8 bytes", vec![0x5a; 8]),
        ("none", Vec::new()),
    ];
    let held: [(&str, Option<PublicKey>); 3] = [
        ("a held ES256 key", Some(es256.public_key())),
        ("a held Ed25519 key", Some(ed25519.public_key())),
        ("no key", None),
    ];
    for (shape, signature) in &shapes {
        for (label, key) in &held {
            let counting = Counting::default();
            let mut presented = assertion.clone();
            presented.signature = signature.clone();
            let _answer = verify_reduction_with(
                &counting,
                &rp(),
                &challenge(&CHALLENGE),
                key.as_ref(),
                &presented.view(),
            );
            let expected = match key {
                Some(own @ PublicKey::Es256 { .. }) => vec![own.clone(), DECOY_ED25519],
                Some(own @ PublicKey::Ed25519 { .. }) => vec![DECOY_ES256, own.clone()],
                None => vec![DECOY_ES256, DECOY_ED25519],
            };
            assert_eq!(
                counting.0.into_inner(),
                expected,
                "{label}, {shape}: one ES256 then one Ed25519 check, the held key in its own"
            );
        }
    }
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
fn each_decoy_is_a_valid_key_yet_with_no_key_held_nothing_verifies() {
    let (p256, ed25519) = decoy_signers();
    let ceremony = Ceremony::get(&CHALLENGE, 1);
    let authenticator_data = ceremony.auth_data_head();
    let client_data_json = ceremony.client_data_json();
    let message = [
        authenticator_data.clone(),
        common::sha256(&client_data_json),
    ]
    .concat();
    let signed = [
        (
            DECOY_ES256,
            p256.sign(&SystemRandom::new(), &message)
                .expect("sign")
                .as_ref()
                .to_vec(),
        ),
        (DECOY_ED25519, ed25519.sign(&message).as_ref().to_vec()),
    ];
    for (decoy, signature) in signed {
        assert_eq!(
            RingCheck.check(&decoy, &message, &signature),
            Ok(()),
            "its published secret signs for it, so the key parses and its check runs to the end"
        );
        let assertion = OwnedAssertion {
            credential_id: vec![1; 16],
            authenticator_data: authenticator_data.clone(),
            client_data_json: client_data_json.clone(),
            signature,
        };
        assert_eq!(
            judged(None, &assertion),
            Err(Refusal::SignatureInvalid),
            "a decoy's check never answers"
        );
        assert_eq!(
            judged(Some(&decoy), &assertion),
            Ok(()),
            "as a held key it would"
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
