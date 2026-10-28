//! OpenBao as a [`SecretSource`] (DEC-434 item 14, DEC-977), over its HTTP API, over TLS only,
//! trusting only the CA certificate it is given, through no proxy and with no redirect.
//!
//! Each process logs in with its own AppRole ([`crate::policy::role_name`]) and holds the token
//! that login returns, with the one policy [`crate::policy::openbao_policy`] gives its identity.
//! The client never checks a policy itself: OpenBao refuses, so the conformance suite proves the
//! server's policies, not this code. Secrets live in the KV v2 mount of [`crate::policy::Mounts`], as
//! `{"value": <base64>}`.
//!
//! Before it logs in, the client reads `sys/seal-status`: a sealed vault is
//! [`SourceError::Sealed`], since unsealing is an operator's step (DEC-977 item 5), and a dev
//! server (in-memory storage) is [`SourceError::DevServerRefused`] outside this crate's own
//! unit tests.
//!
//! A grant is a single-use AppRole secret id for that connection's exchange role, valid
//! [`crate::source::GRANT_SECS`], whose token lives no longer.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mandate_connections::account_entry::BrokerAccountId;
use mandate_connections::record::{AccountPiiRef, AccountRef, ConnectionId};
use mandate_time::UtcNanos;
use reqwest::{Certificate, Method, StatusCode, redirect};
use secrecy::zeroize::Zeroize;
use secrecy::{ExposeSecret, SecretSlice, SecretString};
use serde_json::{Value, json};

use crate::SourceError;
use crate::fingerprint::{
    AccountFingerprint, Claim, FINGERPRINT_KEY, FingerprintError, ManagerFingerprints, decode_entry,
};
use crate::kubernetes::{
    KubernetesAuth, kubernetes_mount_requests, kubernetes_policy, kubernetes_policy_name,
    mount_accessor, provisioner_policy, role_request, service_account_name,
};
use crate::lease::{LEASE_SECS, Lease, lease_times};
use crate::policy::{
    Access, Identity, Mounts, SecretPath, kv_path, openbao_policy, policy_name, require, role_name,
};
use crate::provisioner::{RoleProvisioner, RoleRefusal};
use crate::registry::{Registry, claim_in, release_in};
use crate::source::{GRANT_SECS, Grant, SecretSource};

/// How long one request may take before the vault counts as unreachable.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// The longest an executor's token lives through renewals before it logs in again: 30 days.
pub(crate) const EXECUTOR_TOKEN_MAX_SECS: i64 = 2_592_000;
/// Whether this build may use a dev server: only this crate's own unit-test build, so no other
/// crate's build, test or not, can turn it on (DEC-977 item 6).
const ALLOW_DEV_SERVER: bool = cfg!(test);

/// Where the vault is and how to trust it.
#[derive(Clone)]
pub struct OpenBaoConfig {
    /// `https://host:port`, nothing after it.
    pub address: String,
    /// The CA certificate (PEM) the server's certificate must chain to; the only root trusted.
    pub ca_pem: Vec<u8>,
    /// Where the secrets and roles live.
    pub mounts: Mounts,
}

impl fmt::Debug for OpenBaoConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenBaoConfig")
            .field("address", &self.address)
            .field("mounts", &self.mounts)
            .finish_non_exhaustive()
    }
}

/// A request the vault refused (E21-32v): its method, its path under `/v1/`, and its status. It
/// never holds a body, a token, or a secret id: a path names only mounts, policies, roles, and
/// keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub method: String,
    pub path: String,
    pub status: u16,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} /v1/{} answered {}",
            self.method, self.path, self.status
        )
    }
}

/// Why provisioning stopped: the refusal, and the request the vault refused when a refusal of the
/// vault's own is what stopped it, so the provisioner's output names the status and the path
/// (E21-32v). A refusal the provisioning expected and went past, a mount not there yet, is never
/// named for a later failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{error}")]
pub struct ProvisionError {
    pub error: SourceError,
    pub refused: Option<Refused>,
}

impl From<SourceError> for ProvisionError {
    fn from(error: SourceError) -> Self {
        Self {
            error,
            refused: None,
        }
    }
}

/// The HTTP client for one vault: TLS to the given CA only, no proxy, no redirect.
pub(crate) struct Http {
    client: reqwest::Client,
    address: String,
    /// The last request's refusal, if the vault refused it, and what it was judged to mean; reset
    /// at each request, so a request that never reached an answer leaves none.
    last: Mutex<Option<(Refused, SourceError)>>,
}

impl Http {
    pub(crate) fn new(config: &OpenBaoConfig) -> Result<Self, SourceError> {
        let rest = config
            .address
            .strip_prefix("https://")
            .ok_or(SourceError::Config)?;
        if rest.is_empty() || rest.contains(['/', '?', '#', '@']) {
            return Err(SourceError::Config);
        }
        let certs =
            Certificate::from_pem_bundle(&config.ca_pem).map_err(|_| SourceError::Config)?;
        if certs.is_empty() {
            return Err(SourceError::Config);
        }
        let _already_installed = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .tls_certs_only(certs)
            .https_only(true)
            .redirect(redirect::Policy::none())
            .no_proxy()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|_| SourceError::Config)?;
        Ok(Self {
            client,
            address: config.address.clone(),
            last: Mutex::new(None),
        })
    }

    /// Clears the last request's refusal, before a request that may never reach an answer.
    pub(crate) fn forget(&self) {
        if let Ok(mut last) = self.last.lock() {
            *last = None;
        }
    }

    /// Records the answer to `method` on `path`: its refusal, if `judged` is one, or none.
    pub(crate) fn note(
        &self,
        method: &Method,
        path: &str,
        status: StatusCode,
        judged: &Result<(), SourceError>,
    ) {
        let refused = judged.as_ref().err().map(|error| {
            (
                Refused {
                    method: method.as_str().to_owned(),
                    path: path.to_owned(),
                    status: status.as_u16(),
                },
                error.clone(),
            )
        });
        if let Ok(mut last) = self.last.lock() {
            *last = refused;
        }
    }

    /// `error` as provisioning reports it: with the last request's refusal when that refusal is
    /// `error`, and with none otherwise.
    pub(crate) fn explained(&self, error: SourceError) -> ProvisionError {
        let refused = self
            .last
            .lock()
            .ok()
            .and_then(|last| last.clone())
            .and_then(|(refused, judged)| (judged == error).then_some(refused));
        ProvisionError { error, refused }
    }

    /// One request to `/v1/<path>`. The status maps to a refusal before any body is read, and
    /// no body ever reaches an error.
    async fn call(
        &self,
        method: Method,
        path: &str,
        token: Option<&SecretString>,
        body: Option<&Value>,
    ) -> Result<Option<Value>, SourceError> {
        self.forget();
        let asked = method.clone();
        let mut request = self
            .client
            .request(method, format!("{}/v1/{path}", self.address));
        if let Some(token) = token {
            request = request.header("X-Vault-Token", token.expose_secret());
        }
        if let Some(body) = body {
            request = request
                .header("Content-Type", "application/json")
                .body(body.to_string());
        }
        let response = request.send().await.map_err(|_| SourceError::Unavailable)?;
        let status = response.status();
        let judged = refusal(status);
        self.note(&asked, path, status, &judged);
        judged?;
        let bytes = response
            .bytes()
            .await
            .map_err(|_| SourceError::Unavailable)?
            .to_vec();
        parsed(bytes)
    }

    /// A Kubernetes login to `/v1/<path>` for `role`, its refusal judged by
    /// [`kubernetes_login_refusal`].
    async fn login(
        &self,
        path: &str,
        body: &Value,
        role: &str,
    ) -> Result<Option<Value>, SourceError> {
        self.post_judged(path, None, body, |status, answer| {
            kubernetes_login_refusal(status, answer, role)
        })
        .await
    }

    /// A `POST` to `/v1/<path>`: as [`Http::call`], except that a refusal's body is read, for
    /// `judge`, and then wiped like any other.
    async fn post_judged(
        &self,
        path: &str,
        token: Option<&SecretString>,
        body: &Value,
        judge: impl FnOnce(u16, &[u8]) -> Result<(), SourceError>,
    ) -> Result<Option<Value>, SourceError> {
        self.judged(Method::POST, path, token, Some(body), judge)
            .await
    }

    /// One request to `/v1/<path>` whose refusal `judge` reads from its status and body, as
    /// [`Http::call`] reads it from the status alone. A refused answer's body is wiped and never
    /// reaches the error.
    async fn judged(
        &self,
        method: Method,
        path: &str,
        token: Option<&SecretString>,
        body: Option<&Value>,
        judge: impl FnOnce(u16, &[u8]) -> Result<(), SourceError>,
    ) -> Result<Option<Value>, SourceError> {
        self.forget();
        let asked = method.clone();
        let mut request = self
            .client
            .request(method, format!("{}/v1/{path}", self.address));
        if let Some(token) = token {
            request = request.header("X-Vault-Token", token.expose_secret());
        }
        if let Some(body) = body {
            request = request
                .header("Content-Type", "application/json")
                .body(body.to_string());
        }
        let response = request.send().await.map_err(|_| SourceError::Unavailable)?;
        let status = response.status();
        let mut bytes = response
            .bytes()
            .await
            .map_err(|_| SourceError::Unavailable)?
            .to_vec();
        let judged = judge(status.as_u16(), &bytes);
        self.note(&asked, path, status, &judged);
        if let Err(refused) = judged {
            bytes.zeroize();
            return Err(refused);
        }
        parsed(bytes)
    }
}

