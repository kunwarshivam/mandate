//! The conformance suite over a pinned OpenBao dev server (DEC-977 item 6), when one is given:
//! `MANDATE_OPENBAO_ADDR` (its `https://` address), `MANDATE_OPENBAO_CA` (the path of its CA
//! certificate), and `MANDATE_OPENBAO_OPERATOR_TOKEN_FILE` (the path of a file holding its fake
//! dev root token; the token itself is never in the environment). CI starts one with
//! `.github/scripts/start-openbao.sh` and sets `MANDATE_OPENBAO_REQUIRED=1`, which makes a
//! missing server a failure rather than a skip.
//!
//! Each test provisions its own KV and AppRole mounts, so tests share the server without
//! sharing a secret, and reaches it through a loopback relay it can cut, to stage an outage.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "ADR-0001 ES-09 excepts tests from the safety-critical denies"
)]

use std::fs;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};

use super::conformance::{self, Harness, at, c};
use crate::SourceError;
use crate::kubernetes::{
    KubernetesAuth, kubernetes_mount_requests, kubernetes_policy, kubernetes_policy_name,
    mount_accessor,
};
use crate::openbao::{
    OpenBaoConfig, OpenBaoSource, RoleCredentials, provision, provision_kubernetes_auth,
};
use crate::policy::{Identity, Mounts, role_name};
use crate::source::Grant;
use mandate_time::UtcNanos;
use reqwest::{Certificate, Method};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

static NEXT: AtomicU32 = AtomicU32::new(0);

/// The server the environment names, or `None` when none is (and none is required).
fn server() -> Option<(String, Vec<u8>, SecretString)> {
    let required = std::env::var("MANDATE_OPENBAO_REQUIRED").is_ok_and(|v| v == "1");
    let address = std::env::var("MANDATE_OPENBAO_ADDR").ok();
    let ca = std::env::var("MANDATE_OPENBAO_CA").ok();
    let token = std::env::var("MANDATE_OPENBAO_OPERATOR_TOKEN_FILE").ok();
    match (address, ca, token) {
        (Some(address), Some(ca), Some(token)) => Some((
            address,
            fs::read(ca).unwrap(),
            SecretString::from(fs::read_to_string(token).unwrap().trim().to_owned()),
        )),
        _ if required => panic!("MANDATE_OPENBAO_REQUIRED=1 but no OpenBao server is configured"),
        _ => {
            eprintln!(
                "skipped: no MANDATE_OPENBAO_ADDR, MANDATE_OPENBAO_CA, MANDATE_OPENBAO_OPERATOR_TOKEN_FILE"
            );
            None
        }
    }
}

/// A loopback TCP relay to the server. Cutting it closes every relayed connection and refuses
/// new ones until it is restored.
struct Relay {
    address: SocketAddr,
    up: watch::Sender<bool>,
}

impl Relay {
    async fn start(target: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (up, _) = watch::channel(true);
        let state = up.subscribe();
        tokio::spawn(async move {
            loop {
                let Ok((mut inbound, _)) = listener.accept().await else {
                    return;
                };
                let mut state = state.clone();
                if !*state.borrow() {
                    continue;
                }
                tokio::spawn(async move {
                    let Ok(mut outbound) = TcpStream::connect(target).await else {
                        return;
                    };
                    tokio::select! {
                        _ = copy_bidirectional(&mut inbound, &mut outbound) => {}
                        _ = state.wait_for(|up| !*up) => {}
                    }
                });
            }
        });
        Self { address, up }
    }
}

struct Bao {
    config: OpenBaoConfig,
    credentials: Vec<RoleCredentials>,
    relay: Relay,
}

