//! The adapter over a local fixture endpoint, driven through `exchange_code`: one `POST` of the
//! exact form to the token URL, no redirect followed, and nothing read past the answer's limit
//! (DEC-2629 item 2). It is a cell adapter (DEC-2629 item 3) on loopback; no test sends to Alpaca.

#![allow(
    clippy::disallowed_methods,
    reason = "two tests measure that a refusal does not wait out the timeout; no product code reads the clock"
)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use secrecy::{ExposeSecret, SecretString};

use super::vault;
use super::{Waitless, deadline};
use crate::cell::parse;
use crate::http::{HttpTokenEndpoint, Target};
use crate::response::MAX_BODY;
use crate::run::run;
use crate::startup::{Endpoint, Settings};
use mandate_connections::ConnectError;
use mandate_connections::exchange::{ClientSecret, ExchangedGrant, PendingConnect, exchange_code};
use mandate_connections::grant::{GrantedScopes, check_scope};
use mandate_connections::start::{ClientId, Environment, OAuthClient, PkceVerifier, RedirectUri};
use mandate_connections::token_entry;
use mandate_connections::vault::{AccessToken, AuthorizationCode, Vault, VaultKey, WaitingCode};
use mandate_vault::policy::SecretPath;

const TOKEN_PATH: &str = "/alpaca/oauth/token";
const GRANT: &str = r#"{"access_token":"tok-CANARY","scope":"trading data","token_type":"bearer"}"#;

/// The form for this file's values, written here rather than by `form::body`.
const EXPECTED_BODY: &str = "grant_type=authorization_code&code=code-CANARY\
    &redirect_uri=https%3A%2F%2Fapi.owlhead.ai%2Fv1%2Foauth%2Falpaca%2Fcallback\
    &client_id=client-1&client_secret=secret-CANARY&code_verifier=verifier-CANARY";

/// One request the fixture endpoint received.
#[derive(Debug, Clone)]
struct Hit {
    method: Method,
    path: String,
    query: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}

/// What the fixture endpoint answers at [`TOKEN_PATH`].
#[derive(Clone, Copy)]
enum Answer {
    Grant,
    Redirect(u16),
    Status(u16),
    Oversized,
    AtLimit,
}

#[derive(Clone)]
struct Fixture {
    answer: Answer,
    hits: Arc<Mutex<Vec<Hit>>>,
}

async fn answered(
    State(fixture): State<Fixture>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    fixture.hits.lock().unwrap().push(Hit {
        method,
        path: uri.path().to_owned(),
        query: uri.query().map(str::to_owned),
        content_type,
        body: body.to_vec(),
    });
    let json = [(header::CONTENT_TYPE, "application/json")];
    if uri.path() != TOKEN_PATH {
        return (StatusCode::OK, json, GRANT).into_response();
    }
    match fixture.answer {
        Answer::Grant => (StatusCode::OK, json, GRANT).into_response(),
        Answer::Redirect(code) => (
            StatusCode::from_u16(code).unwrap(),
            [(header::LOCATION, "/elsewhere")],
        )
            .into_response(),
        Answer::Status(code) => (
            StatusCode::from_u16(code).unwrap(),
            json,
            r#"{"error":"invalid_grant"}"#,
        )
            .into_response(),
        Answer::AtLimit => {
            let padded = format!("{GRANT}{}", " ".repeat(MAX_BODY - GRANT.len()));
            (StatusCode::OK, json, padded).into_response()
        }
        Answer::Oversized => {
            let padded = format!("{GRANT}{}", " ".repeat(1 << 20));
            (StatusCode::OK, json, padded).into_response()
        }
    }
}

/// Serves `answer` on a loopback port from a thread of its own, so the adapter's own runtime can
/// block on its request.
fn serve(answer: Answer) -> (SocketAddr, Arc<Mutex<Vec<Hit>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(Mutex::new(Vec::new()));
    let fixture = Fixture {
        answer,
        hits: Arc::clone(&hits),
    };
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let app = Router::new().fallback(answered).with_state(fixture);
            axum::serve(listener, app).await.unwrap();
        });
    });
    (addr, hits)
}

