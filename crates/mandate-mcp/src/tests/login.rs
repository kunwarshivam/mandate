//! The OAuth login's PKCE and single-use `state` (E7-24, O1b part 1, DEC-855, LT-9). Nothing is
//! dialed. Every refusal is checked for the state, the code and the server's `canary`.

use std::collections::BTreeSet;

use proptest::test_runner::TestRunner;
use reqwest::Url;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};

use crate::{AuthServer, AuthorizationCode, ClientRegistration, LoopbackRedirect, PendingLogin};

/// RFC 7636 appendix B: the octets, the verifier they encode to, and its `S256` challenge.
const RFC_SEED: [u8; 32] = [
    116, 24, 223, 180, 151, 153, 224, 37, 79, 250, 96, 125, 216, 173, 187, 186, 22, 212, 37, 77,
    105, 214, 191, 240, 91, 88, 5, 88, 83, 132, 141, 121,
];
const RFC_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const RFC_CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const STATE_SEED: [u8; 32] = [7; 32];
const AUTHORIZE: &str = "https://as.test/authorize";

/// Unpadded base64url, one bit at a time: the test's own encoder, not the crate's.
fn b64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let bits: String = bytes.iter().map(|b| format!("{b:08b}")).collect();
    let sextet = |c: &[u8]| {
        let text = format!("{:0<6}", String::from_utf8(c.to_vec()).unwrap());
        char::from(ALPHABET[usize::from_str_radix(&text, 2).unwrap()])
    };
    bits.as_bytes().chunks(6).map(sextet).collect()
}

fn server(authorize: &str) -> AuthServer {
    let url = |text: &str| Url::parse(text).unwrap();
    AuthServer {
        resource: url("https://mcp.test/mcp"),
        issuer: url("https://as.test/"),
        authorization_endpoint: url(authorize),
        token_endpoint: url("https://as.test/token"),
        registration_endpoint: url("https://as.test/register"),
    }
}

fn client(port: u16) -> ClientRegistration {
    let redirect = LoopbackRedirect::new(port).unwrap();
    let client_id = "c-1".to_owned();
    ClientRegistration {
        client_id,
        redirect,
    }
}

fn begin(verifier: &[u8; 32], state: &[u8; 32]) -> (Url, PendingLogin) {
    let found = server(AUTHORIZE);
    found.begin_with(&client(49152), verifier, state).unwrap()
}

/// The one value of the query member `name`, after checking it appears exactly once.
fn member(url: &Url, name: &str) -> String {
    let values: Vec<_> = url.query_pairs().filter(|(k, _)| k == name).collect();
    assert_eq!(values.len(), 1, "{name} in {url}");
    values[0].1.to_string()
}

/// The refusal's code, after checking that no secret and no server text reached it.
fn refused(login: PendingLogin, target: &str, secrets: &[&str]) -> &'static str {
    let error = login.callback(target).unwrap_err();
    let printed = format!("{error:?} {error}");
    assert!(!printed.to_lowercase().contains("canary"), "{printed}");
    for secret in secrets {
        assert!(!printed.contains(secret), "{secret} in {printed}");
    }
    error.code()
}

#[test]
#[ignore = "pending E7-24"]
fn the_request_carries_rfc_7636s_appendix_b_challenge_and_exactly_the_flow_members() {
    for port in [1_u16, 49152, 65535] {
        let found = server(AUTHORIZE);
        let (url, login) = found
            .begin_with(&client(port), &RFC_SEED, &STATE_SEED)
            .unwrap();
        assert_eq!(url.as_str().split('?').next(), Some(AUTHORIZE));
        let state = b64url(&STATE_SEED);
        let wanted = format!(
            "client_id=c-1 code_challenge={RFC_CHALLENGE} code_challenge_method=S256 \
             redirect_uri=http://127.0.0.1:{port}/callback resource=https://mcp.test/mcp \
             response_type=code state={state}"
        );
        let mut sent: Vec<_> = url.query_pairs().map(|(k, v)| format!("{k}={v}")).collect();
        sent.sort();
        assert_eq!(sent.join(" "), wanted, "{url}");
        assert_eq!(login.verifier.expose_secret(), RFC_VERIFIER);
        assert_eq!(login.state.expose_secret(), state);
        assert_eq!(login.client, client(port));
    }
}

#[test]
#[ignore = "pending E7-24"]
fn every_seed_gives_a_43_character_verifier_and_state_and_their_s256_challenge() {
    let seed = || proptest::array::uniform32(0_u8..);
    let seeds = (seed(), seed());
    let check = |(verifier_seed, state_seed): ([u8; 32], [u8; 32])| {
        let (url, login) = begin(&verifier_seed, &state_seed);
        let verifier = b64url(&verifier_seed);
        assert_eq!(login.verifier.expose_secret(), verifier);
        assert_eq!(login.state.expose_secret(), b64url(&state_seed));
        let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
        assert_eq!(member(&url, "code_challenge"), challenge);
        Ok(())
    };
    TestRunner::default().run(&seeds, check).unwrap();
}

