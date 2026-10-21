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

use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;
use reqwest::{Certificate, Method, StatusCode, redirect};
use secrecy::zeroize::Zeroize;
use secrecy::{ExposeSecret, SecretSlice, SecretString};
use serde_json::{Value, json};

use crate::SourceError;
use crate::lease::{LEASE_SECS, Lease, lease_times};
use crate::policy::{
    Identity, Mounts, SecretPath, kv_path, openbao_policy, policy_name, role_name,
};
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

/// The HTTP client for one vault: TLS to the given CA only, no proxy, no redirect.
struct Http {
    client: reqwest::Client,
    address: String,
}

impl Http {
    fn new(config: &OpenBaoConfig) -> Result<Self, SourceError> {
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
        })
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
        refusal(status)?;
        let mut bytes = response
            .bytes()
            .await
            .map_err(|_| SourceError::Unavailable)?
            .to_vec();
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

/// A logged-in session as one identity.
pub struct OpenBaoSource {
    identity: Identity,
    http: Http,
    mounts: Mounts,
    token: SecretString,
    token_expires_at: Mutex<UtcNanos>,
}

impl fmt::Debug for OpenBaoSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenBaoSource")
            .field("identity", &self.identity)
            .field("address", &self.http.address)
            .finish_non_exhaustive()
    }
}

impl OpenBaoSource {
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
        let encoded = text(value.as_ref(), "/data/data/value")?;
        STANDARD
            .decode(encoded)
            .map(|bytes| Some(SecretSlice::from(bytes)))
            .map_err(|_| SourceError::Malformed)
    }

    fn role_path(&self, connection: &ConnectionId) -> String {
        format!(
            "auth/{}/role/{}",
            self.mounts.auth,
            role_name(&Identity::TokenExchange(connection.clone()))
        )
    }
}

impl SecretSource for OpenBaoSource {
    fn identity(&self) -> &Identity {
        &self.identity
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
pub(crate) fn secret_id_identities(connections: &[ConnectionId]) -> Vec<Identity> {
    provisioned_identities(connections)
        .into_iter()
        .filter(|identity| !matches!(identity, Identity::TokenExchange(_)))
        .collect()
}

/// Every identity [`provision`] makes a policy and a role for: each process type's, then each
/// connection's executor and token exchange, in that order.
pub(crate) fn provisioned_identities(connections: &[ConnectionId]) -> Vec<Identity> {
    let mut identities = vec![
        Identity::Api,
        Identity::ConnectionManager,
        Identity::Runtime,
        Identity::SecretImport,
        Identity::ModelGateway,
        Identity::EmailSender,
    ];
    for connection in connections {
        identities.push(Identity::Executor(connection.clone()));
        identities.push(Identity::TokenExchange(connection.clone()));
    }
    identities
}

/// Sets a vault up for `connections`, with an operator's token: the KV v2 mount, the AppRole
/// auth method, one policy and one role per identity, and a secret id for every identity but the
/// token exchanges, whose secret ids are grants. Running it again rewrites the policies and
/// roles, so adding a connection is running it with the longer list.
pub async fn provision(
    config: &OpenBaoConfig,
    operator_token: &SecretString,
    connections: &[ConnectionId],
) -> Result<Vec<RoleCredentials>, SourceError> {
    let http = Http::new(config)?;
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
    let mut credentials = Vec::new();
    for identity in provisioned_identities(connections) {
        let policy = policy_name(&identity, mounts);
        let hcl = openbao_policy(&identity, connections, mounts)?;
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
    for identity in secret_id_identities(connections) {
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
