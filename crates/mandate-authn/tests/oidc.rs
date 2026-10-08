//! OIDC token verification against one configured issuer (identity spec §6.1, backlog E9-1): the
//! algorithm, the key, the signature, and the issuer. Every token comes from the in-memory issuer
//! in `common`; each expected outcome is how the token was built, never what the verifier computes.

mod common;

use common::*;
use mandate_authn::{Jwks, Refusal, TokenPart, verify};
use serde_json::{Value, json};

#[test]
#[ignore = "pending E9-1"]
fn a_token_from_the_configured_issuer_verifies_under_each_allowed_algorithm() {
    let issuer = TestIssuer::new();
    for signer in Signer::ALLOWED {
        let subject = check(&issuer, &issuer.token(signer, &claims())).unwrap();
        assert_eq!(subject.issuer(), ISSUER, "{signer:?}");
        assert_eq!(subject.subject(), SUBJECT, "{signer:?}");
        assert_eq!(subject.expires_at(), at(NOW_S + LIFETIME_S), "{signer:?}");
        assert!(subject.email_verified(), "{signer:?}");
    }
}

#[test]
#[ignore = "pending E9-1"]
fn none_hmac_and_every_other_algorithm_are_refused_before_a_key_is_used() {
    let issuer = TestIssuer::new();
    let payload = claims().to_string();
    for alg in [
        "none", "HS256", "HS384", "HS512", "ES384", "ES512", "PS256", "RS512", "es256", "",
    ] {
        let header = json!({"alg": alg, "kid": "es256-1"}).to_string();
        let mut token = issuer.sign_raw(Signer::Es256, &header, &payload);
        assert_eq!(
            refused(&issuer, &token),
            Refusal::AlgorithmNotAllowed,
            "{alg}"
        );
        if alg == "none" {
            token.truncate(token.rfind('.').unwrap() + 1);
            assert_eq!(
                refused(&issuer, &token),
                Refusal::AlgorithmNotAllowed,
                "unsigned"
            );
        }
    }
    let confusion = json!({"alg": "HS256", "kid": "es256-1"}).to_string();
    for secret in [issuer.es256_public_point(), issuer.jwks_json().into_bytes()] {
        let token = issuer.sign_hs256(&confusion, &payload, &secret);
        assert_eq!(refused(&issuer, &token), Refusal::AlgorithmNotAllowed);
    }
    let no_alg = json!({"kid": "es256-1"}).to_string();
    let token = issuer.sign_raw(Signer::Es256, &no_alg, &payload);
    assert_eq!(refused(&issuer, &token), Refusal::AlgorithmNotAllowed);
}

#[test]
#[ignore = "pending E9-1"]
fn a_key_must_be_named_found_and_of_the_algorithms_type() {
    let issuer = TestIssuer::new();
    let payload = claims().to_string();
    let header = |alg: &str, kid: Value| {
        let mut h = json!({"alg": alg, "kid": kid});
        if kid.is_null() {
            h.as_object_mut().unwrap().remove("kid");
        }
        h.to_string()
    };
    let missing = issuer.sign_raw(Signer::Es256, &header("ES256", Value::Null), &payload);
    assert_eq!(refused(&issuer, &missing), Refusal::KeyNotFound);
    let unknown = issuer.sign_raw(Signer::Es256, &header("ES256", json!("es256-2")), &payload);
    assert_eq!(refused(&issuer, &unknown), Refusal::KeyNotFound);
    for (alg, signer, kid) in [
        ("RS256", Signer::Rs256, "es256-1"),
        ("ES256", Signer::Es256, "ed25519-1"),
        ("EdDSA", Signer::EdDsa, "rs256-1"),
        ("ES256", Signer::Es256, "rs256-1"),
    ] {
        let token = issuer.sign_raw(signer, &header(alg, json!(kid)), &payload);
        assert_eq!(
            refused(&issuer, &token),
            Refusal::KeyAlgorithmMismatch,
            "{alg} {kid}"
        );
    }
    let mut keys = vec![issuer.jwk(Signer::Es256)];
    keys[0]["alg"] = json!("RS256");
    let jwks = Jwks::parse(&json!({ "keys": keys }).to_string()).unwrap();
    let token = issuer.token(Signer::Es256, &claims());
    let outcome = verify(&token, &config(), &jwks, ID, now());
    assert_eq!(
        outcome.unwrap_err(),
        Refusal::KeyAlgorithmMismatch,
        "the key's own alg"
    );
}

