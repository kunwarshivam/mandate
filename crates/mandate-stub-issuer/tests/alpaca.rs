//! The fixture Alpaca OAuth provider's rules (DEC-2625 items 2 to 5), against a seeded random
//! source and times passed in, with codes, challenges, and forms built by the tests' own encoder.

#![cfg(debug_assertions)]

use std::os::unix::fs::PermissionsExt as _;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mandate_stub_issuer::alpaca::{
    Alpaca, CODE_TTL_S, MAX_CODES, Random, Refusal, SecretFileError, Settings, read_secret,
    settings,
};
use mandate_stub_issuer::start::Refused;
use proptest::prelude::{any, prop};
use proptest::test_runner::{Config, TestRunner};
use ring::digest::{SHA256, digest};
use ring::rand::SystemRandom;

const REDIRECT: &str = "http://127.0.0.1:8080/v1/oauth/alpaca/callback";
const SECRET: &str = "secret-canary-9f2a";
const VERIFIER: &str = "verifier-0123456789-abcdefghijklmnopqrstuvwxyz";
const T0: i64 = 1_791_000_000;

/// Each fill the next count's bytes, repeated, so each code and token differs.
#[derive(Default)]
struct Counter(AtomicU64);

impl Random for Counter {
    fn fill(&self, out: &mut [u8]) -> bool {
        let count = self.0.fetch_add(1, Ordering::Relaxed).to_le_bytes();
        for (byte, from) in out.iter_mut().zip(count.iter().cycle()) {
            *byte = *from;
        }
        true
    }
}

struct Failing;

impl Random for Failing {
    fn fill(&self, _out: &mut [u8]) -> bool {
        false
    }
}

fn provider() -> Alpaca {
    let settings = Settings {
        client_id: "client-1".to_owned(),
        client_secret: SECRET.as_bytes().to_vec(),
        redirect_uri: REDIRECT.to_owned(),
    };
    Alpaca::new(settings, Box::new(Counter::default()))
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(digest(&SHA256, verifier.as_bytes()))
}

/// RFC 3986 percent-encoding of everything but the unreserved characters.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn joined(params: &[(String, String)]) -> String {
    params
        .iter()
        .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn pairs(params: &[(&str, &str)]) -> Vec<(String, String)> {
    params
        .iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect()
}

fn authorize_params() -> Vec<(String, String)> {
    let challenge = challenge(VERIFIER);
    pairs(&[
        ("response_type", "code"),
        ("client_id", "client-1"),
        ("redirect_uri", REDIRECT),
        ("state", "st-1 &x=y"),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("scope", "trading data"),
        ("env", "paper"),
    ])
}

fn token_params(code: &str) -> Vec<(String, String)> {
    pairs(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", "client-1"),
        ("client_secret", SECRET),
        ("redirect_uri", REDIRECT),
        ("code_verifier", VERIFIER),
    ])
}

/// The code a `Location` carries, and its decoded `state`.
fn redirected(location: &str) -> (String, String) {
    let query = location
        .strip_prefix(REDIRECT)
        .and_then(|rest| rest.strip_prefix('?'))
        .expect("the registered redirect URI and a query");
    let mut code = None;
    let mut state = None;
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').expect("name=value");
        let decoded = decode(value);
        match name {
            "code" => code = Some(decoded),
            "state" => state = Some(decoded),
            other => panic!("an unexpected parameter {other}"),
        }
    }
    (code.expect("a code"), state.expect("a state"))
}

fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).expect("hex");
            out.push(u8::from_str_radix(hex, 16).expect("hex"));
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).expect("UTF-8")
}

fn code(provider: &Alpaca, now: i64) -> String {
    let location = provider
        .authorize(&joined(&authorize_params()), now)
        .expect("authorized");
    redirected(&location).0
}

fn with(mut params: Vec<(String, String)>, name: &str, value: &str) -> Vec<(String, String)> {
    for param in &mut params {
        if param.0 == name {
            param.1 = value.to_owned();
        }
    }
    params
}

fn without(params: &[(String, String)], name: &str) -> Vec<(String, String)> {
    params.iter().filter(|(n, _)| n != name).cloned().collect()
}

