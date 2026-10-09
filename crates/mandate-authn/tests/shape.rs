//! The token's shape and the issuer's key set (identity spec §6.1, ID-9, backlog E9-1): malformed
//! and oversized tokens, non-canonical base64url, the keys a JWKS may hold, any single-byte change,
//! and no refusal or subject printing a token, a nonce, a subject, or a key.

mod common;

use common::*;
use mandate_authn::{
    Algorithm, IssuerConfig, Jwks, MAX_TOKEN_BYTES, Refusal, SetupError, TokenKind, TokenPart,
    verify,
};
use proptest::prelude::*;
use serde_json::{Value, json};

#[test]
#[ignore = "pending E9-1"]
fn a_malformed_or_critical_token_is_refused_by_the_part_it_breaks() {
    let issuer = TestIssuer::new();
    let good = issuer.token(Signer::Es256, &claims());
    let header = r#"{"alg":"ES256","kid":"es256-1"}"#;
    let raw = |h: &str, p: &str| issuer.sign_raw(Signer::Es256, h, p);
    let part = |part| Refusal::Malformed { part };
    let cases = [
        ("a.b".to_owned(), part(TokenPart::Token)),
        (format!("{good}.x"), part(TokenPart::Token)),
        ("a".repeat(MAX_TOKEN_BYTES + 1), part(TokenPart::Token)),
        (format!("{good}=="), part(TokenPart::Signature)),
        (format!("!{good}"), part(TokenPart::Header)),
        (raw("[1]", &claims().to_string()), part(TokenPart::Header)),
        (
            raw(r#"{"alg":7,"kid":"es256-1"}"#, &claims().to_string()),
            part(TokenPart::Header),
        ),
        (
            raw(r#"{"alg":"ES256","kid":7}"#, &claims().to_string()),
            part(TokenPart::Header),
        ),
        (
            raw(r#"{"alg":"ES256","alg":"none","kid":"es256-1"}"#, "{}"),
            part(TokenPart::Header),
        ),
        (raw(header, "not json"), part(TokenPart::Payload)),
        (
            raw(
                header,
                &claims().to_string().replacen('{', r#"{"sub":"admin","#, 1),
            ),
            part(TokenPart::Payload),
        ),
        (
            issuer.token(Signer::Es256, &with("exp", json!(1.8e9))),
            part(TokenPart::Payload),
        ),
        (
            issuer.token(Signer::Es256, &with("iss", json!(7))),
            part(TokenPart::Payload),
        ),
        (
            raw(
                r#"{"alg":"ES256","kid":"es256-1","crit":["exp"]}"#,
                &claims().to_string(),
            ),
            Refusal::CriticalHeader,
        ),
    ];
    for (token, expected) in cases {
        assert_eq!(refused(&issuer, &token), expected, "{expected:?}");
    }
}

/// A valid EdDSA token exactly `len` bytes long, its padding split between a header member and a
/// claim so that every length is reachable.
fn token_of_length(issuer: &TestIssuer, len: usize) -> String {
    let sign = |header_pad: usize, claim_pad: usize| {
        let header = json!({"alg": "EdDSA", "kid": "ed25519-1", "x": "h".repeat(header_pad)});
        let claims = with("pad", json!("p".repeat(claim_pad)));
        issuer.sign_raw(Signer::EdDsa, &header.to_string(), &claims.to_string())
    };
    let near = (len - sign(0, 0).len()) * 3 / 4 - 4;
    for header_pad in 0..3 {
        for claim_pad in near..near + 8 {
            let token = sign(header_pad, claim_pad);
            if token.len() == len {
                return token;
            }
        }
    }
    panic!("no padding gives a token of {len} bytes")
}

#[test]
#[ignore = "pending E9-1"]
fn a_token_of_the_largest_size_verifies_and_one_byte_more_is_refused() {
    let issuer = TestIssuer::new();
    assert_eq!(MAX_TOKEN_BYTES, 16 * 1024, "DEC-650 item 3");
    let largest = token_of_length(&issuer, MAX_TOKEN_BYTES);
    assert!(check(&issuer, &largest).is_ok(), "{} bytes", largest.len());
    let over = token_of_length(&issuer, MAX_TOKEN_BYTES + 1);
    let refusal = Refusal::Malformed {
        part: TokenPart::Token,
    };
    assert_eq!(refused(&issuer, &over), refusal);
}

#[test]
#[ignore = "pending E9-1"]
fn the_key_set_keeps_only_usable_signing_keys_and_refuses_ambiguity() {
    let issuer = TestIssuer::new();
    let parse = |keys: Value| Jwks::parse(&json!({ "keys": keys }).to_string());
    let es = issuer.jwk(Signer::Es256);
    let oct = json!({"kty": "oct", "kid": "es256-1", "k": b64(b"shared")});
    let jwks = parse(json!([oct, issuer.jwk(Signer::EdDsa)])).unwrap();
    let hs = issuer.sign_hs256(
        r#"{"alg":"HS256","kid":"es256-1"}"#,
        &claims().to_string(),
        b"shared",
    );
    assert_eq!(
        verify(&hs, &config(), &jwks, ID, now()).unwrap_err(),
        Refusal::AlgorithmNotAllowed
    );
    let es_token = issuer.token(Signer::Es256, &claims());
    assert_eq!(
        verify(&es_token, &config(), &jwks, ID, now()).unwrap_err(),
        Refusal::KeyNotFound
    );
    let mut enc = es.clone();
    enc["use"] = json!("enc");
    let jwks = parse(json!([enc])).unwrap();
    assert_eq!(
        verify(&es_token, &config(), &jwks, ID, now()).unwrap_err(),
        Refusal::KeyNotFound
    );
    assert_eq!(parse(json!([es, es])), Err(SetupError::DuplicateKeyId));
    let mut enc_twin = es.clone();
    enc_twin["use"] = json!("enc");
    let oct_twin = json!({"kty": "oct", "kid": "es256-1", "k": b64(b"shared")});
    let kept = parse(json!([oct_twin, es, enc_twin])).unwrap();
    assert!(
        verify(&es_token, &config(), &kept, ID, now()).is_ok(),
        "a left-out key sharing a kept key's kid is no duplicate"
    );
    let mut no_kid = es.clone();
    no_kid.as_object_mut().unwrap().remove("kid");
    let jwks = parse(json!([no_kid])).unwrap();
    assert_eq!(
        verify(&es_token, &config(), &jwks, ID, now()).unwrap_err(),
        Refusal::KeyNotFound
    );
    for member in ["x", "y"] {
        let mut missing = es.clone();
        missing.as_object_mut().unwrap().remove(member);
        assert_eq!(
            parse(json!([missing])),
            Err(SetupError::KeyMalformed),
            "{member}"
        );
    }
    let es_text = es.to_string();
    let doubled_keys = format!(r#"{{"keys":[],"keys":[{es_text}]}}"#);
    assert_eq!(Jwks::parse(&doubled_keys), Err(SetupError::JwksMalformed));
    let doubled_kid = es_text.replacen('{', r#"{"kid":"other","#, 1);
    let doubled = format!(r#"{{"keys":[{doubled_kid}]}}"#);
    assert_eq!(
        Jwks::parse(&doubled),
        Err(SetupError::JwksMalformed),
        "{doubled}"
    );
    let mut short = issuer.jwk(Signer::Es256);
    short["x"] = json!(b64(&[1; 31]));
    assert_eq!(parse(json!([short])), Err(SetupError::KeyMalformed));
    assert_eq!(Jwks::parse("[]"), Err(SetupError::JwksMalformed));
}

#[test]
#[ignore = "pending E9-1"]
fn a_key_of_another_curve_or_a_weak_rsa_modulus_is_never_used() {
    let issuer = TestIssuer::new();
    let es = issuer.jwk(Signer::Es256);
    let mut p384 = es.clone();
    p384["crv"] = json!("P-384");
    p384["x"] = json!(b64(&[5; 48]));
    p384["y"] = json!(b64(&[6; 48]));
    let x25519 = json!({"kty": "OKP", "crv": "X25519", "kid": "es256-1", "x": b64(&[9; 32])});
    let mut weak = issuer.jwk(Signer::Rs256);
    weak["kid"] = json!("es256-1");
    let mut modulus = vec![0xff; 255];
    modulus.insert(0, 0x7f);
    weak["n"] = json!(b64(&modulus));
    let mut huge = weak.clone();
    let mut wide = vec![0xff; 1024];
    wide.insert(0, 0x01);
    huge["n"] = json!(b64(&wide));
    let oct = json!({"kty": "oct", "kid": "es256-1", "k": b64(b"shared")});
    let token = issuer.token(Signer::Es256, &claims());
    for (name, key) in [
        ("P-384", p384),
        ("X25519", x25519),
        ("RSA 2047", weak),
        ("RSA 8193", huge),
        ("oct", oct),
    ] {
        let jwks = Jwks::parse(&json!({ "keys": [key] }).to_string()).unwrap();
        let outcome = verify(&token, &config(), &jwks, ID, now());
        assert_eq!(outcome.unwrap_err(), Refusal::KeyNotFound, "{name}");
    }
    let rsa = issuer.token(Signer::Rs256, &claims());
    assert!(
        check(&issuer, &rsa).is_ok(),
        "a 2048-bit modulus is the smallest kept"
    );
}

#[test]
#[ignore = "pending E9-1"]
fn an_issuer_config_needs_an_issuer_an_audience_and_an_algorithm() {
    let es = [Algorithm::Es256];
    assert_eq!(
        IssuerConfig::new("", &[AUDIENCE], &es),
        Err(SetupError::EmptyIssuer)
    );
    assert_eq!(
        IssuerConfig::new(ISSUER, &[], &es),
        Err(SetupError::EmptyAudience)
    );
    assert_eq!(
        IssuerConfig::new(ISSUER, &[""], &es),
        Err(SetupError::EmptyAudience)
    );
    assert_eq!(
        IssuerConfig::new(ISSUER, &[AUDIENCE], &[]),
        Err(SetupError::NoAlgorithm)
    );
    assert!(IssuerConfig::new(ISSUER, &[AUDIENCE], &es).is_ok());
}

#[test]
#[ignore = "pending E9-1"]
fn a_header_in_any_but_strict_base64url_is_malformed() {
    let issuer = TestIssuer::new();
    let header = "eyJhbGciOiJFUzI1NiIsImtpZCI6ImVzMjU2LTEifQ";
    let written = r#"{"alg":"ES256","kid":"es256-1"}"#;
    let token = issuer.sign_raw(Signer::Es256, written, &claims().to_string());
    let (_, rest) = token.split_once('.').unwrap();
    assert_eq!(
        format!("{header}.{rest}"),
        token,
        "the hand-written header is the issuer's"
    );
    assert!(check(&issuer, &token).is_ok());
    for bad in [
        "eyJhbGciOiJFUzI1NiIsImtpZCI6ImVzMjU2LTEifQ==",
        "eyJhbGciOiJFUzI1NiIsImtpZCI6ImVzMjU2LTEifR",
        "eyJhbGciOiJFUzI1NiIsImtpZCI6ImVz MjU2LTEifQ",
        "eyJhbGciOiJFUzI1NiIsImtpZCI6ImVz+jU2LTEifQ",
        "eyJhbGciOiJFUzI1NiIsImtpZCI6ImVzMjU2LTEif",
    ] {
        let refusal = Refusal::Malformed {
            part: TokenPart::Header,
        };
        assert_eq!(refused(&issuer, &format!("{bad}.{rest}")), refusal, "{bad}");
    }
}

#[test]
#[ignore = "pending E9-1"]
fn an_rs256_or_eddsa_signature_of_another_length_is_refused() {
    let issuer = TestIssuer::new();
    for signer in [Signer::Rs256, Signer::EdDsa] {
        let token = issuer.token(signer, &claims());
        assert!(check(&issuer, &token).is_ok(), "{signer:?}");
        let (input, sig) = token.rsplit_once('.').unwrap();
        let raw = unb64(sig);
        let short = format!("{input}.{}", b64(&raw[..raw.len() - 1]));
        let long = format!("{input}.{}", b64(&[raw.as_slice(), &[0]].concat()));
        for (name, bad) in [("short", short), ("long", long)] {
            assert_eq!(
                refused(&issuer, &bad),
                Refusal::BadSignature,
                "{signer:?} {name}"
            );
        }
    }
}

#[test]
#[ignore = "pending E9-1"]
fn a_signed_payload_or_a_signature_in_any_but_strict_base64url_is_malformed() {
    let issuer = TestIssuer::new();
    let header = "eyJhbGciOiJFUzI1NiIsImtpZCI6ImVzMjU2LTEifQ";
    let payload = (0..3)
        .map(|n| b64(with("pad", json!("p".repeat(n))).to_string().as_bytes()))
        .find(|p| p.len() % 4 != 0)
        .unwrap();
    let good = issuer.sign_input(Signer::Es256, &format!("{header}.{payload}"));
    assert!(check(&issuer, &good).is_ok());
    let last = BASE64URL
        .iter()
        .position(|c| *c == *payload.as_bytes().last().unwrap())
        .unwrap();
    let flipped = format!(
        "{}{}",
        &payload[..payload.len() - 1],
        BASE64URL[last ^ 1] as char
    );
    for bad in [
        flipped,
        format!("+{}", &payload[1..]),
        format!("/{}", &payload[1..]),
    ] {
        let token = issuer.sign_input(Signer::Es256, &format!("{header}.{bad}"));
        assert_eq!(
            refused(&issuer, &token),
            Refusal::Malformed {
                part: TokenPart::Payload
            },
            "{bad}"
        );
    }
    let (input, sig) = good.rsplit_once('.').unwrap();
    for bad in [format!("+{}", &sig[1..]), format!("/{}", &sig[1..])] {
        let token = format!("{input}.{bad}");
        assert_eq!(
            refused(&issuer, &token),
            Refusal::Malformed {
                part: TokenPart::Signature
            },
            "{bad}"
        );
    }
}

#[test]
#[ignore = "pending E9-1"]
fn a_segment_with_nonzero_trailing_bits_is_refused() {
    let issuer = TestIssuer::new();
    for signer in Signer::ALLOWED {
        let token = issuer.token(signer, &claims());
        let end = token.len() - 1;
        let last = BASE64URL
            .iter()
            .position(|c| *c == token.as_bytes()[end])
            .unwrap();
        let mut bytes = token.into_bytes();
        bytes[end] = BASE64URL[last ^ 1];
        let flipped = String::from_utf8(bytes).unwrap();
        let refusal = Refusal::Malformed {
            part: TokenPart::Signature,
        };
        assert_eq!(refused(&issuer, &flipped), refusal, "{signer:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    #[ignore = "pending E9-1"]
    fn any_single_byte_change_is_refused(
        signer in 0..3usize,
        at in any::<prop::sample::Index>(),
        to in 0x21u8..0x7f,
        segment_end in proptest::option::of(0..3usize),
    ) {
        let issuer = TestIssuer::new();
        let token = issuer.token(Signer::ALLOWED[signer], &claims());
        let mut ends = token.match_indices('.').map(|(i, _)| i - 1).collect::<Vec<_>>();
        ends.push(token.len() - 1);
        let (i, to) = match segment_end {
            Some(segment) => {
                let i = ends[segment];
                let index = BASE64URL.iter().position(|c| *c == token.as_bytes()[i]).unwrap();
                (i, BASE64URL[index ^ 1])
            }
            None => (at.index(token.len()), to),
        };
        prop_assume!(token.as_bytes()[i] != to);
        let mut bytes = token.clone().into_bytes();
        bytes[i] = to;
        let mutated = String::from_utf8(bytes).unwrap();
        prop_assert!(check(&issuer, &token).is_ok(), "the unmutated token verifies");
        prop_assert!(check(&issuer, &mutated).is_err(), "byte {i} set to {to:#x}");
    }
}

#[test]
#[ignore = "pending E9-1"]
fn no_refusal_or_subject_prints_the_token_nonce_subject_or_a_key() {
    let issuer = TestIssuer::new();
    let good = issuer.token(Signer::Es256, &claims());
    let subject = check(&issuer, &good).unwrap();
    let shown = format!("{subject:?}");
    assert!(
        shown.contains(ISSUER),
        "a subject prints its issuer: {shown}"
    );
    let mut printed = vec![shown];
    let tokens = [
        issuer.token(Signer::ImpostorEs256, &claims()),
        issuer.token(Signer::Es256, &with("iss", json!(OTHER_TENANT))),
        issuer.token(Signer::Es256, &with("aud", json!(NONCE))),
        issuer.token(Signer::Es256, &with("nonce", json!(format!("{NONCE}-x")))),
        issuer.token(Signer::Es256, &with("exp", json!(NOW_S - 3600))),
        issuer.token(Signer::Es256, &with("sub", json!(7))),
        format!("{good}.{NONCE}"),
        format!("{NONCE}.{SUBJECT}.{EMAIL}"),
    ];
    let mut refusals = Vec::new();
    for token in &tokens {
        let refusal = check(&issuer, token).unwrap_err();
        refusals.push(format!("{refusal} {refusal:?} {}", refusal.code()));
    }
    refusals.push(format!(
        "{:?}",
        subject.matches_invitation(&format!("{EMAIL}x"))
    ));
    for error in [
        IssuerConfig::new("", &[AUDIENCE], &[Algorithm::Es256]).unwrap_err(),
        Jwks::parse(&format!("{{\"keys\": \"{NONCE}\"}}")).unwrap_err(),
    ] {
        refusals.push(format!("{error} {error:?}"));
    }
    printed.push(format!("{:?}", TokenKind::IdToken { nonce: NONCE }));
    let jwks = format!("{:?}", issuer.jwks());
    assert!(
        jwks.contains("es256-1") && jwks.contains("ed25519-1"),
        "{jwks}"
    );
    printed.push(format!("{:?} {jwks}", config()));
    let key_text =
        |signer: Signer, member: &str| issuer.jwk(signer)[member].as_str().unwrap().to_owned();
    let secrets: Vec<String> = good
        .split('.')
        .map(str::to_owned)
        .chain([NONCE, SUBJECT, EMAIL, "invitee"].map(str::to_owned))
        .chain([
            key_text(Signer::Es256, "x"),
            key_text(Signer::Es256, "y"),
            key_text(Signer::Rs256, "n"),
            key_text(Signer::EdDsa, "x"),
        ])
        .collect();
    let claims_seen = [OTHER_TENANT, AUDIENCE];
    for text in printed.iter().chain(&refusals) {
        for secret in &secrets {
            assert!(
                !text.contains(secret.as_str()),
                "a printed outcome carries a canary: {text}"
            );
        }
    }
    for text in &refusals {
        for claim in claims_seen {
            assert!(!text.contains(claim), "a refusal echoes a claim: {text}");
        }
    }
}
