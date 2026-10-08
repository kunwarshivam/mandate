//! Boundaries the A1 review's planted-bug pass found unpinned (DEC-650 items 2 to 5, the lane
//! lead's rulings on the #810 and #820 reviews): the skew's value, `exp` and `nbf` at the edges of
//! the clock's range, a numeric `sub`, an empty `kid`, and the key set's exact `Debug`.

mod common;

use common::*;
use mandate_authn::{CLOCK_SKEW_S, Jwks, Refusal, TokenPart, UtcNanos, verify};
use serde_json::{Value, json};

const MAX_SECS: i64 = 253_402_300_799;

#[test]
fn the_skew_is_sixty_seconds_on_each_side() {
    assert_eq!(CLOCK_SKEW_S, 60, "DEC-650 item 4");
    let issuer = TestIssuer::new();
    let exp = NOW_S + LIFETIME_S;
    let token = issuer.token(Signer::EdDsa, &with("nbf", json!(NOW_S)));
    let at_time = |secs: i64| verify(&token, &config(), &issuer.jwks(), ID, at(secs)).map(|_| ());
    assert_eq!(at_time(exp + 59), Ok(()));
    assert_eq!(at_time(exp + 60), Err(Refusal::Expired));
    assert_eq!(at_time(NOW_S - 60), Ok(()));
    assert_eq!(at_time(NOW_S - 61), Err(Refusal::NotYetValid));
}

#[test]
fn an_expiry_at_the_last_representable_second_verifies() {
    let issuer = TestIssuer::new();
    let token = issuer.token(Signer::Es256, &with("exp", json!(MAX_SECS)));
    let subject = check(&issuer, &token).unwrap();
    assert_eq!(
        subject.expires_at(),
        UtcNanos::from_parts(MAX_SECS, 0).unwrap()
    );
}

#[test]
fn a_not_before_outside_the_clock_is_malformed_and_the_epoch_is_not() {
    let issuer = TestIssuer::new();
    let outcome =
        |nbf: Value| check(&issuer, &issuer.token(Signer::Es256, &with("nbf", nbf))).map(|_| ());
    let malformed = Err(Refusal::Malformed {
        part: TokenPart::Payload,
    });
    assert_eq!(outcome(json!(-1)), malformed, "before the epoch");
    assert_eq!(outcome(json!(i64::MAX)), malformed, "past 9999-12-31");
    assert_eq!(
        outcome(json!(MAX_SECS + 1)),
        malformed,
        "one second past the range"
    );
    assert_eq!(outcome(json!(0)), Ok(()), "the epoch");
    assert_eq!(
        outcome(json!(MAX_SECS)),
        Err(Refusal::NotYetValid),
        "the last second"
    );
}

#[test]
fn a_numeric_subject_is_malformed() {
    let issuer = TestIssuer::new();
    let token = issuer.token(Signer::Es256, &with("sub", json!(7)));
    assert_eq!(
        refused(&issuer, &token),
        Refusal::Malformed {
            part: TokenPart::Payload
        }
    );
}

#[test]
fn an_empty_kid_names_no_key() {
    let issuer = TestIssuer::new();
    let mut nameless = issuer.jwk(Signer::Es256);
    nameless["kid"] = json!("");
    let jwks = Jwks::parse(&json!({ "keys": [nameless] }).to_string()).unwrap();
    let header = json!({"alg": "ES256", "kid": ""}).to_string();
    let token = issuer.sign_raw(Signer::Es256, &header, &claims().to_string());
    let outcome = verify(&token, &config(), &jwks, ID, now());
    assert_eq!(outcome.unwrap_err(), Refusal::KeyNotFound);
}

#[test]
fn a_key_set_prints_its_key_ids_and_nothing_else() {
    let issuer = TestIssuer::new();
    assert_eq!(
        format!("{:?}", issuer.jwks()),
        r#"Jwks { kids: ["ed25519-1", "es256-1", "es256-2", "rs256-1"] }"#
    );
}