impl Bao {
    async fn new() -> Option<Self> {
        let (address, ca_pem, operator) = server()?;
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let suffix = format!("{}-{n}", std::process::id());
        let mounts = Mounts {
            kv: format!("kv-{suffix}"),
            auth: format!("approle-{suffix}"),
            kubernetes_executor: format!("kubernetes-executor-{suffix}"),
            kubernetes_exchange: format!("kubernetes-exchange-{suffix}"),
        };
        let direct = OpenBaoConfig {
            address: address.clone(),
            ca_pem: ca_pem.clone(),
            mounts: mounts.clone(),
        };
        let credentials = provision(&direct, &operator, &[c("c1"), c("c2")], None)
            .await
            .unwrap();
        let target: SocketAddr = address
            .trim_start_matches("https://")
            .parse()
            .expect("MANDATE_OPENBAO_ADDR is https://<ip>:<port>");
        let relay = Relay::start(target).await;
        let config = OpenBaoConfig {
            address: format!("https://{}", relay.address),
            ca_pem,
            mounts,
        };
        Some(Self {
            config,
            credentials,
            relay,
        })
    }
}

impl Harness for Bao {
    type Source = OpenBaoSource;

    async fn source(&self, identity: Identity, now: UtcNanos) -> OpenBaoSource {
        let role = self
            .credentials
            .iter()
            .find(|r| r.identity == identity)
            .unwrap_or_else(|| panic!("no role for {identity:?}"));
        OpenBaoSource::login(&self.config, identity, &role.role_id, &role.secret_id, now)
            .await
            .unwrap()
    }

    async fn exchange(&self, grant: &Grant, now: UtcNanos) -> Result<OpenBaoSource, SourceError> {
        OpenBaoSource::login_with_grant(&self.config, grant, now).await
    }

    async fn outage(&self) {
        self.relay.up.send_replace(false);
    }

    async fn restore(&self) {
        self.relay.up.send_replace(true);
    }
}

macro_rules! conformance {
    ($($case:ident),* $(,)?) => {
        $(
            #[tokio::test(flavor = "current_thread")]
            async fn $case() {
                if let Some(bao) = Bao::new().await {
                    conformance::$case(&bao).await;
                }
            }
        )*
    };
}

conformance!(
    the_connect_flow_runs_end_to_end,
    every_identity_is_held_to_its_policy,
    leases_and_grants_are_held_to_their_owners,
    a_grant_is_single_use_and_revocable,
    an_outage_stops_no_executor_before_its_lease_expires,
    a_revoked_connection_ends_its_lease_at_the_next_renewal,
    no_debug_or_refusal_shows_a_secret,
    after_revoke_the_session_takes_no_lease,
);

#[tokio::test(flavor = "current_thread")]
async fn the_client_trusts_only_its_ca_and_only_https() {
    let Some(bao) = Bao::new().await else {
        return;
    };
    let role = bao
        .credentials
        .iter()
        .find(|r| r.identity == Identity::Runtime)
        .unwrap();
    let now = at(1_000_000);
    let mut plain = bao.config.clone();
    plain.address = plain.address.replace("https://", "http://");
    let mut trailing = bao.config.clone();
    trailing.address.push_str("/v1");
    let mut no_ca = bao.config.clone();
    no_ca.ca_pem = b"not a certificate".to_vec();
    for config in [plain, trailing, no_ca] {
        assert_eq!(
            OpenBaoSource::login(
                &config,
                Identity::Runtime,
                &role.role_id,
                &role.secret_id,
                now
            )
            .await
            .map(drop),
            Err(SourceError::Config),
            "{config:?}"
        );
    }
    let other_ca = foreign_ca();
    let mut wrong_ca = bao.config.clone();
    wrong_ca.ca_pem = other_ca;
    assert_eq!(
        OpenBaoSource::login(
            &wrong_ca,
            Identity::Runtime,
            &role.role_id,
            &role.secret_id,
            now
        )
        .await
        .map(drop),
        Err(SourceError::Unavailable),
        "a server whose certificate does not chain to the given CA"
    );
}

/// A valid CA certificate that signed nothing the server presents: the public ISRG Root X1.
fn foreign_ca() -> Vec<u8> {
    include_bytes!("fixtures/isrg-root-x1.pem").to_vec()
}

