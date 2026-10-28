//! A stub token opens a session through the API server's own, unchanged path (DEC-2100 item 8;
//! invariants I5 and I6): the server fetches the stub's key set over loopback with its own
//! fetcher, verifies the token as the managed issuer's, and maps the subject through the
//! subject-link file, whose digest binds the issuer.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use mandate_api_server::clock::SystemClock;
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::journal::{Answer, Journal, JournalUnavailable, Rows};
use mandate_api_server::keys::FetchedKeys;
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::members::{Hosted, control_parser};
use mandate_api_server::reduction::Reduction;
use mandate_api_server::session::{OsRandom, Sessions};
use mandate_api_server::subject::{SubjectLinks, digest, principal_at};
use mandate_api_server::token::{Issuer as TokenIssuer, Keys};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_identity::{OrgId, WorkspaceId};

const APP: &str = "https://app.owlhead.test";
const ALICE_ID: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Y";
const MANAGED: &str = "https://ref.supabase.co/auth/v1";

/// A journal no session route reads: opening a session appends and reads nothing.
struct NoJournal;

impl Journal for NoJournal {
    fn rows<'a>(&'a self, _: &'a str) -> Rows<'a> {
        Box::pin(async { Err(JournalUnavailable) })
    }
    fn rows_from<'a>(&'a self, _: &'a str, _: mandate_journal::TrustedStart) -> Rows<'a> {
        Box::pin(async { Err(JournalUnavailable) })
    }
    fn take_ownership<'a>(&'a self, _: &'a str) -> Answer<'a, u64> {
        Box::pin(async { Err(JournalUnavailable) })
    }
    fn head<'a>(&'a self, _: &'a str) -> Answer<'a, u64> {
        Box::pin(async { Err(JournalUnavailable) })
    }
    fn event<'a>(&'a self, _: &'a str) -> Answer<'a, Option<mandate_journal::StoredEvent>> {
        Box::pin(async { Err(JournalUnavailable) })
    }
    fn append<'a>(
        &'a self,
        _: &'a str,
        _: u64,
        _: u64,
        _: mandate_time::UtcNanos,
        _: &'a [&'a [u8]],
    ) -> Answer<'a, mandate_journal::AppendOutcome> {
        Box::pin(async { Err(JournalUnavailable) })
    }
}

struct LinksAt(std::path::PathBuf);

impl SubjectLinks for LinksAt {
    fn principal(
        &self,
        issuer: &str,
        subject: &str,
    ) -> Result<mandate_identity::PrincipalId, mandate_api_server::subject::LinkRefusal> {
        principal_at(&self.0, issuer, subject)
    }
}

/// The API server's router for a cell whose one issuer is the stub at `stub`, whose subject-link
/// file holds `alice` linked under `link_issuer`, and whose audience is `audience`, served on a
/// loopback port.
fn api(
    name: &str,
    stub: &mandate_stub_issuer::issuer::Issuer,
    link_issuer: &str,
    audience: &str,
) -> std::net::SocketAddr {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("mandate-stub-issuer-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let key_file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&key_file, [0x33_u8; 32]).expect("the key");
    std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let links = dir.join("subject-links");
    std::fs::write(
        &links,
        format!("{} {ALICE_ID}\n", digest(link_issuer, "alice")),
    )
    .expect("links");
    std::fs::set_permissions(&links, std::fs::Permissions::from_mode(0o644)).expect("mode");
    let config =
        IssuerConfig::new(&stub.issuer, &[audience], &[Algorithm::Es256]).expect("the issuer");
    let fetched = Arc::new(
        FetchedKeys::new(&format!("{}/jwks.json", stub.issuer)).expect("a loopback http URL"),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    runtime
        .block_on(fetched.fetch())
        .expect("the stub's key set fetched");
    let workspace = WorkspaceId::from_ulid_text(ALICE_ID).expect("a ULID");
    let app = App {
        settings: Settings {
            app_origin: APP.to_owned(),
        },
        sessions: Arc::new(Sessions::new(
            &read_session_key(&dir).expect("the key"),
            Box::new(OsRandom::new()),
        )),
        tokens: Arc::new(TokenIssuer::new(config, fetched as Arc<dyn Keys>)),
        links: Arc::new(LinksAt(links)),
        clock: Arc::new(SystemClock),
        journal: Arc::new(NoJournal),
        members: Arc::new(control_parser(workspace)),
        hosted: Hosted {
            org: OrgId::from_ulid_text("01JB3K6F0C9R2V7N4M8Q1T5W3X").expect("a ULID"),
            workspace,
            internal_paper: false,
        },
        artifacts: None,
        objects: None,
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::default(),
        connect: None,
        reduction: Arc::new(Reduction::new(Vec::new(), Box::new(OsRandom::new()))),
        folds: Arc::default(),
    };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let addr = listener.local_addr().expect("its address");
    listener.set_nonblocking(true).expect("non-blocking");
    let router = http::api(app, Drain::new());
    std::thread::spawn(move || {
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("a listener");
            axum::serve(listener, router).await.expect("serving");
        });
    });
    addr
}

fn open_session(api: std::net::SocketAddr, token: &str) -> common::Reply {
    let headers = [
        ("Origin", APP),
        ("X-Mandate-Request", "1"),
        ("Content-Type", "application/json"),
    ];
    let body = format!(r#"{{"access_token":"{token}"}}"#);
    common::send(
        api,
        "api.test",
        "POST",
        "/v1/sessions",
        &headers,
        body.as_bytes(),
    )
}

fn stub_token(stub: std::net::SocketAddr) -> String {
    let reply = common::mint(stub, r#"{"subject":"alice"}"#);
    assert_eq!(reply.status, 200);
    reply.json()["access_token"]
        .as_str()
        .expect("a token")
        .to_owned()
}

#[test]
fn a_stub_token_signs_in_a_subject_linked_under_the_stubs_issuer() {
    let (stub, stub_addr) = common::serve("authenticated");
    let api = api("linked", &stub, &stub.issuer, "authenticated");
    let reply = open_session(api, &stub_token(stub_addr));
    assert_eq!(
        reply.status,
        204,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    assert!(
        reply
            .header("set-cookie")
            .is_some_and(|c| c.starts_with("__Host-mandate_session="))
    );
}

#[test]
fn a_subject_linked_under_another_issuer_is_the_one_401() {
    let (stub, stub_addr) = common::serve("authenticated");
    let api = api("elsewhere", &stub, MANAGED, "authenticated");
    let reply = open_session(api, &stub_token(stub_addr));
    assert_eq!(reply.status, 401);
    assert!(reply.header("set-cookie").is_none());
}

#[test]
fn a_stub_token_for_another_audience_is_the_one_401() {
    let (stub, stub_addr) = common::serve("mandate-demo");
    let api = api("audience", &stub, &stub.issuer, "authenticated");
    let reply = open_session(api, &stub_token(stub_addr));
    assert_eq!(reply.status, 401);
    assert!(reply.header("set-cookie").is_none());
}
