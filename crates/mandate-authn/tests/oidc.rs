//! OIDC token verification against one configured issuer (identity spec §6.1, backlog E9-1): the
//! algorithm, the header, the key, the signature, the issuer, and the order of those checks. Every
//! token comes from the in-memory issuer in `common`; each expected outcome is how the token was
//! built, never what the verifier computes.

mod common;

use common::*;
use mandate_authn::{Jwks, Refusal, TokenKind, TokenPart, verify};
use serde_json::{Value, json};

#[test]
fn a_token_from_the_configured_issuer_verifies_under_each_allowed_algorithm() {
    let issuer = TestIssuer::new();
    for signer in [
        Signer::Es256,
        Signer::Es256Second,
        Signer::Rs256,
        Signer::EdDsa,
    ] {
        let subject = check(&issuer, &issuer.token(signer, &claims())).unwrap();
        assert_eq!(subject.issuer(), ISSUER, "{signer:?}");
        assert_eq!(subject.subject(), SUBJECT, "{signer:?}");
        assert_eq!(subject.expires_at(), at(NOW_S + LIFETIME_S), "{signer:?}");
        assert!(subject.email_verified(), "{signer:?}");
    }
}

#[test]
fn none_hmac_and_every_other_algorithm_are_refused_before_a_key_is_used() {
    let issuer = TestIssuer::new();
    let payload = claims().to_string();
    for alg in [
        "none", "HS256", "HS384", "HS512", "ES384", "ES512", "PS256", "RS512", "es256", "Es256",
        " ES256", "ES256\0", "",
    ] {
        for kid in ["es256-1", "no-such-kid"] {
            let header = json!({"alg": alg, "kid": kid}).to_string();
            let mut token = issuer.sign_raw(Signer::Es256, &header, &payload);
            let refusal = Refusal::AlgorithmNotAllowed;
            assert_eq!(refused(&issuer, &token), refusal, "{alg:?} {kid}");
            if alg == "none" {
                token.truncate(token.rfind('.').unwrap() + 1);
                assert_eq!(refused(&issuer, &token), refusal, "unsigned, {kid}");
            }
        }
    }
    for kid in ["es256-1", "no-such-kid"] {
        let confusion = json!({"alg": "HS256", "kid": kid}).to_string();
        for secret in issuer.es256_public_encodings() {
            let token = issuer.sign_hs256(&confusion, &payload, &secret);
            assert_eq!(
                refused(&issuer, &token),
                Refusal::AlgorithmNotAllowed,
                "{kid}"
            );
        }
    }
    let no_alg = json!({"kid": "es256-1"}).to_string();
    let token = issuer.sign_raw(Signer::Es256, &no_alg, &payload);
    assert_eq!(refused(&issuer, &token), Refusal::AlgorithmNotAllowed);
}

#[test]
fn an_issuer_configured_for_es256_only_refuses_its_other_algorithms() {
    let issuer = TestIssuer::new();
    let es = issuer.token(Signer::Es256, &claims());
    assert!(verify(&es, &es256_only(), &issuer.jwks(), ID, now()).is_ok());
    for signer in [Signer::Rs256, Signer::EdDsa] {
        let token = issuer.token(signer, &claims());
        let outcome = verify(&token, &es256_only(), &issuer.jwks(), ID, now());
        assert_eq!(
            outcome.unwrap_err(),
            Refusal::AlgorithmNotAllowed,
            "{signer:?}"
        );
    }
}

#[test]
fn a_header_that_names_its_own_key_or_a_critical_extension_is_refused() {
    let issuer = TestIssuer::new();
    let payload = claims().to_string();
    let own_key = issuer.jwk(Signer::ImpostorEs256);
    for (member, value) in [
        ("jwk", own_key.clone()),
        ("jku", json!("https://attacker.test/jwks.json")),
        ("x5u", json!("https://attacker.test/cert.pem")),
        ("x5c", json!([b64(b"certificate")])),
        ("x5t", json!(b64(&[3; 20]))),
        ("x5t#S256", json!(b64(&[3; 32]))),
    ] {
        for (signer, kid) in [
            (Signer::ImpostorEs256, "attacker-1"),
            (Signer::Es256, "es256-1"),
        ] {
            let header = json!({"alg": "ES256", "kid": kid, member: value}).to_string();
            let token = issuer.sign_raw(signer, &header, &payload);
            assert_eq!(
                refused(&issuer, &token),
                Refusal::HeaderKey,
                "{member} {kid}"
            );
        }
    }
    let crit = json!({"alg": "ES256", "kid": "es256-1", "crit": ["x-unknown"], "x-unknown": 1});
    let token = issuer.sign_raw(Signer::Es256, &crit.to_string(), &payload);
    assert_eq!(refused(&issuer, &token), Refusal::CriticalHeader);
}