/// The vault the exchange reads its code from and stores its token in.
#[derive(Default)]
struct MemVault {
    codes: BTreeMap<VaultKey, WaitingCode>,
    tokens: BTreeMap<VaultKey, String>,
}

impl Vault for MemVault {
    fn put_code(
        &mut self,
        key: &VaultKey,
        code: AuthorizationCode,
        verifier: PkceVerifier,
    ) -> Result<(), ConnectError> {
        self.codes
            .insert(key.clone(), WaitingCode { code, verifier });
        Ok(())
    }

    fn take_code(&mut self, key: &VaultKey) -> Result<Option<WaitingCode>, ConnectError> {
        Ok(self.codes.remove(key))
    }

    fn put_token(
        &mut self,
        key: &VaultKey,
        token: AccessToken,
        _scopes: &GrantedScopes,
    ) -> Result<(), ConnectError> {
        self.tokens
            .insert(key.clone(), token.0.expose_secret().to_owned());
        Ok(())
    }

    fn delete_code(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        self.codes.remove(key);
        Ok(())
    }

    fn delete(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        self.codes.remove(key);
        self.tokens.remove(key);
        Ok(())
    }
}

fn key() -> VaultKey {
    VaultKey("connections/conn-1".to_owned())
}

/// Runs one exchange against `answer`, returning its result, what the endpoint received, and
/// the vault after it.
fn exchanged(answer: Answer) -> (Result<ExchangedGrant, ConnectError>, Vec<Hit>, MemVault) {
    let (addr, hits) = serve(answer);
    let result = exchange_at(addr);
    let seen = hits.lock().unwrap().clone();
    (result.0, seen, result.1)
}

/// Runs one exchange against whatever answers on `addr`, returning its result and the vault.
fn exchange_at(addr: SocketAddr) -> (Result<ExchangedGrant, ConnectError>, MemVault) {
    let url = parse(Some("ci"), &format!("http://{addr}{TOKEN_PATH}")).unwrap();
    let mut endpoint = HttpTokenEndpoint::for_cell(url).unwrap();
    let mut vault = MemVault::default();
    vault
        .put_code(
            &key(),
            AuthorizationCode(SecretString::from("code-CANARY")),
            PkceVerifier(SecretString::from("verifier-CANARY")),
        )
        .unwrap();
    let pending = PendingConnect {
        connection_id: "conn-1".to_owned(),
        environment: Environment::Paper,
        vault_key: key(),
    };
    let client = OAuthClient {
        client_id: ClientId("client-1".to_owned()),
        redirect_uri: RedirectUri::production(),
    };
    let secret = ClientSecret(SecretString::from("secret-CANARY"));
    let result = exchange_code(&pending, &client, &secret, &mut vault, &mut endpoint);
    (result, vault)
}

/// A raw endpoint that reads the request, writes `head`, then writes `chunk` until the client
/// hangs up (or, with an empty `chunk`, holds the connection open without a body).
fn raw(head: &'static str, chunk: &'static [u8]) -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _read = stream.read(&mut request);
        if stream.write_all(head.as_bytes()).is_err() {
            return;
        }
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(30) {
            if chunk.is_empty() {
                std::thread::sleep(Duration::from_millis(50));
                let mut probe = [0_u8; 1];
                stream
                    .set_read_timeout(Some(Duration::from_millis(50)))
                    .unwrap();
                if matches!(stream.read(&mut probe), Ok(0)) {
                    return;
                }
            } else if stream.write_all(chunk).is_err() {
                return;
            }
        }
    });
    addr
}

/// An answer of exactly [`MAX_BODY`] bytes is read whole over HTTP.
#[test]
fn an_answer_at_the_limit_is_read() {
    let (result, hits, vault) = exchanged(Answer::AtLimit);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(hits.len(), 1);
    assert_eq!(
        vault.tokens.get(&key()).map(String::as_str),
        Some("tok-CANARY")
    );
}