#[tokio::test(flavor = "current_thread")]
async fn a_wrong_secret_id_or_role_is_refused() {
    let Some(bao) = Bao::new().await else {
        return;
    };
    let now = at(1_000_000);
    let executor = bao
        .credentials
        .iter()
        .find(|r| r.identity == Identity::Executor(c("c1")))
        .unwrap();
    let runtime = bao
        .credentials
        .iter()
        .find(|r| r.identity == Identity::Runtime)
        .unwrap();
    let swapped = OpenBaoSource::login(
        &bao.config,
        Identity::Executor(c("c1")),
        &executor.role_id,
        &runtime.secret_id,
        now,
    )
    .await;
    assert_eq!(swapped.map(drop), Err(SourceError::Denied));
    let claimed = OpenBaoSource::login(
        &bao.config,
        Identity::Executor(c("c1")),
        &runtime.role_id,
        &runtime.secret_id,
        now,
    )
    .await;
    assert_eq!(
        claimed.map(drop),
        Err(SourceError::Denied),
        "a login whose role is not the claimed identity's"
    );
    let session = bao.source(Identity::Runtime, now).await;
    let printed = format!("{session:?}");
    assert!(
        printed.starts_with("OpenBaoSource { identity: Runtime, address: \"https://127.0.0.1:"),
        "{printed}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_expired_grant_is_refused_before_any_login() {
    let Some(bao) = Bao::new().await else {
        return;
    };
    let now = at(1_000_000);
    let manager = bao.source(Identity::ConnectionManager, now).await;
    let grant = crate::source::SecretSource::issue_grant(&manager, &c("c1"), now)
        .await
        .unwrap();
    let late = at(1_000_000 + crate::source::GRANT_SECS);
    assert_eq!(
        bao.exchange(&grant, late).await.map(drop),
        Err(SourceError::GrantInvalid)
    );
    assert!(
        bao.exchange(&grant, now).await.is_ok(),
        "the refusal spent nothing"
    );
}

/// DEC-2020: the platform's own secrets, on OpenBao. The table is checked first, so a run with
/// no server still fails until it holds.
#[tokio::test(flavor = "current_thread")]
async fn a_platform_secret_is_written_by_import_and_read_by_its_one_process() {
    conformance::the_platform_secrets_have_their_holders();
    if let Some(bao) = Bao::new().await {
        conformance::a_platform_secret_is_written_by_import_and_read_by_its_one_process(&bao).await;
    }
}

/// DEC-2020: no output shows a platform secret, on OpenBao.
#[tokio::test(flavor = "current_thread")]
async fn no_debug_or_refusal_shows_a_platform_secret() {
    conformance::the_platform_secrets_have_their_holders();
    if let Some(bao) = Bao::new().await {
        conformance::no_debug_or_refusal_shows_a_platform_secret(&bao).await;
    }
}

/// `GET /v1/<path>` as the operator, the vault's answer.
async fn operator_get(address: &str, ca_pem: &[u8], operator: &SecretString, path: &str) -> Value {
    reqwest::Client::builder()
        .add_root_certificate(Certificate::from_pem(ca_pem).unwrap())
        .build()
        .unwrap()
        .get(format!("{address}/v1/{path}"))
        .header("X-Vault-Token", operator.expose_secret())
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .map(|body| serde_json::from_slice(&body).unwrap())
        .unwrap()
}

/// DEC-2060 items 4 and 7 on a real vault: provisioning, twice, enables both Kubernetes mounts,
/// points each at the API server, and writes each kind's templated policy keyed by its mount's
/// own accessor; a login to a role not yet made is `RoleMissing`, which a process may wait out.
/// The pure requests are checked first, so a run with no server still fails until they hold.
#[tokio::test(flavor = "current_thread")]
async fn provisioning_sets_both_kubernetes_mounts_up_with_their_templated_policies() {
    let auth = |ca_pem: &[u8]| KubernetesAuth {
        host: "https://127.0.0.1:6443".to_owned(),
        ca_pem: Some(String::from_utf8(ca_pem.to_vec()).unwrap()),
    };
    let listed = kubernetes_mount_requests(&Mounts::default(), &auth(PEM_SHAPE));
    assert!(listed.is_ok(), "{listed:?}");
    let Some((address, ca_pem, operator)) = server() else {
        return;
    };
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let suffix = format!("{}-{n}", std::process::id());
    let config = OpenBaoConfig {
        address: address.clone(),
        ca_pem: ca_pem.clone(),
        mounts: Mounts {
            kv: format!("kv-{suffix}"),
            auth: format!("approle-{suffix}"),
            kubernetes_executor: format!("kubernetes-executor-{suffix}"),
            kubernetes_exchange: format!("kubernetes-exchange-{suffix}"),
        },
    };
    for round in 0..2 {
        let provisioned = provision(&config, &operator, &[c("c1")], Some(&auth(&ca_pem))).await;
        assert!(
            provisioned.is_ok(),
            "round {round}: {:?}",
            provisioned.map(drop)
        );
    }
    let mounts = &config.mounts;
    let listed = operator_get(&address, &ca_pem, &operator, "sys/auth").await;
    for (identity, mount) in [
        (Identity::Executor(c("c1")), &mounts.kubernetes_executor),
        (
            Identity::TokenExchange(c("c1")),
            &mounts.kubernetes_exchange,
        ),
    ] {
        assert_eq!(listed["data"][format!("{mount}/")]["type"], "kubernetes");
        let accessor = mount_accessor(Some(&listed), mount).unwrap();
        let name = kubernetes_policy_name(&identity, mounts).unwrap();
        let stored = operator_get(
            &address,
            &ca_pem,
            &operator,
            &format!("sys/policies/acl/{name}"),
        )
        .await;
        assert_eq!(
            stored["data"]["policy"].as_str(),
            Some(
                kubernetes_policy(&identity, mounts, &accessor)
                    .unwrap()
                    .as_str()
            ),
            "{identity:?}"
        );
        let configured = operator_get(
            &address,
            &ca_pem,
            &operator,
            &format!("auth/{mount}/config"),
        )
        .await;
        assert_eq!(
            configured["data"]["kubernetes_host"],
            "https://127.0.0.1:6443"
        );
    }
    let login = OpenBaoSource::login_kubernetes(
        &config,
        Identity::Executor(c("c1")),
        &SecretString::from("x.y.z".to_owned()),
        "ws-ws1",
        at(1_000_000),
    )
    .await;
    assert_eq!(login.map(drop), Err(SourceError::RoleMissing));
}

/// A CA certificate's shape, for the pure check that runs without a server.
/// One request as the operator: its answer, or `null` when it has none.
async fn operator_request(
    address: &str,
    ca_pem: &[u8],
    operator: &SecretString,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Value {
    let mut request = reqwest::Client::builder()
        .add_root_certificate(Certificate::from_pem(ca_pem).unwrap())
        .build()
        .unwrap()
        .request(method, format!("{address}/v1/{path}"))
        .header("X-Vault-Token", operator.expose_secret());
    if let Some(body) = body {
        request = request.body(body.to_string());
    }
    let answer = request.send().await.unwrap().bytes().await.unwrap();
    if answer.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&answer).unwrap()
    }
}

