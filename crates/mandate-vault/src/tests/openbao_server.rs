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
    mount_accessor, service_account_name,
};
use crate::openbao::{
    OpenBaoConfig, OpenBaoSource, RoleCredentials, provision, provision_kubernetes_auth,
};
use crate::policy::{Identity, Mounts, SecretPath, kv_path, role_name};
use crate::source::{Grant, SecretSource};
use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;
use reqwest::{Certificate, Method};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
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
    address: String,
    operator: SecretString,
}

impl Bao {
    /// One request as the operator, straight to the server: its answer, or `null` when it has
    /// none.
    async fn operator(&self, method: Method, path: &str, body: Option<Value>) -> Value {
        operator_call(
            &self.address,
            &self.config.ca_pem,
            &self.operator,
            method,
            path,
            body,
        )
        .await
    }

    /// A token under the exchange's templated policy with `connection`'s ServiceAccount put in,
    /// as a Kubernetes login would give, for a server with no Kubernetes API to review a token.
    async fn take_token(&self, connection: &ConnectionId) -> SecretString {
        let identity = Identity::TokenExchange(connection.clone());
        let mounts = &self.config.mounts;
        let template = "{{identity.entity.aliases.auth_take.metadata.service_account_name}}";
        let account = service_account_name(&identity).unwrap();
        let hcl = kubernetes_policy(&identity, mounts, "auth_take")
            .unwrap()
            .replace(template, &account);
        let name = format!(
            "{}-take-{account}",
            kubernetes_policy_name(&identity, mounts).unwrap()
        );
        self.operator(
            Method::PUT,
            &format!("sys/policies/acl/{name}"),
            Some(json!({"policy": hcl})),
        )
        .await;
        let made = self
            .operator(
                Method::POST,
                "auth/token/create",
                Some(json!({"policies": [name], "no_default_policy": true, "ttl": "600s"})),
            )
            .await;
        SecretString::from(made["auth"]["client_token"].as_str().unwrap().to_owned())
    }

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
            address,
            operator,
        })
    }
}

impl Harness for Bao {
    type Source = OpenBaoSource;

    /// The operator writes the Kubernetes-auth exchange's policy for `connection` with its
    /// ServiceAccount put in for the template, as the vault would resolve it at a login, and
    /// makes a token holding only that policy.
    async fn taker(&self, connection: &ConnectionId, now: UtcNanos) -> OpenBaoSource {
        let identity = Identity::TokenExchange(connection.clone());
        let token = self.take_token(connection).await;
        OpenBaoSource::from_token(&self.config, identity, token, at(now.secs() + 600)).unwrap()
    }

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
    operator_call(address, ca_pem, operator, Method::GET, path, None).await
}