/// The marker a take overwrites a code with (DEC-2061 item 4), never a code.
const TOMBSTONE: &str = "taken";

/// Whether the vault's answer to a KV read is a tombstone a take left.
fn tombstoned(answer: Option<&Value>) -> bool {
    answer.and_then(|a| a.pointer(&format!("/data/data/{TOMBSTONE}"))) == Some(&Value::Bool(true))
}

/// The refusal a check-and-set overwrite's answer means, its status and its body: [`refusal`]'s,
/// except that a 400 whose one error is that the version moved on is [`SourceError::Missing`],
/// since another taker got the code first. No part of the body reaches the error.
pub(crate) fn cas_refusal(status: u16, body: &[u8]) -> Result<(), SourceError> {
    let moved = json!(["check-and-set parameter did not match the current version"]);
    let lost = status == 400
        && serde_json::from_slice::<Value>(body)
            .ok()
            .is_some_and(|answer| answer.get("errors") == Some(&moved));
    if lost {
        return Err(SourceError::Missing);
    }
    refusal(StatusCode::from_u16(status).map_err(|_| SourceError::Unavailable)?)
}

/// The JSON in `bytes`, or `None` when there are none; the bytes are wiped either way.
fn parsed(mut bytes: Vec<u8>) -> Result<Option<Value>, SourceError> {
    let parsed = if bytes.is_empty() {
        Ok(None)
    } else {
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| SourceError::Malformed)
    };
    bytes.zeroize();
    parsed
}

/// The refusal a status means, if any. Only a refusal the vault itself states is one: 400, 401
/// and 403 are [`SourceError::Denied`], 404 [`SourceError::Missing`], 503 (sealed or standby)
/// [`SourceError::Sealed`]. Every other status, a redirect from an HA standby, a 429 quota, a
/// 408, or a 5xx, is [`SourceError::Unavailable`]: an outage, which keeps a running executor's
/// lease (E21-9), never a revocation.
pub(crate) fn refusal(status: StatusCode) -> Result<(), SourceError> {
    match status.as_u16() {
        200..=299 => Ok(()),
        400 | 401 | 403 => Err(SourceError::Denied),
        404 => Err(SourceError::Missing),
        503 => Err(SourceError::Sealed),
        _ => Err(SourceError::Unavailable),
    }
}

/// The refusal a Kubernetes login's answer means, its status and its body: [`refusal`]'s, except
/// that a 400 whose one error is that `role` does not exist is [`SourceError::RoleMissing`], so a
/// process whose role the provisioner has not made yet can wait for it within its deadline, and
/// every other refusal stops it (DEC-2060, agreed with lane D1). No part of the body reaches the
/// error.
pub fn kubernetes_login_refusal(status: u16, body: &[u8], role: &str) -> Result<(), SourceError> {
    let missing = json!([format!("invalid role name \"{role}\"")]);
    let says_missing = status == 400
        && serde_json::from_slice::<Value>(body)
            .ok()
            .is_some_and(|answer| answer.get("errors") == Some(&missing));
    if says_missing {
        return Err(SourceError::RoleMissing);
    }
    refusal(StatusCode::from_u16(status).map_err(|_| SourceError::Unavailable)?)
}

/// OpenBao's answers, verbatim, to every request on a KV v2 mount while the mount finishes the
/// upgrade it starts in the background when it is enabled (`internal/builtin/logical/kv/upgrade.go`
/// in OpenBao 2.7.1): first as the node that upgrades, then as one that waits for it.
const KV_UPGRADING: [&str; 2] = [
    "Upgrading from non-versioned to versioned data. This backend will be unavailable for a brief \
     period and will resume service shortly.",
    "Waiting for the primary to upgrade from non-versioned to versioned data. This backend will be \
     unavailable for a brief period and will resume service when the primary is finished.",
];

/// The refusal an answer from a KV v2 mount means, its status and its body: [`refusal`]'s, except
/// that a 400 whose one error is one of [`KV_UPGRADING`] is [`SourceError::KvUpgrading`], so a
/// provisioning that enabled the mount a moment ago can wait for the upgrade to finish, and every
/// other refusal, any other 400 among them, stays a refusal (E21-32v). No part of the body reaches
/// the error.
pub fn kv_upgrade_refusal(status: u16, body: &[u8]) -> Result<(), SourceError> {
    let says_upgrading = status == 400
        && serde_json::from_slice::<Value>(body)
            .ok()
            .is_some_and(|answer| {
                KV_UPGRADING
                    .iter()
                    .any(|message| answer.get("errors") == Some(&json!([message])))
            });
    if says_upgrading {
        return Err(SourceError::KvUpgrading);
    }
    refusal(StatusCode::from_u16(status).map_err(|_| SourceError::Unavailable)?)
}

