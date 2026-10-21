//! The token request's body holds exactly the six members, each once, in order, and decodes back
//! to the values given (DEC-2629 item 2).

use proptest::prelude::*;
use proptest::test_runner::TestRunner;
use secrecy::SecretString;

use crate::form::body;
use mandate_connections::exchange::{ClientSecret, ExchangeForm};
use mandate_connections::start::{ClientId, PkceVerifier};
use mandate_connections::vault::AuthorizationCode;

/// The six values, in the body's order after `grant_type`.
struct Values {
    code: String,
    redirect_uri: String,
    client_id: String,
    client_secret: String,
    verifier: String,
}

fn written(values: &Values) -> String {
    let code = AuthorizationCode(SecretString::from(values.code.as_str()));
    let verifier = PkceVerifier(SecretString::from(values.verifier.as_str()));
    let client_id = ClientId(values.client_id.clone());
    let secret = ClientSecret(SecretString::from(values.client_secret.as_str()));
    body(&ExchangeForm {
        code: &code,
        code_verifier: &verifier,
        client_id: &client_id,
        client_secret: &secret,
        redirect_uri: &values.redirect_uri,
    })
}

/// RFC 3986's percent-decoding, written here rather than taken from the crate under test.
fn decoded(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[test]
#[ignore = "pending E10-13"]
fn the_body_is_the_six_members_in_order_with_reserved_characters_encoded() {
    let values = Values {
        code: "c0de/+=".to_owned(),
        redirect_uri: "https://api.owlhead.ai/v1/oauth/alpaca/callback".to_owned(),
        client_id: "client-1".to_owned(),
        client_secret: "s3cret&x=y".to_owned(),
        verifier: "v_1-x.y~z".to_owned(),
    };
    assert_eq!(
        written(&values),
        "grant_type=authorization_code&code=c0de%2F%2B%3D\
         &redirect_uri=https%3A%2F%2Fapi.owlhead.ai%2Fv1%2Foauth%2Falpaca%2Fcallback\
         &client_id=client-1&client_secret=s3cret%26x%3Dy&code_verifier=v_1-x.y~z"
    );
}

/// For any visible-ASCII values, the body is unreserved characters, `%`, `&` and `=` only, and
/// splitting and decoding it independently gives back the six members, each once, in order.
#[test]
#[ignore = "pending E10-13"]
fn any_values_decode_back_to_exactly_the_six_members() {
    let value = || "[!-~ ]{1,64}";
    let values = (value(), value(), value(), value(), value());
    TestRunner::deterministic()
        .run(
            &values,
            |(code, redirect_uri, client_id, client_secret, verifier)| {
                let values = Values {
                    code,
                    redirect_uri,
                    client_id,
                    client_secret,
                    verifier,
                };
                let text = written(&values);
                prop_assert!(
                    text.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-._~%&=".contains(&b)),
                    "{}",
                    text
                );
                let pairs: Option<Vec<(String, String)>> = text
                    .split('&')
                    .map(|pair| {
                        let (name, value) = pair.split_once('=')?;
                        Some((name.to_owned(), decoded(value)?))
                    })
                    .collect();
                let expected = [
                    ("grant_type", "authorization_code"),
                    ("code", values.code.as_str()),
                    ("redirect_uri", values.redirect_uri.as_str()),
                    ("client_id", values.client_id.as_str()),
                    ("client_secret", values.client_secret.as_str()),
                    ("code_verifier", values.verifier.as_str()),
                ]
                .map(|(name, value)| (name.to_owned(), value.to_owned()));
                prop_assert_eq!(pairs, Some(expected.to_vec()));
                Ok(())
            },
        )
        .unwrap();
}