/// `<method> /v1/<path>` as the operator: the vault's answer, or `null` when it has none.
async fn operator_call(
    address: &str,
    ca_pem: &[u8],
    operator: &SecretString,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Value {
    let _already_installed = rustls::crypto::ring::default_provider().install_default();
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

/// A config whose vault nobody answers for.
fn nowhere(ca_pem: Vec<u8>) -> OpenBaoConfig {
    OpenBaoConfig {
        address: "https://127.0.0.1:9".to_owned(),
        ca_pem,
        mounts: Mounts::default(),
    }
}

/// A take from a vault nobody answers for is an outage, never `None`: a taker that cannot reach
/// the vault has not learned that the code is gone.
async fn an_unreachable_take_is_an_outage() {
    let ca = include_bytes!("fixtures/isrg-root-x1.pem").to_vec();
    let session = OpenBaoSource::from_token(
        &nowhere(ca),
        Identity::TokenExchange(c("c1")),
        SecretString::from("unused".to_owned()),
        at(1_000_600),
    )
    .unwrap();
    assert_eq!(
        conformance::taken(&session, "c1").await,
        Err(SourceError::Unavailable)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_code_is_taken_once_and_only_by_its_own_exchange() {
    an_unreachable_take_is_an_outage().await;
    if let Some(bao) = Bao::new().await {
        conformance::a_code_is_taken_once_and_only_by_its_own_exchange(&bao).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn of_two_racing_takers_exactly_one_gets_the_code() {
    an_unreachable_take_is_an_outage().await;
    if let Some(bao) = Bao::new().await {
        conformance::of_two_racing_takers_exactly_one_gets_the_code(&bao).await;
    }
}

/// DEC-2061 item 4: a tombstone that a taker stopped between its overwrite and its delete left
/// behind is never a code, to a take or to a read.
#[tokio::test(flavor = "current_thread")]
async fn a_tombstone_is_never_taken_or_read_as_a_code() {
    an_unreachable_take_is_an_outage().await;
    let Some(bao) = Bao::new().await else {
        return;
    };
    let code = SecretPath::Code(c("c1"));
    let data = format!("{}/data/{}", bao.config.mounts.kv, kv_path(&code).unwrap());
    bao.operator(Method::POST, &data, Some(json!({"data": {"taken": true}})))
        .await;
    let taker = bao.taker(&c("c1"), at(1_000_000)).await;
    assert_eq!(conformance::taken(&taker, "c1").await, Ok(None));
    assert!(matches!(taker.read(&code).await, Ok(None)));
}

/// DEC-2061 item 4: once a code is taken no version of it is left, the one taken included, and
/// its version count is kept (DEC-2321).
#[tokio::test(flavor = "current_thread")]
async fn after_a_take_no_version_of_the_code_is_left() {
    an_unreachable_take_is_an_outage().await;
    let Some(bao) = Bao::new().await else {
        return;
    };
    let now = at(1_000_000);
    let code = SecretPath::Code(c("c1"));
    bao.source(Identity::Api, now)
        .await
        .put(&code, &conformance::secret("code-1"))
        .await
        .unwrap();
    let taker = bao.taker(&c("c1"), now).await;
    assert_eq!(
        conformance::taken(&taker, "c1").await,
        Ok(Some("code-1".to_owned()))
    );
    let data = format!("{}/data/{}", bao.config.mounts.kv, kv_path(&code).unwrap());
    for version in ["", "?version=1", "?version=2"] {
        let left = bao
            .operator(Method::GET, &format!("{data}{version}"), None)
            .await;
        assert!(left["data"]["data"].is_null(), "{version}: {left}");
    }
    let metadata = format!(
        "{}/metadata/{}",
        bao.config.mounts.kv,
        kv_path(&code).unwrap()
    );
    let kept = bao.operator(Method::GET, &metadata, None).await;
    assert_eq!(kept["data"]["current_version"].as_u64(), Some(2), "{kept}");
}

/// DEC-2321: a take keeps the code's version count, so a taker that read version 1 and stalled
/// cannot overwrite a code stored after the take by a re-connect, and that code is taken whole.
#[tokio::test(flavor = "current_thread")]
async fn a_stalled_taker_never_overwrites_a_code_stored_after_a_take() {
    an_unreachable_take_is_an_outage().await;
    let Some(bao) = Bao::new().await else {
        return;
    };
    let now = at(1_000_000);
    let code = SecretPath::Code(c("c1"));
    let api = bao.source(Identity::Api, now).await;
    api.put(&code, &conformance::secret("code-1"))
        .await
        .unwrap();
    let data = format!("{}/data/{}", bao.config.mounts.kv, kv_path(&code).unwrap());
    let stalled_read = bao.operator(Method::GET, &data, None).await;
    let stalled_version = stalled_read["data"]["metadata"]["version"]
        .as_u64()
        .unwrap();
    let taker = bao.taker(&c("c1"), now).await;
    assert_eq!(
        conformance::taken(&taker, "c1").await,
        Ok(Some("code-1".to_owned()))
    );
    api.put(&code, &conformance::secret("code-2"))
        .await
        .unwrap();
    let stalled = operator_call(
        &bao.address,
        &bao.config.ca_pem,
        &bao.take_token(&c("c1")).await,
        Method::POST,
        &data,
        Some(json!({"options": {"cas": stalled_version}, "data": {"taken": true}})),
    )
    .await;
    assert!(stalled["errors"].is_array(), "{stalled}");
    assert_eq!(
        conformance::taken(&taker, "c1").await,
        Ok(Some("code-2".to_owned()))
    );
}

/// DEC-2061 item 2: an exchange whose policy lacks the take, the AppRole one, reads its code but
/// is refused the overwrite, and the code is left for a taker that may take it.
#[tokio::test(flavor = "current_thread")]
async fn an_exchange_without_the_take_is_refused_and_leaves_the_code() {
    an_unreachable_take_is_an_outage().await;
    let Some(bao) = Bao::new().await else {
        return;
    };
    let now = at(1_000_000);
    bao.source(Identity::Api, now)
        .await
        .put(&SecretPath::Code(c("c1")), &conformance::secret("code-1"))
        .await
        .unwrap();
    let manager = bao.source(Identity::ConnectionManager, now).await;
    let grant = manager.issue_grant(&c("c1"), now).await.unwrap();
    let approle = bao.exchange(&grant, now).await.unwrap();
    assert_eq!(
        conformance::taken(&approle, "c1").await,
        Err(SourceError::Denied)
    );
    let taker = bao.taker(&c("c1"), now).await;
    assert_eq!(
        conformance::taken(&taker, "c1").await,
        Ok(Some("code-1".to_owned()))
    );
}

/// Provisioning writes each policy whole: a policy someone else left under a provisioned name is
/// replaced, not kept.
#[tokio::test(flavor = "current_thread")]
async fn provisioning_replaces_a_policy_left_under_its_name() {
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
    let exchange = Identity::TokenExchange(c("c1"));
    let name = kubernetes_policy_name(&exchange, &config.mounts).unwrap();
    let planted = "path \"secret/*\" {\n  capabilities = [\"read\"]\n}\n";
    operator_call(
        &address,
        &ca_pem,
        &operator,
        Method::PUT,
        &format!("sys/policies/acl/{name}"),
        Some(json!({"policy": planted})),
    )
    .await;
    let auth = KubernetesAuth {
        host: "https://127.0.0.1:6443".to_owned(),
        ca_pem: Some(String::from_utf8(ca_pem.clone()).unwrap()),
    };
    provision(&config, &operator, &[c("c1")], Some(&auth))
        .await
        .unwrap();
    let stored = operator_get(
        &address,
        &ca_pem,
        &operator,
        &format!("sys/policies/acl/{name}"),
    )
    .await;
    let listed = operator_get(&address, &ca_pem, &operator, "sys/auth").await;
    let accessor = mount_accessor(Some(&listed), &config.mounts.kubernetes_exchange).unwrap();
    assert_eq!(
        stored["data"]["policy"].as_str(),
        Some(
            kubernetes_policy(&exchange, &config.mounts, &accessor)
                .unwrap()
                .as_str()
        )
    );
}

/// DEC-2061 item 2: the exchange's update on its code never creates one, so where no code is
/// its write is refused and leaves nothing to take.
#[tokio::test(flavor = "current_thread")]
async fn the_exchanges_update_never_creates_a_code() {
    an_unreachable_take_is_an_outage().await;
    let Some(bao) = Bao::new().await else {
        return;
    };
    let taker = bao.taker(&c("c1"), at(1_000_000)).await;
    assert_eq!(
        taker
            .put(&SecretPath::Code(c("c1")), &conformance::secret("planted"))
            .await,
        Err(SourceError::Denied)
    );
    assert_eq!(conformance::taken(&taker, "c1").await, Ok(None));
}