/// What a read of the code a take is about to overwrite says: no code, or a tombstone a take left,
/// is `None`; otherwise the code's version and its bytes, held as a secret from the moment they are
/// decoded, so a lost race or an error wipes them. A read with no version or no value, or whose
/// value is not base64, is [`SourceError::Malformed`]; any other refusal is the read's own.
pub(crate) fn taken_read(
    read: Result<Option<Value>, SourceError>,
) -> Result<Option<(u64, SecretSlice<u8>)>, SourceError> {
    let read = match read {
        Ok(read) => read,
        Err(SourceError::Missing) => return Ok(None),
        Err(e) => return Err(e),
    };
    if tombstoned(read.as_ref()) {
        return Ok(None);
    }
    let version = read
        .as_ref()
        .and_then(|r| r.pointer("/data/metadata/version"))
        .and_then(Value::as_u64)
        .ok_or(SourceError::Malformed)?;
    let code = exact_decode(text(read.as_ref(), "/data/data/value")?)?;
    Ok(Some((version, code)))
}

/// The bytes `encoded`, padded standard base64, decodes to, in a buffer of exactly their size, so
/// boxing them as a secret never copies them and leaves the first copy unwiped; a buffer partly
/// written by a decode that fails is wiped. The size is three bytes a four-character group less
/// one a pad, which is exact for every input the decode accepts: it refuses a length that is not
/// a whole number of groups, misplaced padding, and output that does not fit. Anything but padded
/// standard base64 is [`SourceError::Malformed`].
fn exact_decode(encoded: &str) -> Result<SecretSlice<u8>, SourceError> {
    let padding = encoded
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'=')
        .count();
    let size = encoded
        .len()
        .checked_div(4)
        .and_then(|quads| quads.checked_mul(3))
        .and_then(|whole| whole.checked_sub(padding))
        .ok_or(SourceError::Malformed)?;
    let mut bytes = vec![0u8; size];
    match STANDARD.decode_slice(encoded, &mut bytes) {
        Ok(_) => Ok(SecretSlice::from(bytes.into_boxed_slice())),
        Err(_) => {
            bytes.zeroize();
            Err(SourceError::Malformed)
        }
    }
}

/// What the read again after a refused overwrite means: the code gone is a race another taker
/// won, `None`; a code still there is one this taker may not take, [`SourceError::Denied`]; a read
/// that fails is that failure.
pub(crate) fn lost_or_denied_verdict(
    reread: Result<Option<Value>, SourceError>,
) -> Result<Option<SecretSlice<u8>>, SourceError> {
    match reread {
        Err(SourceError::Missing) => Ok(None),
        Ok(_) => Err(SourceError::Denied),
        Err(e) => Err(e),
    }
}

/// The string at `pointer` in `value`.
fn text<'a>(value: Option<&'a Value>, pointer: &str) -> Result<&'a str, SourceError> {
    value
        .and_then(|v| v.pointer(pointer))
        .and_then(Value::as_str)
        .ok_or(SourceError::Malformed)
}

/// The non-negative integer at `pointer` in `value`.
fn seconds(value: Option<&Value>, pointer: &str) -> Result<i64, SourceError> {
    value
        .and_then(|v| v.pointer(pointer))
        .and_then(Value::as_i64)
        .filter(|s| *s >= 0)
        .ok_or(SourceError::Malformed)
}

fn after(now: UtcNanos, secs: i64) -> Result<UtcNanos, SourceError> {
    now.secs()
        .checked_add(secs)
        .and_then(|s| UtcNanos::from_parts(s, now.nanos()).ok())
        .ok_or(SourceError::Malformed)
}

/// Whether a server may be used, from its `sys/seal-status` body: sealed or uninitialized is
/// [`SourceError::Sealed`]; in-memory storage (a dev server) is
/// [`SourceError::DevServerRefused`] unless `allow_dev`; anything unreadable is
/// [`SourceError::Malformed`].
pub fn seal_verdict(body: &[u8], allow_dev: bool) -> Result<(), SourceError> {
    let status: Value = serde_json::from_slice(body).map_err(|_| SourceError::Malformed)?;
    let sealed = status
        .get("sealed")
        .and_then(Value::as_bool)
        .ok_or(SourceError::Malformed)?;
    let storage = status
        .get("storage_type")
        .and_then(Value::as_str)
        .ok_or(SourceError::Malformed)?;
    let initialized = status
        .get("initialized")
        .map_or(Some(true), Value::as_bool)
        .ok_or(SourceError::Malformed)?;
    if sealed || !initialized {
        Err(SourceError::Sealed)
    } else if storage == "inmem" && !allow_dev {
        Err(SourceError::DevServerRefused)
    } else {
        Ok(())
    }
}

/// Whether the mount the fingerprint key is on, from its `sys/mounts/<transit>` answer, is a
/// transit mount: one of any other type is [`SourceError::Config`], so provisioning never takes
/// another engine for the key's (DEC-2345 item 1).
///
/// # Errors
/// [`SourceError::Config`] for any other answer.
pub fn transit_mount_verdict(answer: Option<&Value>) -> Result<(), SourceError> {
    let transit = answer.and_then(|mount| mount.pointer("/data/type")) == Some(&json!("transit"));
    if transit {
        Ok(())
    } else {
        Err(SourceError::Config)
    }
}

/// Whether a fingerprint key already there, from its `<transit>/keys/<key>` answer, is as
/// provisioning makes it: never exportable, never deletable, and with no plaintext backup. A key
/// that is any of them, or an answer that does not say, is [`SourceError::Config`]: provisioning
/// fails rather than keep a key that can leave the vault (DEC-2345 item 1).
///
/// # Errors
/// [`SourceError::Config`] for any other answer.
pub fn fingerprint_key_verdict(answer: Option<&Value>) -> Result<(), SourceError> {
    let never = |field: &str| {
        answer.and_then(|key| key.pointer(&format!("/data/{field}"))) == Some(&json!(false))
    };
    if never("exportable") && never("deletion_allowed") && never("allow_plaintext_backup") {
        Ok(())
    } else {
        Err(SourceError::Config)
    }
}

/// Whether provisioning must create the fingerprint key, from its `GET <transit>/keys/<key>`:
/// only a 404 is a key that is not there, `true`; a key that is there must pass
/// [`fingerprint_key_verdict`], `false`. Any other refusal fails provisioning, so a transient
/// error never lets an existing key go unverified (review F1, round 2).
///
/// # Errors
/// The GET's own refusal, other than [`SourceError::Missing`], and [`fingerprint_key_verdict`]'s.
pub fn key_needs_creating(found: &Result<Option<Value>, SourceError>) -> Result<bool, SourceError> {
    match found {
        Ok(key) => fingerprint_key_verdict(key.as_ref()).map(|()| false),
        Err(SourceError::Missing) => Ok(true),
        Err(e) => Err(e.clone()),
    }
}

/// A logged-in session as one identity.
pub struct OpenBaoSource {
    identity: Identity,
    http: Http,
    mounts: Mounts,
    token: SecretString,
    token_expires_at: Mutex<UtcNanos>,
}