/// Kubernetes auth alone, on a vault whose AppRole credentials are already in use: a mount that
/// is there is kept, the one missing is added, both are pointed at the host given last, both
/// policies are written, and no secret id is minted, so no credential is rotated. A vault no one
/// answers for is an outage, checked first, so a run with no server fails the same way.
#[tokio::test(flavor = "current_thread")]
async fn kubernetes_auth_alone_adds_only_the_missing_mount_and_mints_no_secret_id() {
    let auth = |ca_pem: &[u8], host: &str| KubernetesAuth {
        host: host.to_owned(),
        ca_pem: Some(String::from_utf8(ca_pem.to_vec()).unwrap()),
    };
    let nowhere = OpenBaoConfig {
        address: "https://127.0.0.1:9".to_owned(),
        ca_pem: foreign_ca(),
        mounts: Mounts::default(),
    };
    assert_eq!(
        provision_kubernetes_auth(
            &nowhere,
            &SecretString::from("unused".to_owned()),
            &auth(PEM_SHAPE, "https://127.0.0.1:6443"),
        )
        .await,
        Err(SourceError::Unavailable)
    );
    let Some((address, ca_pem, operator)) = server() else {
        return;
    };
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let suffix = format!("{}-{n}", std::process::id());
    let config = OpenBaoConfig {
        address: address.clone(),
        ca_pem: ca_pem.clone(),
        mounts: Mounts {
            kv: format!("kv-{suffix}"),
            auth: format!("approle-{suffix}"),
            kubernetes_executor: format!("kubernetes-executor-{suffix}"),
            kubernetes_exchange: format!("kubernetes-exchange-{suffix}"),
        },
    };
    let mounts = &config.mounts;
    provision(&config, &operator, &[c("c1")], None)
        .await
        .unwrap();
    let secret_ids = |listed: Value| listed["data"]["keys"].as_array().map_or(0, Vec::len);
    let list = Method::from_bytes(b"LIST").unwrap();
    let issued: Vec<(Identity, usize)> = {
        let mut issued = Vec::new();
        for identity in [Identity::Api, Identity::Executor(c("c1"))] {
            let path = format!(
                "auth/{}/role/{}/secret-id",
                mounts.auth,
                role_name(&identity)
            );
            issued.push((
                identity,
                secret_ids(
                    operator_request(&address, &ca_pem, &operator, list.clone(), &path, None).await,
                ),
            ));
        }
        issued
    };
    assert!(issued.iter().all(|(_, count)| *count == 1), "{issued:?}");
    operator_request(
        &address,
        &ca_pem,
        &operator,
        Method::POST,
        &format!("sys/auth/{}", mounts.kubernetes_executor),
        Some(serde_json::json!({"type": "kubernetes"})),
    )
    .await;
    let before = operator_get(&address, &ca_pem, &operator, "sys/auth").await;
    let kept = mount_accessor(Some(&before), &mounts.kubernetes_executor).unwrap();
    assert!(mount_accessor(Some(&before), &mounts.kubernetes_exchange).is_err());
    for host in ["https://127.0.0.1:6443", "https://127.0.0.1:7443"] {
        let set = provision_kubernetes_auth(&config, &operator, &auth(&ca_pem, host)).await;
        assert_eq!(set, Ok(()), "{host}");
    }
    let listed = operator_get(&address, &ca_pem, &operator, "sys/auth").await;
    assert_eq!(
        mount_accessor(Some(&listed), &mounts.kubernetes_executor),
        Ok(kept)
    );
    for (identity, mount) in [
        (Identity::Executor(c("c1")), &mounts.kubernetes_executor),
        (
            Identity::TokenExchange(c("c1")),
            &mounts.kubernetes_exchange,
        ),
    ] {
        assert_eq!(listed["data"][format!("{mount}/")]["type"], "kubernetes");
        let accessor = mount_accessor(Some(&listed), mount).unwrap();
        let name = kubernetes_policy_name(&identity, mounts).unwrap();
        let stored = operator_get(
            &address,
            &ca_pem,
            &operator,
            &format!("sys/policies/acl/{name}"),
        )
        .await;
        assert_eq!(
            stored["data"]["policy"].as_str(),
            Some(
                kubernetes_policy(&identity, mounts, &accessor)
                    .unwrap()
                    .as_str()
            ),
            "{identity:?}"
        );
        let configured = operator_get(
            &address,
            &ca_pem,
            &operator,
            &format!("auth/{mount}/config"),
        )
        .await;
        assert_eq!(
            configured["data"]["kubernetes_host"],
            "https://127.0.0.1:7443"
        );
    }
    for (identity, count) in issued {
        let path = format!(
            "auth/{}/role/{}/secret-id",
            mounts.auth,
            role_name(&identity)
        );
        assert_eq!(
            secret_ids(
                operator_request(&address, &ca_pem, &operator, list.clone(), &path, None).await
            ),
            count,
            "{identity:?}"
        );
    }
}

const PEM_SHAPE: &[u8] = b"-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";