#[test]
fn a_code_for_the_registered_client_redeems_for_a_bearer_token_once() {
    let provider = provider();
    let location = provider
        .authorize(&joined(&authorize_params()), T0)
        .expect("authorized");
    let (code, state) = redirected(&location);
    assert_eq!(state, "st-1 &x=y");
    assert_eq!(provider.held(), 1);
    let body = provider
        .token(&joined(&token_params(&code)), T0 + 1)
        .expect("a token");
    let answer: serde_json::Value = serde_json::from_str(&body).expect("JSON");
    let members: Vec<&str> = answer
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(members, vec!["access_token", "scope", "token_type"]);
    assert_eq!(answer["token_type"], "bearer");
    assert_eq!(answer["scope"], "trading data");
    let token = answer["access_token"].as_str().expect("a token");
    assert_eq!(URL_SAFE_NO_PAD.decode(token).expect("base64url").len(), 32);
    assert_ne!(token, code);
    assert!(!body.contains(SECRET) && !body.contains(VERIFIER));
    assert_eq!(provider.held(), 0, "a redeemed code is gone");
    assert_eq!(
        provider.token(&joined(&token_params(&code)), T0 + 2),
        Err(Refusal::InvalidGrant)
    );
}

#[test]
fn every_other_authorization_request_is_refused_and_issues_no_code() {
    let base = authorize_params();
    let mut requests: Vec<Vec<(String, String)>> = Vec::new();
    for (name, _) in &base {
        requests.push(without(&base, name));
        if name != "state" {
            requests.push(with(base.clone(), name, "other"));
        }
        let mut doubled = base.clone();
        doubled.push(base.iter().find(|(n, _)| n == name).expect("named").clone());
        requests.push(doubled);
    }
    requests.push(with(base.clone(), "scope", "data trading"));
    requests.push(with(base.clone(), "scope", "trading data account:write"));
    requests.push(with(base.clone(), "env", "live"));
    requests.push(with(base.clone(), "code_challenge_method", "plain"));
    requests.push(with(base.clone(), "state", ""));
    requests.push(with(base.clone(), "code_challenge", ""));
    requests.push(with(base.clone(), "redirect_uri", &format!("{REDIRECT}/")));
    let mut extra = base.clone();
    extra.push(("prompt".to_owned(), "none".to_owned()));
    requests.push(extra);
    let provider = provider();
    for request in requests {
        let query = joined(&request);
        assert_eq!(
            provider.authorize(&query, T0),
            Err(Refusal::InvalidRequest),
            "{query}"
        );
    }
    assert_eq!(provider.authorize("%zz", T0), Err(Refusal::InvalidRequest));
    assert_eq!(provider.authorize("", T0), Err(Refusal::InvalidRequest));
    assert_eq!(provider.held(), 0);
}

#[test]
fn a_code_tried_once_is_spent_whatever_the_answer() {
    let provider = provider();
    let code = code(&provider, T0);
    let wrong = with(token_params(&code), "client_secret", "secret-canary-9f2b");
    assert_eq!(
        provider.token(&joined(&wrong), T0),
        Err(Refusal::InvalidClient)
    );
    assert_eq!(
        provider.token(&joined(&token_params(&code)), T0),
        Err(Refusal::InvalidGrant)
    );
}

/// A token form built around a fresh code.
type Form = dyn Fn(&str) -> Vec<(String, String)>;

#[test]
fn each_token_refusal_has_its_code() {
    let provider = provider();
    let cases: Vec<(Box<Form>, Refusal)> = vec![
        (
            Box::new(|c| with(token_params(c), "client_id", "client-2")),
            Refusal::InvalidClient,
        ),
        (
            Box::new(|c| with(token_params(c), "client_secret", "")),
            Refusal::InvalidClient,
        ),
        (
            Box::new(|c| with(token_params(c), "code_verifier", "another-verifier")),
            Refusal::InvalidGrant,
        ),
        (
            Box::new(|c| {
                with(
                    token_params(c),
                    "redirect_uri",
                    "http://localhost:8080/v1/oauth/alpaca/callback",
                )
            }),
            Refusal::InvalidGrant,
        ),
        (
            Box::new(|c| with(token_params(c), "grant_type", "refresh_token")),
            Refusal::InvalidRequest,
        ),
        (
            Box::new(|c| without(&token_params(c), "code_verifier")),
            Refusal::InvalidRequest,
        ),
        (
            Box::new(|c| {
                let mut params = token_params(c);
                params.push(("scope".to_owned(), "trading data".to_owned()));
                params
            }),
            Refusal::InvalidRequest,
        ),
        (
            Box::new(|c| {
                let mut params = token_params(c);
                params.push(("code".to_owned(), c.to_owned()));
                params
            }),
            Refusal::InvalidRequest,
        ),
        (
            Box::new(|_| token_params("no-such-code")),
            Refusal::InvalidGrant,
        ),
    ];
    for (make, refusal) in cases {
        let code = code(&provider, T0);
        let form = joined(&make(&code));
        assert_eq!(provider.token(&form, T0 + 1), Err(refusal), "{form}");
    }
    assert_eq!(
        provider.token("not a form %zz", T0),
        Err(Refusal::InvalidRequest)
    );
}