#[test]
#[ignore = "pending E9-1"]
fn a_signature_from_another_key_or_over_other_bytes_is_refused() {
    let issuer = TestIssuer::new();
    let impostor = issuer.token(Signer::ImpostorEs256, &claims());
    assert_eq!(refused(&issuer, &impostor), Refusal::BadSignature);
    for signer in Signer::ALLOWED {
        let token = issuer.token(signer, &claims());
        let (input, sig) = token.rsplit_once('.').unwrap();
        let (header, _) = input.split_once('.').unwrap();
        let swapped = format!(
            "{header}.{}.{sig}",
            b64(with("sub", json!("admin")).to_string().as_bytes())
        );
        assert_eq!(
            refused(&issuer, &swapped),
            Refusal::BadSignature,
            "{signer:?}"
        );
        let empty = format!("{input}.");
        assert_eq!(
            refused(&issuer, &empty),
            Refusal::BadSignature,
            "{signer:?}"
        );
    }
}

#[test]
#[ignore = "pending E9-1"]
fn only_the_configured_issuer_is_accepted_even_under_the_same_keys() {
    let issuer = TestIssuer::new();
    for iss in [
        OTHER_TENANT,
        "https://tenant-a.auth.test/auth/v1/",
        "HTTPS://tenant-a.auth.test/auth/v1",
        "",
    ] {
        let token = issuer.token(Signer::Es256, &with("iss", json!(iss)));
        assert_eq!(refused(&issuer, &token), Refusal::WrongIssuer, "{iss}");
    }
    let token = issuer.token(Signer::Es256, &with("iss", Value::Null));
    assert_eq!(
        refused(&issuer, &token),
        Refusal::MissingClaim { claim: "iss" }
    );
    let token = issuer.token(Signer::Es256, &with("sub", Value::Null));
    assert_eq!(
        refused(&issuer, &token),
        Refusal::MissingClaim { claim: "sub" }
    );
}

#[test]
fn a_refusal_code_names_its_check_and_carries_no_value() {
    let refusals = [
        Refusal::Unimplemented { story: "E9-1" },
        Refusal::Malformed {
            part: TokenPart::Payload,
        },
        Refusal::AlgorithmNotAllowed,
        Refusal::CriticalHeader,
        Refusal::KeyNotFound,
        Refusal::KeyAlgorithmMismatch,
        Refusal::BadSignature,
        Refusal::MissingClaim { claim: "exp" },
        Refusal::WrongIssuer,
        Refusal::WrongAudience,
        Refusal::WrongAuthorizedParty,
        Refusal::Expired,
        Refusal::NotYetValid,
        Refusal::NonceMismatch,
        Refusal::EmailNotVerified,
        Refusal::EmailMismatch,
    ];
    let codes: Vec<&str> = refusals.iter().map(Refusal::code).collect();
    let expected = [
        "unimplemented",
        "token_malformed",
        "algorithm_not_allowed",
        "critical_header",
        "key_not_found",
        "key_algorithm_mismatch",
        "bad_signature",
        "missing_claim",
        "wrong_issuer",
        "wrong_audience",
        "wrong_authorized_party",
        "token_expired",
        "token_not_yet_valid",
        "nonce_mismatch",
        "email_not_verified",
        "email_mismatch",
    ];
    assert_eq!(codes, expected);
}
