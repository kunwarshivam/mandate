//! The key made at start and the tokens it signs (DEC-2100 items 4, 6 and 7; invariants I3 and
//! I5), checked against the server's own verifier, `mandate_authn::verify`.

mod common;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::now_s;
use mandate_authn::{Algorithm, CLOCK_SKEW_S, IssuerConfig, Jwks, TokenKind, UtcNanos, verify};
use mandate_stub_issuer::issuer::{IssueError, Issuer, MAX_SUBJECT_BYTES, TOKEN_TTL_S};

fn issuer_at(bind: &str, audience: &str) -> Issuer {
    Issuer::new(bind.parse().expect("an address"), audience).expect("a key")
}

fn issuer() -> Issuer {
    issuer_at("127.0.0.1:8899", "authenticated")
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a time")
}

fn verified(issuer: &Issuer, keys: &Issuer, token: &str, now: i64) -> bool {
    let config = IssuerConfig::new(
        &issuer.issuer,
        &[issuer.audience.as_str()],
        &[Algorithm::Es256],
    )
    .expect("the issuer");
    let jwks = Jwks::parse(&keys.jwks).expect("a key set");
    verify(token, &config, &jwks, TokenKind::AccessToken, at(now)).is_ok()
}

fn part(token: &str, index: usize) -> serde_json::Value {
    let segment = token.split('.').nth(index).expect("a segment");
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(segment).expect("base64url")).expect("JSON")
}

#[test]
fn the_issuer_is_the_bind_exactly() {
    assert_eq!(issuer().issuer, "http://127.0.0.1:8899");
    assert_eq!(
        issuer_at("[::1]:8899", "authenticated").issuer,
        "http://[::1]:8899"
    );
}

#[test]
fn the_key_set_is_one_es256_p256_key() {
    let issuer = issuer();
    let set: serde_json::Value = serde_json::from_str(&issuer.jwks).expect("JSON");
    let keys = set["keys"].as_array().expect("keys");
    assert_eq!(keys.len(), 1);
    let key = &keys[0];
    let mut names: Vec<&str> = key
        .as_object()
        .expect("a key")
        .keys()
        .map(String::as_str)
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["alg", "crv", "kid", "kty", "use", "x", "y"]);
    assert_eq!(
        (&key["kty"], &key["crv"], &key["alg"], &key["use"]),
        (
            &"EC".into(),
            &"P-256".into(),
            &"ES256".into(),
            &"sig".into()
        )
    );
    assert_eq!(key["kid"], issuer.kid);
    assert_eq!(issuer.kid.len(), 32);
    assert!(
        issuer
            .kid
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    for coordinate in ["x", "y"] {
        let bytes = URL_SAFE_NO_PAD
            .decode(key[coordinate].as_str().expect("text"))
            .expect("base64url");
        assert_eq!(bytes.len(), 32, "{coordinate}");
    }
}

#[test]
fn a_token_verifies_as_the_servers_access_token_with_exactly_its_claims() {
    let issuer = issuer();
    let now = now_s();
    let token = issuer.token("alice", now).expect("a token");
    assert!(verified(&issuer, &issuer, &token, now));
    let header = part(&token, 0);
    assert_eq!(
        header,
        serde_json::json!({"alg": "ES256", "typ": "JWT", "kid": issuer.kid})
    );
    let claims = part(&token, 1);
    assert_eq!(
        claims,
        serde_json::json!({
            "iss": "http://127.0.0.1:8899",
            "sub": "alice",
            "aud": "authenticated",
            "iat": now,
            "nbf": now,
            "exp": now + TOKEN_TTL_S,
        })
    );
    assert_eq!(TOKEN_TTL_S, 300);
}

#[test]
fn a_token_lapses_after_its_ttl_and_the_skew() {
    let issuer = issuer();
    let now = now_s();
    let token = issuer.token("alice", now).expect("a token");
    assert!(verified(
        &issuer,
        &issuer,
        &token,
        now + TOKEN_TTL_S + CLOCK_SKEW_S - 1
    ));
    assert!(!verified(
        &issuer,
        &issuer,
        &token,
        now + TOKEN_TTL_S + CLOCK_SKEW_S + 1
    ));
}

#[test]
fn the_audience_is_the_one_given() {
    let issuer = issuer_at("127.0.0.1:8899", "mandate-demo");
    let token = issuer.token("alice", now_s()).expect("a token");
    assert_eq!(part(&token, 1)["aud"], "mandate-demo");
}

#[test]
fn each_start_makes_a_new_key_and_no_other_key_verifies_its_tokens() {
    let (first, second) = (issuer(), issuer());
    assert_ne!(first.kid, second.kid);
    assert_ne!(first.jwks, second.jwks);
    let now = now_s();
    let token = first.token("alice", now).expect("a token");
    assert!(!verified(&first, &second, &token, now));
}

#[test]
fn a_subject_that_is_empty_has_a_control_character_or_is_too_long_is_refused() {
    let issuer = issuer();
    let now = now_s();
    let long = "a".repeat(MAX_SUBJECT_BYTES + 1);
    let wide = "é".repeat(MAX_SUBJECT_BYTES / 2 + 1);
    for subject in [
        "",
        "a\nb",
        "a\u{0}",
        "\u{7f}",
        "a\u{85}",
        long.as_str(),
        wide.as_str(),
    ] {
        assert_eq!(
            issuer.token(subject, now),
            Err(IssueError::Subject),
            "{subject:?}"
        );
    }
    let longest = "a".repeat(MAX_SUBJECT_BYTES);
    for subject in ["a", "user@example.com", "é", longest.as_str()] {
        assert!(issuer.token(subject, now).is_ok(), "{subject:?}");
    }
}