#[test]
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
    let unknown = issuer.sign_raw(Signer::Es256, &header("ES256", json!("es256-9")), &payload);
    assert_eq!(refused(&issuer, &unknown), Refusal::KeyNotFound);
    for (alg, signer, kid) in [
        ("RS256", Signer::Rs256, "es256-1"),
        ("ES256", Signer::Es256, "ed25519-1"),
        ("EdDSA", Signer::EdDsa, "rs256-1"),
        ("ES256", Signer::Es256, "rs256-1"),
    ] {
        let token = issuer.sign_raw(signer, &header(alg, json!(kid)), &payload);
        let refusal = Refusal::KeyAlgorithmMismatch;
        assert_eq!(refused(&issuer, &token), refusal, "{alg} {kid}");
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
fn a_signature_from_another_key_or_over_other_bytes_is_refused() {
    let issuer = TestIssuer::new();
    let impostor = issuer.token(Signer::ImpostorEs256, &claims());
    assert_eq!(refused(&issuer, &impostor), Refusal::BadSignature);
    let payload = claims().to_string();
    let named_first = json!({"alg": "ES256", "kid": "es256-1"}).to_string();
    let second_under_first = issuer.sign_raw(Signer::Es256Second, &named_first, &payload);
    assert_eq!(
        refused(&issuer, &second_under_first),
        Refusal::BadSignature,
        "kid is read"
    );
    for signer in Signer::ALLOWED {
        let token = issuer.token(signer, &claims());
        assert!(check(&issuer, &token).is_ok(), "{signer:?}");
        let (input, sig) = token.rsplit_once('.').unwrap();
        let (header, _) = input.split_once('.').unwrap();
        let admin = b64(with("sub", json!("admin")).to_string().as_bytes());
        let swapped = format!("{header}.{admin}.{sig}");
        assert_eq!(
            refused(&issuer, &swapped),
            Refusal::BadSignature,
            "{signer:?}"
        );
        let empty = format!("{input}.");
        assert_eq!(
            refused(&issuer, &empty),
            Refusal::BadSignature,
            "empty, {signer:?}"
        );
    }
}

#[test]
fn an_es256_signature_is_the_raw_64_bytes_and_no_other_form() {
    let issuer = TestIssuer::new();
    let header = json!({"alg": "ES256", "kid": "es256-1"}).to_string();
    let payload = claims().to_string();
    let der = issuer.sign_es256_der(&header, &payload);
    assert_eq!(refused(&issuer, &der), Refusal::BadSignature, "DER");
    let token = issuer.sign_raw(Signer::Es256, &header, &payload);
    assert!(check(&issuer, &token).is_ok(), "the raw 64 bytes verify");
    let (input, sig) = token.rsplit_once('.').unwrap();
    let raw = unb64(sig);
    assert_eq!(raw.len(), 64);
    let short = format!("{input}.{}", b64(&raw[..63]));
    assert_eq!(refused(&issuer, &short), Refusal::BadSignature, "63 bytes");
    let long = format!("{input}.{}", b64(&[raw.as_slice(), &[0]].concat()));
    assert_eq!(refused(&issuer, &long), Refusal::BadSignature, "65 bytes");
}

#[test]
fn no_claim_is_read_before_the_signature_verifies() {
    let issuer = TestIssuer::new();
    for (key, value) in [
        ("iss", json!(OTHER_TENANT)),
        ("exp", json!(NOW_S - 3_600)),
        ("aud", json!("other-client")),
        ("nonce", json!("other")),
        ("iss", json!(7)),
    ] {
        let genuine = issuer.token(Signer::Es256, &with(key, value.clone()));
        assert_ne!(
            refused(&issuer, &genuine),
            Refusal::BadSignature,
            "the claim alone is refused, {key} {value}"
        );
        let token = issuer.token(Signer::ImpostorEs256, &with(key, value.clone()));
        assert_eq!(
            refused(&issuer, &token),
            Refusal::BadSignature,
            "{key} {value}"
        );
    }
}

#[test]
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
        Refusal::Malformed {
            part: TokenPart::Payload,
        },
        Refusal::AlgorithmNotAllowed,
        Refusal::CriticalHeader,
        Refusal::HeaderKey,
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
        "token_malformed",
        "algorithm_not_allowed",
        "critical_header",
        "header_key",
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

#[test]
fn a_token_kind_never_prints_its_nonce() {
    let kind = TokenKind::IdToken { nonce: NONCE };
    assert_eq!(format!("{kind:?}"), "IdToken { nonce: .. }");
    assert_eq!(format!("{:?}", TokenKind::AccessToken), "AccessToken");
}