/// An answer that declares more than [`MAX_BODY`] bytes is refused on its declaration, without
/// waiting for a body that never comes.
#[test]
fn a_declared_oversized_answer_is_refused_before_its_body() {
    let addr = raw(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1048576\r\n\r\n",
        b"",
    );
    let started = Instant::now();
    let (result, _) = exchange_at(addr);
    assert_eq!(result.map(|_| ()), Err(ConnectError::EndpointFailed));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

/// An answer that never ends is refused once it passes [`MAX_BODY`], not read to the timeout.
#[test]
fn an_endless_answer_is_refused_once_past_the_limit() {
    let addr = raw(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
        b"                                                                ",
    );
    let started = Instant::now();
    let (result, _) = exchange_at(addr);
    assert_eq!(result.map(|_| ()), Err(ConnectError::EndpointFailed));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn the_exchange_posts_the_exact_form_once_to_the_token_url() {
    let (result, hits, vault) = exchanged(Answer::Grant);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(hits.len(), 1, "one request");
    let hit = &hits[0];
    assert_eq!(hit.method, Method::POST);
    assert_eq!(hit.path, TOKEN_PATH);
    assert_eq!(hit.query, None);
    assert_eq!(
        hit.content_type.as_deref(),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(String::from_utf8(hit.body.clone()).unwrap(), EXPECTED_BODY);
    assert_eq!(
        vault.tokens.get(&key()).map(String::as_str),
        Some("tok-CANARY")
    );
}

/// A redirect is never followed: the exchange fails, the redirect's target is never asked, and
/// the consumed code is not sent again (DEC-2628).
#[test]
fn a_redirect_is_not_followed() {
    for code in [301, 302, 303, 307, 308] {
        let (result, hits, vault) = exchanged(Answer::Redirect(code));
        assert_eq!(
            result.map(|_| ()),
            Err(ConnectError::EndpointFailed),
            "{code}"
        );
        let paths: Vec<&str> = hits.iter().map(|hit| hit.path.as_str()).collect();
        assert_eq!(paths, [TOKEN_PATH], "{code}: only the token URL is asked");
        assert!(vault.tokens.is_empty(), "{code}");
        assert!(
            vault.codes.is_empty(),
            "{code}: consumed before the request"
        );
    }
}

#[test]
fn a_refusal_or_an_oversized_answer_is_endpoint_failed() {
    for answer in [Answer::Status(400), Answer::Status(500), Answer::Oversized] {
        let (result, hits, vault) = exchanged(answer);
        assert_eq!(result.map(|_| ()), Err(ConnectError::EndpointFailed));
        assert_eq!(hits.len(), 1, "never retried");
        assert!(vault.tokens.is_empty());
    }
}

/// An endpoint nothing listens on fails the exchange, and no error names a secret.
#[test]
fn an_unreachable_endpoint_is_endpoint_failed_and_names_no_secret() {
    let addr = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let url = parse(Some("dev"), &format!("http://{addr}{TOKEN_PATH}")).unwrap();
    let mut endpoint = HttpTokenEndpoint::for_cell(url).unwrap();
    let mut vault = MemVault::default();
    vault
        .put_code(
            &key(),
            AuthorizationCode(SecretString::from("code-CANARY")),
            PkceVerifier(SecretString::from("verifier-CANARY")),
        )
        .unwrap();
    let pending = PendingConnect {
        connection_id: "conn-1".to_owned(),
        environment: Environment::Paper,
        vault_key: key(),
    };
    let client = OAuthClient {
        client_id: ClientId("client-1".to_owned()),
        redirect_uri: RedirectUri::production(),
    };
    let secret = ClientSecret(SecretString::from("secret-CANARY"));
    let error = exchange_code(&pending, &client, &secret, &mut vault, &mut endpoint).unwrap_err();
    assert_eq!(error, ConnectError::EndpointFailed);
    let printed = format!("{error:?} {error}");
    assert!(!printed.contains("CANARY"), "{printed}");
}

#[test]
fn the_live_adapter_builds_without_sending_anything() {
    assert!(HttpTokenEndpoint::live().is_ok());
}

/// The live adapter's client refuses a plain `http` URL before anything is sent.
#[test]
fn the_live_adapter_refuses_plain_http() {
    let (addr, hits) = serve(Answer::Grant);
    let endpoint = HttpTokenEndpoint::live().unwrap();
    let sent = endpoint.runtime.block_on(
        endpoint
            .client
            .post(format!("http://{addr}{TOKEN_PATH}"))
            .send(),
    );
    assert!(sent.is_err(), "https only");
    assert!(
        hits.lock().unwrap().is_empty(),
        "nothing reached the endpoint"
    );
}

/// An endpoint that accepts the request and never answers fails the exchange at the adapter's
/// total timeout, here a short one, instead of holding the process.
#[test]
fn a_hung_endpoint_fails_at_the_total_timeout() {
    let addr = raw("", b"");
    let url = parse(Some("ci"), &format!("http://{addr}{TOKEN_PATH}")).unwrap();
    let mut endpoint =
        HttpTokenEndpoint::built(false, Target::Cell(url), Duration::from_millis(300)).unwrap();
    let mut vault = MemVault::default();
    vault
        .put_code(
            &key(),
            AuthorizationCode(SecretString::from("code-CANARY")),
            PkceVerifier(SecretString::from("verifier-CANARY")),
        )
        .unwrap();
    let pending = PendingConnect {
        connection_id: "conn-1".to_owned(),
        environment: Environment::Paper,
        vault_key: key(),
    };
    let client = OAuthClient {
        client_id: ClientId("client-1".to_owned()),
        redirect_uri: RedirectUri::production(),
    };
    let secret = ClientSecret(SecretString::from("secret-CANARY"));
    let started = Instant::now();
    let result = exchange_code(&pending, &client, &secret, &mut vault, &mut endpoint);
    assert_eq!(result.map(|_| ()), Err(ConnectError::EndpointFailed));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

/// The whole exchange after the start checks (DEC-2330 item 5): `run` through the vault adapter
/// over a scripted session and the real HTTP adapter against the loopback fixture provider. The
/// client secret is read, then the code entry taken, then the one request sent with both, then the
/// token entry written at the connection's token, and nothing else asked of the vault.
#[test]
fn the_exchange_runs_through_the_vault_adapter_and_the_http_adapter() {
    let (addr, hits) = serve(Answer::Grant);
    let url = format!("http://{addr}{TOKEN_PATH}");
    let settings = Settings {
        connection: vault::connection(vault::CONNECTION),
        endpoint: Endpoint::Cell(parse(Some("ci"), &url).unwrap()),
        client: OAuthClient {
            client_id: ClientId("client-1".to_owned()),
            redirect_uri: RedirectUri::production(),
        },
    };
    let script = vault::Script::waiting();
    let adapter = vault::adapter(&script);
    let mut endpoint = HttpTokenEndpoint::for_cell(parse(Some("ci"), &url).unwrap()).unwrap();
    let grant = run(
        &settings,
        &mut &adapter,
        &mut &adapter,
        &mut endpoint,
        &mut Waitless,
        deadline(),
    )
    .unwrap();
    assert_eq!(grant.connection_id, vault::CONNECTION);
    let hits = hits.lock().unwrap().clone();
    assert_eq!(hits.len(), 1, "one request");
    let body = String::from_utf8(hits[0].body.clone()).unwrap();
    for sent in [
        format!("code={}", vault::CODE),
        format!("code_verifier={}", vault::VERIFIER),
        format!("client_secret={}", vault::CLIENT_SECRET),
        "client_id=client-1".to_owned(),
    ] {
        assert!(body.contains(&sent), "{sent} in {body}");
    }
    let scopes = check_scope("trading data").unwrap();
    let token =
        token_entry::encode(&AccessToken(SecretString::from("tok-CANARY")), &scopes).unwrap();
    assert_eq!(
        script.calls(),
        vec![
            vault::Call::Read(SecretPath::ClientSecret),
            vault::Call::Take(vault::connection(vault::CONNECTION)),
            vault::Call::Put(
                SecretPath::Token(vault::connection(vault::CONNECTION)),
                token.expose_secret().as_bytes().to_vec()
            ),
        ]
    );
}
