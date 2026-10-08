//! The byte-level structure [`enrol`] reads (DEC-660 item 4): credential ID lengths, the CBOR
//! subset (nesting, simple values, arrays, long-form lengths), and the COSE key parameters each
//! algorithm requires.

mod common;

use common::{
    Alg, Authenticator, Ceremony, ED, OwnedRegistration, attestation_object, cbor_bytes, cbor_head,
    cbor_int, cbor_map, cbor_text, challenge, rp,
};
use mandate_passkey::{Credential, PublicKey, Refusal, enrol};

const CHALLENGE: [u8; 16] = [0x33; 16];

fn enrol_with(registration: &OwnedRegistration) -> Result<Credential, Refusal> {
    enrol(&rp(), &challenge(&CHALLENGE), &registration.view())
}

/// A registration whose authenticator data carries `key` and, when `extensions` is not empty,
/// the `ED` flag and those bytes after it.
fn registration(authenticator: &Authenticator, key: &[u8], extensions: &[u8]) -> OwnedRegistration {
    let mut ceremony = Ceremony::create(&CHALLENGE);
    if !extensions.is_empty() {
        ceremony.flags |= ED;
    }
    let auth_data = authenticator.attested_auth_data(&ceremony, &[key, extensions].concat());
    OwnedRegistration {
        attestation_object: attestation_object("none", &cbor_map(&[]), &auth_data),
        client_data_json: ceremony.client_data_json(),
    }
}

#[test]
fn a_credential_id_has_one_to_1023_bytes() {
    for (length, accepted) in [(0, false), (1, true), (1023, true), (1024, false)] {
        let mut authenticator = Authenticator::new(Alg::Es256);
        authenticator.credential_id = vec![0x42; length];
        let result = enrol_with(&registration(
            &authenticator,
            &authenticator.cose_key(),
            &[],
        ));
        let expected = if accepted {
            Ok(authenticator.credential(0))
        } else {
            Err(Refusal::AuthenticatorDataMalformed)
        };
        assert_eq!(result, expected, "{length} bytes");
    }
}

#[test]
fn cbor_nests_at_most_eight_levels_below_the_outermost_item() {
    let authenticator = Authenticator::new(Alg::Ed25519);
    let nested = |levels: usize| {
        let inner = (0..levels).fold(cbor_int(0), |item, _| [cbor_head(4, 1), item].concat());
        cbor_map(&[(cbor_text("deep"), inner)])
    };
    let key = authenticator.cose_key();
    let deepest = enrol_with(&registration(&authenticator, &key, &nested(7)));
    assert_eq!(deepest, Ok(authenticator.credential(0)));
    let too_deep = enrol_with(&registration(&authenticator, &key, &nested(8)));
    assert_eq!(too_deep, Err(Refusal::AuthenticatorDataMalformed));
}

#[test]
fn booleans_null_and_arrays_are_read_and_tags_and_floats_are_not() {
    let authenticator = Authenticator::new(Alg::Ed25519);
    let key = authenticator.cose_key();
    let simple = cbor_map(&[
        (cbor_text("f"), vec![0xf4]),
        (cbor_text("t"), vec![0xf5]),
        (cbor_text("n"), vec![0xf6]),
        (
            cbor_text("a"),
            [cbor_head(4, 2), cbor_int(-1), cbor_text("x")].concat(),
        ),
    ]);
    let read = enrol_with(&registration(&authenticator, &key, &simple));
    assert_eq!(read, Ok(authenticator.credential(0)));
    for (name, value) in [
        ("tag", vec![0xc1, 0x00]),
        ("half float", vec![0xf9, 0x3c, 0x00]),
        ("undefined", vec![0xf7]),
    ] {
        let extensions = cbor_map(&[(cbor_text("v"), value)]);
        let result = enrol_with(&registration(&authenticator, &key, &extensions));
        assert_eq!(result, Err(Refusal::AuthenticatorDataMalformed), "{name}");
    }
}

#[test]
fn a_longer_length_form_reads_like_the_shortest() {
    let authenticator = Authenticator::new(Alg::Es256);
    let ceremony = Ceremony::create(&CHALLENGE);
    let auth_data = authenticator.attested_auth_data(&ceremony, &authenticator.cose_key());
    let length = auth_data.len() as u64;
    let heads = [
        [vec![0x59], (length as u16).to_be_bytes().to_vec()].concat(),
        [vec![0x5a], (length as u32).to_be_bytes().to_vec()].concat(),
        [vec![0x5b], length.to_be_bytes().to_vec()].concat(),
    ];
    for head in heads {
        let object = cbor_map(&[
            (cbor_text("fmt"), cbor_text("none")),
            (cbor_text("attStmt"), cbor_map(&[])),
            (
                cbor_text("authData"),
                [head.clone(), auth_data.clone()].concat(),
            ),
        ]);
        let registration = OwnedRegistration {
            attestation_object: object,
            client_data_json: ceremony.client_data_json(),
        };
        assert_eq!(
            enrol_with(&registration),
            Ok(authenticator.credential(0)),
            "{head:02x?}"
        );
    }
}

#[test]
fn an_eddsa_key_needs_the_okp_key_type_and_the_ed25519_curve() {
    let authenticator = Authenticator::new(Alg::Ed25519);
    let PublicKey::Ed25519 { x } = authenticator.public_key() else {
        panic!("an Ed25519 authenticator has an Ed25519 key");
    };
    let key = |kty: i64, crv: i64| {
        cbor_map(&[
            (cbor_int(1), cbor_int(kty)),
            (cbor_int(3), cbor_int(-8)),
            (cbor_int(-1), cbor_int(crv)),
            (cbor_int(-2), cbor_bytes(&x)),
        ])
    };
    for (kty, crv) in [(2, 6), (1, 7)] {
        let result = enrol_with(&registration(&authenticator, &key(kty, crv), &[]));
        assert_eq!(
            result,
            Err(Refusal::PublicKeyMalformed),
            "kty {kty}, crv {crv}"
        );
    }
    let result = enrol_with(&registration(&authenticator, &key(1, 6), &[]));
    assert_eq!(result, Ok(authenticator.credential(0)));
}