impl OpenBaoSource {
    /// A session over `token`, which the test's operator made with the policy it gives: the one
    /// way to hold the Kubernetes-auth exchange's policy without a cluster to check a login.
    #[cfg(test)]
    pub(crate) fn from_token(
        config: &OpenBaoConfig,
        identity: Identity,
        token: SecretString,
        expires_at: UtcNanos,
    ) -> Result<Self, SourceError> {
        Ok(Self {
            identity,
            http: Http::new(config)?,
            mounts: config.mounts.clone(),
            token,
            token_expires_at: Mutex::new(expires_at),
        })
    }
}

impl fmt::Debug for OpenBaoSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenBaoSource")
            .field("identity", &self.identity)
            .field("address", &self.http.address)
            .finish_non_exhaustive()
    }
}

/// The Kubernetes auth login path of `identity`, a connection's executor or token exchange, and
/// the role it logs in to: `auth/<its kind's mount>/login` and its ServiceAccount name, which
/// names its role (DEC-2060 item 5). Any other identity is [`SourceError::Config`].
pub fn kubernetes_login_path(
    identity: &Identity,
    mounts: &Mounts,
) -> Result<(String, String), SourceError> {
    let mount = match identity {
        Identity::Executor(_) => &mounts.kubernetes_executor,
        Identity::TokenExchange(_) => &mounts.kubernetes_exchange,
        Identity::Api
        | Identity::ConnectionManager
        | Identity::Runtime
        | Identity::SecretImport
        | Identity::ModelGateway
        | Identity::NewsReader
        | Identity::BarsReader
        | Identity::EmailSender
        | Identity::Provisioner => return Err(SourceError::Config),
    };
    Ok((
        format!("auth/{mount}/login"),
        service_account_name(identity)?,
    ))
}

/// Whether the vault's answer to a Kubernetes auth login, its `auth` object, is `identity`'s own:
/// the role it reached is the one named by its ServiceAccount, and the ServiceAccount the vault
/// checked is that one. An answer without either is [`SourceError::Malformed`]; another's is
/// [`SourceError::Denied`].
pub fn kubernetes_login_verdict(
    identity: &Identity,
    auth: Option<&Value>,
) -> Result<(), SourceError> {
    let role = text(auth, "/metadata/role")?;
    let account = text(auth, "/metadata/service_account_name")?;
    let (_, own_role) = kubernetes_login_path(identity, &Mounts::default())?;
    if role == own_role && account == service_account_name(identity)? {
        Ok(())
    } else {
        Err(SourceError::Denied)
    }
}

/// Whether the vault's answer to a Kubernetes login names `namespace` as the ServiceAccount's,
/// beside the role's own binding, as defence in depth. An answer without it is
/// [`SourceError::Malformed`]; another namespace is [`SourceError::Denied`].
pub fn kubernetes_namespace_verdict(
    auth: Option<&Value>,
    namespace: &str,
) -> Result<(), SourceError> {
    if text(auth, "/metadata/service_account_namespace")? == namespace {
        Ok(())
    } else {
        Err(SourceError::Denied)
    }
}

impl OpenBaoSource {
    /// A session as `identity` that never logged in, for unit tests of the checks that refuse
    /// before the vault is reached: its token is empty, so any call that does reach a vault is
    /// refused there.
    #[cfg(test)]
    pub(crate) fn unlogged(
        config: &OpenBaoConfig,
        identity: Identity,
    ) -> Result<Self, SourceError> {
        Ok(Self {
            identity,
            http: Http::new(config)?,
            mounts: config.mounts.clone(),
            token: SecretString::from(String::new()),
            token_expires_at: Mutex::new(UtcNanos::EPOCH),
        })
    }

    /// Logs in as `identity` with its AppRole `role_id` and `secret_id` (read by the caller from
    /// mounted files, never the environment). The login is refused unless the role it reaches
    /// is `identity`'s own. The token exchange logs in with [`OpenBaoSource::login_with_grant`].
    pub async fn login(
        config: &OpenBaoConfig,
        identity: Identity,
        role_id: &str,
        secret_id: &SecretString,
        now: UtcNanos,
    ) -> Result<Self, SourceError> {
        if matches!(identity, Identity::TokenExchange(_)) {
            return Err(SourceError::Config);
        }
        Self::log_in(config, identity, role_id, secret_id, now).await
    }

    /// Logs in as `identity`, a connection's executor or token exchange, with the projected
    /// ServiceAccount token `jwt` its pod was given (audience [`crate::kubernetes::AUDIENCE`],
    /// read by the caller with [`crate::mounted::read_service_account_token`] at each login), on
    /// its kind's Kubernetes auth mount, its role named by its ServiceAccount (DEC-2060). The
    /// vault checks the token with the cluster on every login, and admits only the one
    /// ServiceAccount the role binds, which must be in `namespace`, the workspace's. Any other
    /// identity is [`SourceError::Config`]; a login refused, or one that reaches a role,
    /// ServiceAccount or namespace that is not `identity`'s own, is [`SourceError::Denied`]; a
    /// role not yet made is [`SourceError::RoleMissing`]; a vault that cannot reach the cluster is
    /// [`SourceError::Unavailable`]. The token is never logged, never put in an error, and dropped
    /// once the login is done.
    pub async fn login_kubernetes(
        config: &OpenBaoConfig,
        identity: Identity,
        jwt: &SecretString,
        namespace: &str,
        now: UtcNanos,
    ) -> Result<Self, SourceError> {
        let (path, role) = kubernetes_login_path(&identity, &config.mounts)?;
        let http = Http::new(config)?;
        let status = http
            .call(Method::GET, "sys/seal-status", None, None)
            .await?
            .ok_or(SourceError::Malformed)?;
        seal_verdict(status.to_string().as_bytes(), ALLOW_DEV_SERVER)?;
        let body = json!({ "role": role, "jwt": jwt.expose_secret() });
        let answer = http.login(&path, &body, &role).await?;
        let auth = answer.as_ref().and_then(|answer| answer.get("auth"));
        let token = SecretString::from(text(answer.as_ref(), "/auth/client_token")?.to_owned());
        let ttl = seconds(answer.as_ref(), "/auth/lease_duration")?;
        let session = Self {
            identity,
            http,
            mounts: config.mounts.clone(),
            token,
            token_expires_at: Mutex::new(after(now, ttl)?),
        };
        let verdict = kubernetes_login_verdict(&session.identity, auth)
            .and_then(|()| kubernetes_namespace_verdict(auth, namespace));
        if let Err(refused) = verdict {
            let _ = session.revoke_token().await;
            return Err(refused);
        }
        Ok(session)
    }

    /// The token exchange's session for the grant's connection, spending the grant. A grant
    /// expired at `now`, used, revoked, or for another connection is
    /// [`SourceError::GrantInvalid`].
    pub async fn login_with_grant(
        config: &OpenBaoConfig,
        grant: &Grant,
        now: UtcNanos,
    ) -> Result<Self, SourceError> {
        if now >= grant.expires_at {
            return Err(SourceError::GrantInvalid);
        }
        let identity = Identity::TokenExchange(grant.connection.clone());
        Self::log_in(config, identity, &grant.role_id, &grant.secret_id, now)
            .await
            .map_err(|e| match e {
                SourceError::Denied => SourceError::GrantInvalid,
                other => other,
            })
    }