#[test]
#[ignore = "pending E7-24"]
fn an_authorization_endpoint_with_a_query_is_refused() {
    let plain = server(AUTHORIZE).begin_with(&client(1), &RFC_SEED, &STATE_SEED);
    assert!(plain.is_ok(), "{plain:?}");
    for authorize in [format!("{AUTHORIZE}?x=1"), format!("{AUTHORIZE}?")] {
        let begun = server(&authorize).begin_with(&client(49152), &RFC_SEED, &STATE_SEED);
        assert_eq!(begun.unwrap_err().code(), "endpoint_shape", "{authorize}");
    }
}

#[test]
#[ignore = "pending E7-24"]
fn each_login_draws_a_fresh_verifier_and_state_from_the_os() {
    let found = server(AUTHORIZE);
    let (url, a) = found.begin(&client(49152)).unwrap();
    let (_, b) = found.begin(&client(49152)).unwrap();
    let drawn = [&a.verifier, &a.state, &b.verifier, &b.state]
        .map(|secret| secret.expose_secret().to_owned());
    let base64url = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    for value in &drawn {
        assert!(value.len() == 43 && value.chars().all(base64url), "{value}");
    }
    assert_eq!(drawn.iter().collect::<BTreeSet<_>>().len(), 4, "{drawn:?}");
    let challenge = b64url(&Sha256::digest(drawn[0].as_bytes()));
    assert_eq!(member(&url, "code_challenge"), challenge);
    assert_eq!(member(&url, "state"), drawn[1]);
}

#[test]
#[ignore = "pending E7-24"]
fn the_callback_with_the_logins_own_state_yields_its_code() {
    let state = b64url(&STATE_SEED);
    let cases = [
        (format!("/callback?code=abc-123&state={state}"), "abc-123"),
        (format!("/callback?state={state}&iss=x&code=a%2Fb"), "a/b"),
    ];
    for (target, code) in cases {
        let (_, login) = begin(&RFC_SEED, &STATE_SEED);
        let granted = login.callback(&target).unwrap();
        assert_eq!(granted.code.expose_secret(), code, "{target}");
        assert_eq!(granted.verifier.expose_secret(), RFC_VERIFIER);
        assert_eq!(granted.client, client(49152));
    }
}

#[test]
#[ignore = "pending E7-24"]
fn a_state_that_differs_in_any_way_or_repeats_is_refused() {
    let own = b64url(&STATE_SEED);
    let other = b64url(&[8; 32]);
    let flipped = format!("b{}", &own[1..]);
    let (_, login) = begin(&RFC_SEED, &STATE_SEED);
    let accepted = login.callback(&format!("/callback?code=c&state={own}"));
    assert!(accepted.is_ok(), "{accepted:?}");
    let states = [
        String::new(),
        "state=".to_owned(),
        format!("state={}", &own[..42]),
        format!("state={own}A"),
        format!("state={flipped}"),
        format!("state={other}"),
        format!("state={own}&state={own}"),
        format!("state={own}&state={other}"),
        format!("STATE={own}"),
    ];
    for state in states {
        let (_, login) = begin(&RFC_SEED, &STATE_SEED);
        let target = format!("/callback?code=canary-code&{state}");
        let code = refused(login, &target, &[&own, &other]);
        assert_eq!(code, "state_mismatch", "{target}");
    }
}

#[test]
#[ignore = "pending E7-24"]
fn an_error_a_bad_path_or_a_bad_code_is_refused_unread() {
    let own = b64url(&STATE_SEED);
    let cases = [
        (
            "/callback?error=e&error_description=canary&state=@",
            "authorization_denied",
        ),
        (
            "/callback?code=c&error=access_denied&state=@",
            "authorization_denied",
        ),
        ("/callback?error=canary&code=c&state=x", "state_mismatch"),
        ("/callback?state=@", "malformed"),
        ("/callback?code=&state=@", "malformed"),
        ("/callback?code=a&code=a&state=@", "malformed"),
        ("/callback?code=a&code=b&state=@", "malformed"),
        ("/callback?code=a%20b&state=@", "malformed"),
        ("/callback?code=%C3%A9&state=@", "malformed"),
        ("/callback?code=%7F&state=@", "malformed"),
        ("/?code=canary&state=@", "malformed"),
        ("/callback/?code=canary&state=@", "malformed"),
        ("/CALLBACK?code=canary&state=@", "malformed"),
        ("//callback?code=canary&state=@", "malformed"),
        ("http://a/callback?code=canary&state=@", "malformed"),
    ];
    for (target, wanted) in cases {
        let (_, login) = begin(&RFC_SEED, &STATE_SEED);
        let target = target.replace('@', &own);
        assert_eq!(refused(login, &target, &[&own]), wanted, "{target}");
    }
}

#[test]
fn a_login_and_its_code_never_print_their_secrets() {
    let secret = |text: &str| SecretString::from(text.to_owned());
    let (state, verifier, client) = (secret("canary-s"), secret("canary-v"), client(49152));
    let login = PendingLogin {
        state,
        verifier,
        client: client.clone(),
    };
    let (code, verifier) = (secret("canary-c"), secret("canary-v"));
    let granted = AuthorizationCode {
        code,
        verifier,
        client,
    };
    for printed in [format!("{login:?}"), format!("{granted:#?}")] {
        assert!(!printed.contains("canary"), "{printed}");
        assert!(printed.contains("c-1"), "{printed}");
    }
}
