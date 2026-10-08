//! The software authenticator is the oracle every passkey test trusts, so it is checked first
//! against published vectors: base64url against RFC 4648 §10 (the prefixes of `foobar`), the CBOR writer against RFC 8949
//! Appendix A, and its signatures against `ring`'s verifier. Live from the tests PR on.

mod common;

use common::{Alg, Authenticator, Ceremony, b64url, cbor_bytes, cbor_int, cbor_map, cbor_text};
use ring::signature::{ECDSA_P256_SHA256_ASN1, ED25519, UnparsedPublicKey};

#[test]
fn base64url_matches_rfc_4648_without_padding() {
    let encodings = ["", "Zg", "Zm8", "Zm9v", "Zm9vYg", "Zm9vYmE", "Zm9vYmFy"];
    for (length, encoded) in encodings.into_iter().enumerate() {
        assert_eq!(
            b64url(&b"foobar"[..length]),
            encoded,
            "{length} bytes of foobar"
        );
    }
    assert_eq!(b64url(&[0xfb, 0xff]), "-_8");
}

#[test]
fn the_cbor_writer_matches_rfc_8949_appendix_a() {
    let vectors: [(i64, &str); 10] = [
        (0, "00"),
        (23, "17"),
        (24, "1818"),
        (100, "1864"),
        (1000, "1903e8"),
        (1_000_000, "1a000f4240"),
        (1_000_000_000_000, "1b000000e8d4a51000"),
        (-1, "20"),
        (-100, "3863"),
        (-1000, "3903e7"),
    ];
    for (n, hex) in vectors {
        let encoded: String = cbor_int(n).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(encoded, hex, "{n}");
    }
    assert_eq!(cbor_bytes(&[1, 2, 3, 4]), [0x44, 1, 2, 3, 4]);
    assert_eq!(cbor_text("IETF"), b"dIETF");
    let map = cbor_map(&[(cbor_int(1), cbor_int(2)), (cbor_int(3), cbor_int(4))]);
    assert_eq!(map, [0xa2, 0x01, 0x02, 0x03, 0x04]);
}

#[test]
fn the_authenticator_signs_what_ring_verifies() {
    for alg in [Alg::Es256, Alg::Ed25519] {
        let authenticator = Authenticator::new(alg);
        let assertion = authenticator.assert(&Ceremony::get(&[9; 16], 1));
        let message = [
            assertion.authenticator_data.clone(),
            common::sha256(&assertion.client_data_json),
        ]
        .concat();
        let checked = match authenticator.public_key() {
            mandate_passkey::PublicKey::Es256 { x, y } => {
                let point = [&[0x04][..], &x, &y].concat();
                UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, point)
                    .verify(&message, &assertion.signature)
            }
            mandate_passkey::PublicKey::Ed25519 { x } => {
                UnparsedPublicKey::new(&ED25519, x).verify(&message, &assertion.signature)
            }
        };
        assert!(checked.is_ok(), "{alg:?}");
        assert_eq!(assertion.authenticator_data.len(), 37, "{alg:?}");
        assert_eq!(assertion.authenticator_data[32], common::UP | common::UV);
    }
}