    async fn log_in(
        config: &OpenBaoConfig,
        identity: Identity,
        role_id: &str,
        secret_id: &SecretString,
        now: UtcNanos,
    ) -> Result<Self, SourceError> {
        let http = Http::new(config)?;
        let status = http
            .call(Method::GET, "sys/seal-status", None, None)
            .await?
            .ok_or(SourceError::Malformed)?;
        seal_verdict(status.to_string().as_bytes(), ALLOW_DEV_SERVER)?;
        let body = json!({ "role_id": role_id, "secret_id": secret_id.expose_secret() });
        let auth = http
            .call(
                Method::POST,
                &format!("auth/{}/login", config.mounts.auth),
                None,
                Some(&body),
            )
            .await?;
        let token = SecretString::from(text(auth.as_ref(), "/auth/client_token")?.to_owned());
        let ttl = seconds(auth.as_ref(), "/auth/lease_duration")?;
        let role = text(auth.as_ref(), "/auth/metadata/role_name")?;
        let session = Self {
            identity,
            http,
            mounts: config.mounts.clone(),
            token,
            token_expires_at: Mutex::new(after(now, ttl)?),
        };
        if role != role_name(&session.identity) {
            let _ = session.revoke_token().await;
            return Err(SourceError::Denied);
        }
        Ok(session)
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Option<Value>, SourceError> {
        self.http.call(method, path, Some(&self.token), body).await
    }

    fn data_path(&self, path: &SecretPath) -> Result<String, SourceError> {
        Ok(format!("{}/data/{}", self.mounts.kv, kv_path(path)?))
    }

    fn token_expiry(&self) -> Result<UtcNanos, SourceError> {
        self.token_expires_at
            .lock()
            .map(|at| *at)
            .map_err(|_| SourceError::Unavailable)
    }

    /// Ends this session: its token is revoked, so every later request with it is refused. A
    /// process that reads one secret once ends its session right after, so no token able to read
    /// that secret outlives the read (E2-22).
    pub async fn end_session(&self) -> Result<(), SourceError> {
        self.revoke_token().await
    }

    async fn revoke_token(&self) -> Result<(), SourceError> {
        self.call(Method::POST, "auth/token/revoke-self", None)
            .await
            .map(drop)
    }

    async fn read_value(&self, path: &SecretPath) -> Result<Option<SecretSlice<u8>>, SourceError> {
        let value = match self.call(Method::GET, &self.data_path(path)?, None).await {
            Ok(value) => value,
            Err(SourceError::Missing) => return Ok(None),
            Err(e) => return Err(e),
        };
        if tombstoned(value.as_ref()) {
            return Ok(None);
        }
        let encoded = text(value.as_ref(), "/data/data/value")?;
        STANDARD
            .decode(encoded)
            .map(|bytes| Some(SecretSlice::from(bytes)))
            .map_err(|_| SourceError::Malformed)
    }

    /// What a refused overwrite of the code at `data` means. The taker may only update, never
    /// create, so once another taker has deleted the code the overwrite is refused before its
    /// version is checked: that is a lost race, and `None`, but only if the code is gone when read
    /// again. A code still there means the taker may not take it: [`SourceError::Denied`].
    async fn lost_or_denied(&self, data: &str) -> Result<Option<SecretSlice<u8>>, SourceError> {
        lost_or_denied_verdict(self.call(Method::GET, data, None).await)
    }

    fn role_path(&self, connection: &ConnectionId) -> String {
        format!(
            "auth/{}/role/{}",
            self.mounts.auth,
            role_name(&Identity::TokenExchange(connection.clone()))
        )
    }
}

impl OpenBaoSource {
    /// Refuses `role_refused`, before the vault is reached, a role operation from any session but
    /// the provisioner's own (DEC-2350 item 5), whatever its policy would answer.
    fn provisioner_only(&self) -> Result<(), RoleRefusal> {
        if self.identity == Identity::Provisioner {
            Ok(())
        } else {
            Err(RoleRefusal::RoleRefused)
        }
    }

    /// Whether the marker that the fingerprint key rotates is there.
    async fn rotating(&self) -> Result<bool, FingerprintError> {
        require(&self.identity, Access::Read, &SecretPath::Rotating).map_err(unavailable)?;
        self.read_value(&SecretPath::Rotating)
            .await
            .map(|marker| marker.is_some())
            .map_err(unavailable)
    }

    /// The fingerprint entry of the request `account` of `connection`, by its one reader.
    async fn fingerprint_entry(
        &self,
        connection: &ConnectionId,
        account: &AccountRef,
    ) -> Result<(AccountFingerprint, AccountPiiRef), FingerprintError> {
        let entry = self
            .fingerprint_bytes(connection, account)
            .await?
            .ok_or(FingerprintError::Missing)?;
        decode_entry(entry.expose_secret()).map_err(|_| FingerprintError::Missing)
    }

    /// The bytes of the fingerprint entry of the request `account` of `connection`, `None` where
    /// there is none.
    async fn fingerprint_bytes(
        &self,
        connection: &ConnectionId,
        account: &AccountRef,
    ) -> Result<Option<SecretSlice<u8>>, FingerprintError> {
        let path = SecretPath::Fingerprint(connection.clone(), account.clone());
        require(&self.identity, Access::Read, &path).map_err(unavailable)?;
        self.read_value(&path).await.map_err(unavailable)
    }
}

/// The version of the registry's form.
const CLAIMS_FORMAT: u8 = 1;

impl ManagerFingerprints for OpenBaoSource {
    async fn fingerprint(
        &self,
        connection: &ConnectionId,
        account: &AccountRef,
    ) -> Result<(AccountFingerprint, AccountPiiRef), FingerprintError> {
        if self.rotating().await? {
            return Err(FingerprintError::Rotating);
        }
        self.fingerprint_entry(connection, account).await
    }

    async fn claim(
        &self,
        connection: &ConnectionId,
        fingerprint: &AccountFingerprint,
    ) -> Result<Claim, FingerprintError> {
        if self.rotating().await? {
            return Err(FingerprintError::Rotating);
        }
        claim_in(self, connection, fingerprint).await
    }

    async fn release(
        &self,
        connection: &ConnectionId,
        account: &AccountRef,
    ) -> Result<(), FingerprintError> {
        let Some(entry) = self.fingerprint_bytes(connection, account).await? else {
            return Ok(());
        };
        let (fingerprint, _) =
            decode_entry(entry.expose_secret()).map_err(|_| FingerprintError::Unavailable)?;
        release_in(self, connection, &fingerprint).await
    }
}

impl Registry for OpenBaoSource {
    /// The registry entry, by check-and-set's version; an empty one at version 0 where there is
    /// none.
    async fn claims(&self) -> Result<(u64, BTreeMap<String, String>), FingerprintError> {
        require(&self.identity, Access::Read, &SecretPath::Claims).map_err(unavailable)?;
        let read = self
            .call(
                Method::GET,
                &self.data_path(&SecretPath::Claims).map_err(unavailable)?,
                None,
            )
            .await;
        match taken_read(read).map_err(unavailable)? {
            None => Ok((0, BTreeMap::new())),
            Some((version, bytes)) => serde_json::from_slice::<Value>(bytes.expose_secret())
                .ok()
                .filter(|registry| registry.get("format") == Some(&json!(CLAIMS_FORMAT)))
                .and_then(|registry| {
                    serde_json::from_value::<BTreeMap<String, String>>(
                        registry.get("claims")?.clone(),
                    )
                    .ok()
                })
                .map(|claims| (version, claims))
                .ok_or(FingerprintError::Unavailable),
        }
    }

