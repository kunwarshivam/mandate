//! The claims of a verified token (identity spec §6.1, backlog E9-1): audience and authorized
//! party, expiry and not-before with the stated skew, the nonce, and `email_verified` for an
//! invitation matched by address.

mod common;

use common::*;
use mandate_authn::{CLOCK_SKEW_S, Refusal, TokenKind, TokenPart, verify};
use proptest::prelude::*;
use serde_json::{Value, json};

#[test]
#[ignore = "pending E9-1"]
fn the_audience_must_name_this_client_and_several_need_its_authorized_party() {
    let issuer = TestIssuer::new();
    let outcome = |claims: Value| check(&issuer, &issuer.token(Signer::Es256, &claims)).map(|_| ());
    assert_eq!(outcome(with("aud", json!([AUDIENCE]))), Ok(()));
    assert_eq!(
        outcome(with("aud", json!("other-client"))),
        Err(Refusal::WrongAudience)
    );
    assert_eq!(
        outcome(with("aud", json!(["other-client"]))),
        Err(Refusal::WrongAudience)
    );
    assert_eq!(outcome(with("aud", json!([]))), Err(Refusal::WrongAudience));
    assert_eq!(
        outcome(with("aud", Value::Null)),
        Err(Refusal::MissingClaim { claim: "aud" })
    );
    let several = json!([AUDIENCE, "other-client"]);
    let mut c = with("aud", several.clone());
    assert_eq!(
        outcome(c.clone()),
        Err(Refusal::WrongAuthorizedParty),
        "no azp"
    );
    c["azp"] = json!("other-client");
    assert_eq!(
        outcome(c.clone()),
        Err(Refusal::WrongAuthorizedParty),
        "another azp"
    );
    c["azp"] = json!(AUDIENCE);
    assert_eq!(outcome(c), Ok(()));
}

#[test]
#[ignore = "pending E9-1"]
fn expiry_and_not_before_hold_to_the_stated_skew() {
    let issuer = TestIssuer::new();
    let exp = NOW_S + LIFETIME_S;
    let token = issuer.token(Signer::EdDsa, &with("nbf", json!(NOW_S)));
    let at_time = |secs: i64, nanos: u32| {
        let now = mandate_authn::UtcNanos::from_parts(secs, nanos).unwrap();
        verify(&token, &config(), &issuer.jwks(), ID, now).map(|_| ())
    };
    assert_eq!(at_time(exp + CLOCK_SKEW_S - 1, 999_999_999), Ok(()));
    assert_eq!(at_time(exp + CLOCK_SKEW_S, 0), Err(Refusal::Expired));
    assert_eq!(at_time(NOW_S - CLOCK_SKEW_S, 0), Ok(()));
    assert_eq!(
        at_time(NOW_S - CLOCK_SKEW_S - 1, 999_999_999),
        Err(Refusal::NotYetValid)
    );
    let no_exp = issuer.token(Signer::EdDsa, &with("exp", Value::Null));
    assert_eq!(
        refused(&issuer, &no_exp),
        Refusal::MissingClaim { claim: "exp" }
    );
}

#[test]
#[ignore = "pending E9-1"]
fn an_id_token_needs_the_sign_ins_nonce_and_an_access_token_needs_none() {
    let issuer = TestIssuer::new();
    let outcome = |claims: Value, kind| {
        let token = issuer.token(Signer::Rs256, &claims);
        verify(&token, &config(), &issuer.jwks(), kind, now()).map(|_| ())
    };
    assert_eq!(
        outcome(with("nonce", Value::Null), ID),
        Err(Refusal::NonceMismatch)
    );
    assert_eq!(
        outcome(with("nonce", json!("other")), ID),
        Err(Refusal::NonceMismatch)
    );
    let numeric = Err(Refusal::Malformed {
        part: TokenPart::Payload,
    });
    assert_eq!(
        outcome(with("nonce", json!(7)), ID),
        numeric,
        "a nonce is a string"
    );
    let empty = TokenKind::IdToken { nonce: "" };
    assert_eq!(
        outcome(with("nonce", json!("")), empty),
        Err(Refusal::NonceMismatch)
    );
    assert_eq!(
        outcome(with("nonce", Value::Null), TokenKind::AccessToken),
        Ok(())
    );
}

#[test]
#[ignore = "pending E9-1"]
fn an_invitation_matches_only_a_verified_address_byte_for_byte() {
    let issuer = TestIssuer::new();
    let subject = |claims: Value| check(&issuer, &issuer.token(Signer::Es256, &claims)).unwrap();
    assert_eq!(subject(claims()).matches_invitation(EMAIL), Ok(()));
    let other = subject(claims()).matches_invitation("Invitee@example.test");
    assert_eq!(other, Err(Refusal::EmailMismatch));
    for verified in [json!(false), json!("true"), json!(1), Value::Null] {
        let s = subject(with("email_verified", verified.clone()));
        assert!(!s.email_verified(), "{verified}");
        assert_eq!(
            s.matches_invitation(EMAIL),
            Err(Refusal::EmailNotVerified),
            "{verified}"
        );
    }
    let s = subject(with("email", Value::Null));
    assert_eq!(s.matches_invitation(EMAIL), Err(Refusal::EmailMismatch));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    #[ignore = "pending E9-1"]
    fn a_token_verifies_exactly_inside_its_skewed_window(nbf in -400i64..400, exp in -400i64..400, now_s in -600i64..600, nanos in 0u32..1_000_000_000) {
        let issuer = TestIssuer::new();
        let claims = with_all(&[("nbf", json!(NOW_S + nbf)), ("exp", json!(NOW_S + exp))]);
        let token = issuer.token(Signer::EdDsa, &claims);
        let when = mandate_authn::UtcNanos::from_parts(NOW_S + now_s, nanos).unwrap();
        let expected = if now_s >= exp + CLOCK_SKEW_S {
            Err(Refusal::Expired)
        } else if now_s < nbf - CLOCK_SKEW_S {
            Err(Refusal::NotYetValid)
        } else {
            Ok(())
        };
        prop_assert_eq!(verify(&token, &config(), &issuer.jwks(), ID, when).map(|_| ()), expected);
    }
}