#[test]
fn a_code_expires_six_hundred_seconds_after_it_is_issued() {
    let provider = provider();
    let fresh = code(&provider, T0);
    let stale = code(&provider, T0);
    assert!(
        provider
            .token(&joined(&token_params(&fresh)), T0 + CODE_TTL_S - 1)
            .is_ok()
    );
    assert_eq!(
        provider.token(&joined(&token_params(&stale)), T0 + CODE_TTL_S),
        Err(Refusal::InvalidGrant)
    );
}

#[test]
fn the_store_holds_at_most_its_bound_and_drops_expired_codes_first() {
    let provider = provider();
    for _ in 0..MAX_CODES {
        code(&provider, T0);
    }
    assert_eq!(provider.held(), MAX_CODES);
    let query = joined(&authorize_params());
    assert_eq!(
        provider.authorize(&query, T0 + 1),
        Err(Refusal::Unavailable)
    );
    assert!(provider.authorize(&query, T0 + CODE_TTL_S).is_ok());
    assert_eq!(provider.held(), 1);
}

#[test]
fn a_failed_random_source_issues_nothing() {
    let settings = Settings {
        client_id: "client-1".to_owned(),
        client_secret: SECRET.as_bytes().to_vec(),
        redirect_uri: REDIRECT.to_owned(),
    };
    let provider = Alpaca::new(settings, Box::new(Failing));
    assert_eq!(
        provider.authorize(&joined(&authorize_params()), T0),
        Err(Refusal::Unavailable)
    );
    assert_eq!(provider.held(), 0);
}

#[test]
fn any_tampered_token_parameter_is_refused() {
    let names = [
        "grant_type",
        "code",
        "client_id",
        "client_secret",
        "redirect_uri",
        "code_verifier",
    ];
    let provider = provider();
    let untampered = joined(&token_params(&code(&provider, T0)));
    assert!(
        provider.token(&untampered, T0 + 1).is_ok(),
        "the untampered form redeems, so each refusal below is the tampering's"
    );
    TestRunner::new(Config::with_cases(128))
        .run(
            &(0..names.len(), prop::collection::vec(any::<u8>(), 0..16)),
            |(index, noise)| {
                let code = code(&provider, T0);
                let original = token_params(&code);
                let value = format!("{}{}", original[index].1, URL_SAFE_NO_PAD.encode(&noise));
                let value = if noise.is_empty() {
                    format!("{value}x")
                } else {
                    value
                };
                let form = joined(&with(original, names[index], &value));
                proptest::prop_assert!(provider.token(&form, T0 + 1).is_err(), "{form}");
                Ok(())
            },
        )
        .expect("every tampered parameter is refused");
}

static DIRS: AtomicU8 = AtomicU8::new(0);