    /// The registry entry written by check-and-set on `version`; a refused check-and-set is
    /// `false`.
    async fn write_claims(
        &self,
        version: u64,
        claims: &BTreeMap<String, String>,
    ) -> Result<bool, FingerprintError> {
        require(&self.identity, Access::Write, &SecretPath::Claims).map_err(unavailable)?;
        let registry = json!({ "claims": claims, "format": CLAIMS_FORMAT });
        let encoded = STANDARD.encode(registry.to_string());
        let body = json!({ "options": { "cas": version }, "data": { "value": encoded } });
        let path = self.data_path(&SecretPath::Claims).map_err(unavailable)?;
        match self
            .http
            .post_judged(&path, Some(&self.token), &body, cas_refusal)
            .await
        {
            Ok(_) => Ok(true),
            Err(SourceError::Missing) => Ok(false),
            Err(e) => Err(unavailable(e)),
        }
    }
}

impl RoleProvisioner for OpenBaoSource {
    async fn make_roles(
        &self,
        connection: &ConnectionId,
        namespace: &str,
    ) -> Result<(), RoleRefusal> {
        self.provisioner_only()?;
        for identity in [
            Identity::Executor(connection.clone()),
            Identity::TokenExchange(connection.clone()),
        ] {
            let (path, body) = role_request(&identity, &self.mounts, namespace)
                .map_err(|e| RoleRefusal::of(&e))?;
            self.call(Method::POST, &path, Some(&body))
                .await
                .map_err(|e| RoleRefusal::of(&e))?;
        }
        Ok(())
    }

    /// OpenBao answers a DELETE of a role that is not there 204, so an absent role is `Ok` from
    /// the call itself. `Missing` (404) comes only when the auth mount is absent, and then no
    /// role can exist under it either, so it is `Ok` too: nothing is left for the exchange.
    async fn remove_exchange_role(
        &self,
        connection: &ConnectionId,
        namespace: &str,
    ) -> Result<(), RoleRefusal> {
        self.provisioner_only()?;
        let (path, _) = role_request(
            &Identity::TokenExchange(connection.clone()),
            &self.mounts,
            namespace,
        )
        .map_err(|e| RoleRefusal::of(&e))?;
        match self.call(Method::DELETE, &path, None).await {
            Ok(_) | Err(SourceError::Missing) => Ok(()),
            Err(e) => Err(RoleRefusal::of(&e)),
        }
    }
}

/// A vault refusal, as the fingerprint operations answer every one: unavailable.
fn unavailable(_: SourceError) -> FingerprintError {
    FingerprintError::Unavailable
}

impl SecretSource for OpenBaoSource {
    fn identity(&self) -> &Identity {
        &self.identity
    }

    /// The fingerprint of the broker account id `account` under the deployment's key, by transit's
    /// HMAC (`<transit>/hmac/account-fingerprint`): asked by a connection's executor alone, at its
    /// connect (DEC-2345 item 2). The key never leaves the vault, and the id is sent only to it.
    ///
    /// # Errors
    /// [`SourceError::Denied`] for any identity but an executor, the vault's refusals, and
    /// [`SourceError::Malformed`] for an answer that is not a fingerprint.
    async fn hmac_account(
        &self,
        account: &BrokerAccountId,
    ) -> Result<AccountFingerprint, SourceError> {
        if !matches!(self.identity, Identity::Executor(_)) {
            return Err(SourceError::Denied);
        }
        let mut input = STANDARD.encode(account.secret().expose_secret());
        let mut body = json!({ "input": input });
        input.zeroize();
        let path = format!("{}/hmac/{FINGERPRINT_KEY}", self.mounts.transit);
        let answer = self.call(Method::POST, &path, Some(&body)).await;
        if let Some(Value::String(sent)) = body.get_mut("input") {
            sent.zeroize();
        }
        AccountFingerprint::new(text(answer?.as_ref(), "/data/hmac")?)
    }

    /// A check-and-set write at version 0, which the vault takes only where no version is there,
    /// after the client-side check of [`Access::Create`].
    async fn create(&self, path: &SecretPath, secret: &SecretSlice<u8>) -> Result<(), SourceError> {
        require(&self.identity, Access::Create, path)?;
        let mut encoded = STANDARD.encode(secret.expose_secret());
        let body = json!({ "options": { "cas": 0 }, "data": { "value": encoded } });
        encoded.zeroize();
        let created = self
            .http
            .post_judged(
                &self.data_path(path)?,
                Some(&self.token),
                &body,
                cas_refusal,
            )
            .await;
        match created {
            Ok(_) => Ok(()),
            Err(SourceError::Missing) => Err(SourceError::Denied),
            Err(e) => Err(e),
        }
    }

    /// Reads the code with its version, overwrites that version alone with a tombstone, destroys
    /// the version read and deletes the tombstone's version alone, keeping the metadata so the
    /// version count never restarts and a code stored since is never hidden (DEC-2321): a taker
    /// whose overwrite loses, or that reads a tombstone or no code, gets `None`.
    async fn take_code(
        &self,
        connection: &ConnectionId,
    ) -> Result<Option<SecretSlice<u8>>, SourceError> {
        let path = SecretPath::Code(connection.clone());
        require(&self.identity, Access::Take, &path)?;
        let data = self.data_path(&path)?;
        let Some((version, code)) = taken_read(self.call(Method::GET, &data, None).await)? else {
            return Ok(None);
        };
        let tombstone = version.checked_add(1).ok_or(SourceError::Malformed)?;
        let body = json!({ "options": { "cas": version }, "data": { TOMBSTONE: true } });
        let overwrite = self
            .http
            .post_judged(&data, Some(&self.token), &body, cas_refusal)
            .await;
        match overwrite {
            Ok(_) => {}
            Err(SourceError::Missing) => return Ok(None),
            Err(SourceError::Denied) => return self.lost_or_denied(&data).await,
            Err(e) => return Err(e),
        }
        for (endpoint, versions) in [("destroy", version), ("delete", tombstone)] {
            let named = format!("{}/{endpoint}/{}", self.mounts.kv, kv_path(&path)?);
            match self
                .call(
                    Method::POST,
                    &named,
                    Some(&json!({ "versions": [versions] })),
                )
                .await
            {
                Ok(_) | Err(SourceError::Missing) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(Some(code))
    }

    async fn put(&self, path: &SecretPath, secret: &SecretSlice<u8>) -> Result<(), SourceError> {
        let mut encoded = STANDARD.encode(secret.expose_secret());
        let body = json!({ "data": { "value": encoded } });
        encoded.zeroize();
        self.call(Method::POST, &self.data_path(path)?, Some(&body))
            .await
            .map(drop)
    }

    async fn read(&self, path: &SecretPath) -> Result<Option<SecretSlice<u8>>, SourceError> {
        self.read_value(path).await
    }

    async fn delete(&self, path: &SecretPath) -> Result<(), SourceError> {
        let metadata = format!("{}/metadata/{}", self.mounts.kv, kv_path(path)?);
        match self.call(Method::DELETE, &metadata, None).await {
            Ok(_) | Err(SourceError::Missing) => Ok(()),
            Err(e) => Err(e),
        }
    }

    async fn lease(&self, connection: &ConnectionId, now: UtcNanos) -> Result<Lease, SourceError> {
        let secret = self
            .read_value(&SecretPath::Token(connection.clone()))
            .await?
            .ok_or(SourceError::Missing)?;
        let (renew_at, expires_at) =
            lease_times(now, Some(self.token_expiry()?)).ok_or(SourceError::Malformed)?;
        Ok(Lease {
            connection: connection.clone(),
            secret,
            expires_at,
            renew_at,
        })
    }

    async fn renew(&self, lease: &Lease, now: UtcNanos) -> Result<Lease, SourceError> {
        let body = json!({ "increment": format!("{LEASE_SECS}s") });
        let auth = self
            .call(Method::POST, "auth/token/renew-self", Some(&body))
            .await?;
        let ttl = seconds(auth.as_ref(), "/auth/lease_duration")?;
        let expires = after(now, ttl)?;
        *self
            .token_expires_at
            .lock()
            .map_err(|_| SourceError::Unavailable)? = expires;
        self.lease(&lease.connection, now).await
    }

    async fn revoke(&self, lease: Lease) -> Result<(), SourceError> {
        drop(lease);
        self.revoke_token().await
    }

    async fn issue_grant(
        &self,
        connection: &ConnectionId,
        now: UtcNanos,
    ) -> Result<Grant, SourceError> {
        let role = self.role_path(connection);
        let role_id = self
            .call(Method::GET, &format!("{role}/role-id"), None)
            .await?;
        let role_id = text(role_id.as_ref(), "/data/role_id")?.to_owned();
        let issued = self
            .call(Method::POST, &format!("{role}/secret-id"), Some(&json!({})))
            .await?;
        Ok(Grant {
            connection: connection.clone(),
            role_id,
            secret_id: SecretString::from(text(issued.as_ref(), "/data/secret_id")?.to_owned()),
            accessor: text(issued.as_ref(), "/data/secret_id_accessor")?.to_owned(),
            expires_at: after(now, GRANT_SECS)?,
        })
    }

    /// A grant already spent or expired is gone, which is what revoking it asks: OpenBao
    /// answers a lookup of it with 404, so only a live grant is destroyed.
    async fn revoke_grant(&self, grant: &Grant) -> Result<(), SourceError> {
        let body = json!({ "secret_id_accessor": grant.accessor });
        let role = self.role_path(&grant.connection);
        let lookup = format!("{role}/secret-id-accessor/lookup");
        match self.call(Method::POST, &lookup, Some(&body)).await {
            Ok(_) => {}
            Err(SourceError::Missing) => return Ok(()),
            Err(e) => return Err(e),
        }
        let destroy = format!("{role}/secret-id-accessor/destroy");
        self.call(Method::POST, &destroy, Some(&body))
            .await
            .map(drop)
    }
}

/// One identity's AppRole credentials, as [`provision`] made them.
pub struct RoleCredentials {
    pub identity: Identity,
    pub role_id: String,
    pub secret_id: SecretString,
}

impl fmt::Debug for RoleCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoleCredentials")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

/// The AppRole settings of one identity's role: its token's lifetime and its secret ids' uses
/// and lifetime (zero: unlimited).
pub(crate) fn role_settings(identity: &Identity) -> Value {
    match identity {
        Identity::TokenExchange(_) => json!({
            "token_ttl": format!("{GRANT_SECS}s"),
            "token_max_ttl": format!("{GRANT_SECS}s"),
            "secret_id_num_uses": 1,
            "secret_id_ttl": format!("{GRANT_SECS}s"),
        }),
        Identity::Executor(_) => json!({
            "token_ttl": format!("{LEASE_SECS}s"),
            "token_max_ttl": format!("{EXECUTOR_TOKEN_MAX_SECS}s"),
            "secret_id_num_uses": 0,
            "secret_id_ttl": "0s",
        }),
        _ => json!({
            "token_ttl": format!("{LEASE_SECS}s"),
            "token_max_ttl": format!("{LEASE_SECS}s"),
            "secret_id_num_uses": 0,
            "secret_id_ttl": "0s",
        }),
    }
}

/// The identities [`provision`] issues a secret id for, whose files
/// [`crate::credentials::write_credentials`] writes: every provisioned identity but the token
/// exchanges, whose secret ids are grants, in [`provisioned_identities`]' order.
pub(crate) fn secret_id_identities(
    connections: &[ConnectionId],
    provisioner: bool,
) -> Vec<Identity> {
    provisioned_identities(connections, provisioner)
        .into_iter()
        .filter(|identity| !matches!(identity, Identity::TokenExchange(_)))
        .collect()
}

/// Every identity [`provision`] makes a policy and a role for: each process type's, the role
/// provisioner's when `provisioner`, then each connection's executor and token exchange, in that
/// order.
pub(crate) fn provisioned_identities(
    connections: &[ConnectionId],
    provisioner: bool,
) -> Vec<Identity> {
    let mut identities = vec![
        Identity::Api,
        Identity::ConnectionManager,
        Identity::Runtime,
        Identity::SecretImport,
        Identity::ModelGateway,
        Identity::NewsReader,
        Identity::BarsReader,
        Identity::EmailSender,
    ];
    if provisioner {
        identities.push(Identity::Provisioner);
    }
    for connection in connections {
        identities.push(Identity::Executor(connection.clone()));
        identities.push(Identity::TokenExchange(connection.clone()));
    }
    identities
}

/// Sets both Kubernetes auth mounts up, as [`kubernetes_mount_requests`] lists, enabling a mount
/// only where it is not there, and writes each kind's templated policy keyed by its mount's
/// accessor (DEC-2060 items 4 and 7).
async fn provision_kubernetes(
    http: &Http,
    token: Option<&SecretString>,
    mounts: &Mounts,
    auth: &KubernetesAuth,
) -> Result<(), SourceError> {
    let listed = http.call(Method::GET, "sys/auth", token, None).await?;
    for (path, body) in kubernetes_mount_requests(mounts, auth)? {
        let enabled = path
            .strip_prefix("sys/auth/")
            .is_some_and(|mount| mount_accessor(listed.as_ref(), mount).is_ok());
        if !enabled {
            http.call(Method::POST, &path, token, Some(&body)).await?;
        }
    }
    let listed = http.call(Method::GET, "sys/auth", token, None).await?;
    let any = ConnectionId::new("0").map_err(|_| SourceError::Config)?;
    for (identity, mount) in [
        (Identity::Executor(any.clone()), &mounts.kubernetes_executor),
        (Identity::TokenExchange(any), &mounts.kubernetes_exchange),
    ] {
        let accessor = mount_accessor(listed.as_ref(), mount)?;
        let hcl = kubernetes_policy(&identity, mounts, &accessor)?;
        let name = kubernetes_policy_name(&identity, mounts)?;
        http.call(
            Method::PUT,
            &format!("sys/policies/acl/{name}"),
            token,
            Some(&json!({ "policy": hcl })),
        )
        .await?;
    }
    Ok(())
}

/// Sets Kubernetes auth alone up on a vault already provisioned, with an operator's token: both
/// mounts, each enabled only where it is not there, their configuration, and their templated
/// policies, as [`provision`] sets them given `kubernetes`. It touches no AppRole role and mints
/// no secret id, so running it on a vault whose credentials are in use rotates none of them.
pub async fn provision_kubernetes_auth(
    config: &OpenBaoConfig,
    operator_token: &SecretString,
    auth: &KubernetesAuth,
) -> Result<(), ProvisionError> {
    let http = Http::new(config)?;
    provision_kubernetes(&http, Some(operator_token), &config.mounts, auth)
        .await
        .map_err(|error| http.explained(error))
}

/// The policy [`provision`] writes for `identity`'s role: [`provisioner_policy`] for the role
/// provisioner, given the workspace's `namespace`, and [`openbao_policy`] for every other identity
/// (DEC-2350 item 1).
///
/// # Errors
/// [`provisioner_policy`]'s or [`openbao_policy`]'s refusal.
pub fn provisioned_policy(
    identity: &Identity,
    connections: &[ConnectionId],
    mounts: &Mounts,
    namespace: Option<&str>,
) -> Result<String, SourceError> {
    match (identity, namespace) {
        (Identity::Provisioner, Some(namespace)) => provisioner_policy(mounts, namespace),
        _ => openbao_policy(identity, connections, mounts),
    }
}

/// Reads the KV mount's configuration, so a mount that is still upgrading answers
/// [`SourceError::KvUpgrading`] before provisioning writes anything to it, or any policy, role or
/// secret id: the caller waits and provisions again (E21-32v).
async fn kv_settled(
    http: &Http,
    token: Option<&SecretString>,
    mounts: &Mounts,
) -> Result<(), SourceError> {
    http.judged(
        Method::GET,
        &format!("{}/config", mounts.kv),
        token,
        None,
        kv_upgrade_refusal,
    )
    .await
    .map(drop)
}

/// The fingerprint key and registry (DEC-2345): the transit mount, its key, never exportable nor
/// deletable, and the registry entry made to take only check-and-set writes.
async fn provision_fingerprints(
    http: &Http,
    token: Option<&SecretString>,
    mounts: &Mounts,
) -> Result<(), SourceError> {
    let transit = format!("sys/mounts/{}", mounts.transit);
    if http.call(Method::GET, &transit, token, None).await.is_err() {
        http.call(
            Method::POST,
            &transit,
            token,
            Some(&json!({ "type": "transit" })),
        )
        .await?;
    }
    let mount = http.call(Method::GET, &transit, token, None).await?;
    transit_mount_verdict(mount.as_ref())?;
    let key = format!("{}/keys/{FINGERPRINT_KEY}", mounts.transit);
    if key_needs_creating(&http.call(Method::GET, &key, token, None).await)? {
        http.call(
            Method::POST,
            &key,
            token,
            Some(&json!({ "exportable": false })),
        )
        .await?;
        let made = http.call(Method::GET, &key, token, None).await?;
        fingerprint_key_verdict(made.as_ref())?;
    }
    let claims = format!("{}/metadata/{}", mounts.kv, kv_path(&SecretPath::Claims)?);
    http.call(
        Method::POST,
        &claims,
        token,
        Some(&json!({ "cas_required": true })),
    )
    .await
    .map(drop)
}

/// Sets a vault up for `connections`, with an operator's token: the KV v2 mount, the fingerprint
/// key and registry ([`provision_fingerprints`]), the AppRole auth method, one policy and one role per identity, and a secret id for every identity but the
/// token exchanges, whose secret ids are grants; and, given `kubernetes`, both Kubernetes auth
/// mounts and their templated policies, and, given its namespace too, the role provisioner, whose
/// policy is [`provisioner_policy`] for that namespace and nothing else (DEC-2350 item 1). Running
/// it again rewrites the policies and roles, so adding a connection is running it with the longer
/// list.
pub async fn provision(
    config: &OpenBaoConfig,
    operator_token: &SecretString,
    connections: &[ConnectionId],
    kubernetes: Option<&KubernetesAuth>,
) -> Result<Vec<RoleCredentials>, ProvisionError> {
    let http = Http::new(config)?;
    provision_over(&http, config, operator_token, connections, kubernetes)
        .await
        .map_err(|error| http.explained(error))
}

/// [`provision`] over `http`.
async fn provision_over(
    http: &Http,
    config: &OpenBaoConfig,
    operator_token: &SecretString,
    connections: &[ConnectionId],
    kubernetes: Option<&KubernetesAuth>,
) -> Result<Vec<RoleCredentials>, SourceError> {
    let token = Some(operator_token);
    let mounts = &config.mounts;
    let kv_mounted = http
        .call(
            Method::GET,
            &format!("sys/mounts/{}", mounts.kv),
            token,
            None,
        )
        .await;
    if kv_mounted.is_err() {
        let body = json!({ "type": "kv", "options": { "version": "2" } });
        http.call(
            Method::POST,
            &format!("sys/mounts/{}", mounts.kv),
            token,
            Some(&body),
        )
        .await?;
    }
    kv_settled(http, token, mounts).await?;
    provision_fingerprints(http, token, mounts).await?;
    let auth_mounted = http
        .call(
            Method::GET,
            &format!("sys/auth/{}", mounts.auth),
            token,
            None,
        )
        .await;
    if auth_mounted.is_err() {
        let body = json!({ "type": "approle" });
        http.call(
            Method::POST,
            &format!("sys/auth/{}", mounts.auth),
            token,
            Some(&body),
        )
        .await?;
    }
    if let Some(auth) = kubernetes {
        provision_kubernetes(http, token, mounts, auth).await?;
    }
    let namespace = kubernetes.and_then(|auth| auth.namespace.as_deref());
    let mut credentials = Vec::new();
    for identity in provisioned_identities(connections, namespace.is_some()) {
        let policy = policy_name(&identity, mounts);
        let hcl = provisioned_policy(&identity, connections, mounts, namespace)?;
        http.call(
            Method::PUT,
            &format!("sys/policies/acl/{policy}"),
            token,
            Some(&json!({ "policy": hcl })),
        )
        .await?;
        let role = format!("auth/{}/role/{}", mounts.auth, role_name(&identity));
        let mut settings = role_settings(&identity);
        if let Some(object) = settings.as_object_mut() {
            object.insert("token_policies".to_owned(), json!([policy]));
        }
        http.call(Method::POST, &role, token, Some(&settings))
            .await?;
    }
    for identity in secret_id_identities(connections, namespace.is_some()) {
        let role = format!("auth/{}/role/{}", mounts.auth, role_name(&identity));
        let role_id = http
            .call(Method::GET, &format!("{role}/role-id"), token, None)
            .await?;
        let issued = http
            .call(
                Method::POST,
                &format!("{role}/secret-id"),
                token,
                Some(&json!({})),
            )
            .await?;
        credentials.push(RoleCredentials {
            identity,
            role_id: text(role_id.as_ref(), "/data/role_id")?.to_owned(),
            secret_id: SecretString::from(text(issued.as_ref(), "/data/secret_id")?.to_owned()),
        });
    }
    Ok(credentials)
}