fn secret_file(contents: &[u8], mode: u32) -> std::path::PathBuf {
    let n = DIRS.fetch_add(1, Ordering::Relaxed);
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("stub-alpaca-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let path = dir.join("client-secret");
    std::fs::write(&path, contents).expect("written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("mode");
    path
}

#[test]
fn the_client_secret_is_read_only_from_an_owner_only_regular_file() {
    assert_eq!(
        read_secret(&secret_file(SECRET.as_bytes(), 0o600)),
        Ok(SECRET.as_bytes().to_vec())
    );
    assert_eq!(
        read_secret(&secret_file(SECRET.as_bytes(), 0o400)),
        Ok(SECRET.as_bytes().to_vec())
    );
    assert_eq!(
        read_secret(&secret_file(SECRET.as_bytes(), 0o640)),
        Err(SecretFileError::Mode)
    );
    assert_eq!(
        read_secret(&secret_file(SECRET.as_bytes(), 0o604)),
        Err(SecretFileError::Mode)
    );
    assert_eq!(
        read_secret(&secret_file(b"", 0o600)),
        Err(SecretFileError::Empty)
    );
    let missing = secret_file(b"x", 0o600).with_file_name("absent");
    assert_eq!(read_secret(&missing), Err(SecretFileError::Unreadable));
    let dir = secret_file(b"x", 0o600)
        .parent()
        .expect("a parent")
        .to_path_buf();
    assert_eq!(read_secret(&dir), Err(SecretFileError::Unreadable));
}

#[test]
fn the_settings_are_all_three_flags_or_none() {
    let file = secret_file(SECRET.as_bytes(), 0o600);
    let file = file.to_str().expect("UTF-8");
    assert!(matches!(settings(None, None, None), Ok(None)));
    let all = settings(Some("client-1"), Some(file), Some(REDIRECT))
        .expect("all three")
        .expect("settings");
    assert_eq!(
        (
            all.client_id.as_str(),
            all.client_secret.as_slice(),
            all.redirect_uri.as_str()
        ),
        ("client-1", SECRET.as_bytes(), REDIRECT)
    );
    let some: [(Option<&str>, Option<&str>, Option<&str>); 6] = [
        (Some("client-1"), None, None),
        (None, Some(file), None),
        (None, None, Some(REDIRECT)),
        (Some("client-1"), Some(file), None),
        (Some("client-1"), None, Some(REDIRECT)),
        (None, Some(file), Some(REDIRECT)),
    ];
    for (id, path, uri) in some {
        assert!(
            matches!(settings(id, path, uri), Err(Refused::Alpaca)),
            "{id:?} {path:?} {uri:?}"
        );
    }
    let shared = secret_file(SECRET.as_bytes(), 0o640);
    let shared = shared.to_str().expect("UTF-8");
    for (id, path, uri) in [
        ("", file, REDIRECT),
        ("client-1", file, ""),
        ("client-1", shared, REDIRECT),
    ] {
        assert!(
            matches!(
                settings(Some(id), Some(path), Some(uri)),
                Err(Refused::Alpaca)
            ),
            "{id:?} {path} {uri:?}"
        );
    }
}

#[test]
fn a_full_store_is_unavailable_and_every_other_refusal_is_a_bad_request() {
    assert_eq!(Refusal::Unavailable.status(), 503);
    assert_eq!(Refusal::Unavailable.code(), "temporarily_unavailable");
    for (refusal, code) in [
        (Refusal::InvalidRequest, "invalid_request"),
        (Refusal::InvalidClient, "invalid_client"),
        (Refusal::InvalidGrant, "invalid_grant"),
    ] {
        assert_eq!((refusal.status(), refusal.code()), (400, code));
    }
}

#[test]
fn codes_and_tokens_from_the_system_random_source_differ_and_are_not_zero() {
    let settings = Settings {
        client_id: "client-1".to_owned(),
        client_secret: SECRET.as_bytes().to_vec(),
        redirect_uri: REDIRECT.to_owned(),
    };
    let provider = Alpaca::new(settings, Box::new(SystemRandom::new()));
    let codes = [code(&provider, T0), code(&provider, T0)];
    let tokens: Vec<String> = codes
        .iter()
        .map(|code| {
            let body = provider
                .token(&joined(&token_params(code)), T0 + 1)
                .expect("a token");
            let answer: serde_json::Value = serde_json::from_str(&body).expect("JSON");
            answer["access_token"].as_str().expect("a token").to_owned()
        })
        .collect();
    let drawn: Vec<&String> = codes.iter().chain(&tokens).collect();
    for (index, value) in drawn.iter().enumerate() {
        let bytes = URL_SAFE_NO_PAD.decode(value).expect("base64url");
        assert_ne!(bytes, vec![0_u8; 32], "{value} is all zero");
        assert!(
            drawn[index + 1..].iter().all(|other| other != value),
            "{value} repeats"
        );
    }
}

#[test]
fn a_redirect_uri_outside_dec_2623s_shape_is_refused_at_start() {
    let file = secret_file(SECRET.as_bytes(), 0o600);
    let file = file.to_str().expect("UTF-8");
    for uri in [
        "https://api.example.com/v1/oauth/alpaca/callback",
        "http://localhost:8080/v1/oauth/alpaca/callback",
        "http://[::1]:8080/v1/oauth/alpaca/callback",
    ] {
        let settings = settings(Some("client-1"), Some(file), Some(uri));
        assert!(matches!(settings, Ok(Some(_))), "{uri}");
    }
    for uri in [
        "http://example.com/v1/oauth/alpaca/callback",
        "http://127.0.0.1:8080/v1/oauth/alpaca/callback/",
        "http://127.0.0.1:8080/v1/oauth/alpaca/other",
        "http://127.0.0.1:8080/v1/oauth/alpaca/callback?x=1",
        "http://127.0.0.1:8080/v1/oauth/alpaca/callback#f",
        "http://user@127.0.0.1:8080/v1/oauth/alpaca/callback",
        "ftp://127.0.0.1/v1/oauth/alpaca/callback",
        "http://127.0.0.1:0/v1/oauth/alpaca/callback",
    ] {
        let settings = settings(Some("client-1"), Some(file), Some(uri));
        assert!(matches!(settings, Err(Refused::Alpaca)), "{uri}");
    }
}
