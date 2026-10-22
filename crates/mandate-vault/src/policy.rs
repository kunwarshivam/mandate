//! Who may do what (infrastructure §5.2): one closed table, [`allows`], that the local backend
//! enforces in code and [`openbao_policy`] turns into one OpenBao policy per identity, with every
//! path spelled out and no wildcard.
//!
//! | Identity | Read | Write | Delete |
//! |---|---|---|---|
//! | API | — | every connection's code | every connection's code and token |
//! | Connection manager | — | — | every connection's code and token; it also issues and revokes token-exchange grants |
//! | Token exchange for `c` | `c`'s code, the client secret | `c`'s token, and `c`'s code only as [`Access::Take`] (DEC-2061) | `c`'s code |
//! | Executor for `c` | `c`'s token, as a lease | — | — |
//! | Runtime | — | — | — |
//! | Secret import | — | the client secret, the platform's own secrets | — |
//! | Model gateway | the model provider's key | — | — |
//! | Email sender | the email credential | — | — |
//!
//! The platform's own secrets ([`PlatformSecret`], DEC-2020) are each read by one identity and
//! written only by the secret import; none is ever deleted, and a rotation writes over it.

use mandate_connections::record::{AccountRef, ConnectionId};

use crate::SourceError;

/// The identity a process holds with the vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// The API process: writes the callback's code (connections spec §5.2 step 3).
    Api,
    /// The connection manager: issues the token-exchange grant and deletes a revoked
    /// connection's entries (§5.4).
    ConnectionManager,
    /// The token-exchange process for one connection, through a single-use grant (DEC-821
    /// item 2).
    TokenExchange(ConnectionId),
    /// The account executor for one connection.
    Executor(ConnectionId),
    /// An agent runtime: no credential, ever (CN-4).
    Runtime,
    /// `mandate vault import`: writes the platform's client secret, and reads nothing.
    SecretImport,
    /// The model gateway: reads the model provider's API key (DEC-2020).
    ModelGateway,
    /// The process that sends the platform's email: reads the email credential (DEC-2020).
    EmailSender,
}

/// One secret the vault holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretPath {
    /// A connection's waiting authorization code and PKCE verifier.
    Code(ConnectionId),
    /// A connection's access token.
    Token(ConnectionId),
    /// The platform's OAuth client secret.
    ClientSecret,
    /// The broker account id and its random reference of one connect of a connection, to one
    /// account stream, in the personal-data vault, outside `connections/` so no revoke deletes it
    /// (DEC-2339): a request again on the same connection to another account has its own.
    AccountId(ConnectionId, AccountRef),
    /// A secret of the platform's own that one process type reads (DEC-2020).
    Platform(PlatformSecret),
}

/// A secret of the platform's own, held for one process type, never an environment variable or a
/// committed file (DEC-2020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformSecret {
    /// The model provider's API key, the model gateway's (OpenRouter, DEC-997).
    ModelProviderKey,
    /// The credential the platform's email is sent with.
    EmailCredential,
}

impl PlatformSecret {
    pub const ALL: [Self; 2] = [Self::ModelProviderKey, Self::EmailCredential];
}

/// Who holds a [`PlatformSecret`]: the one identity that reads it and the one that writes it.
/// No identity deletes it; a rotation writes over it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holders {
    pub reads: Identity,
    pub writes: Identity,
}

/// The holders of `secret` (DEC-2020 item 2).
pub fn holders(secret: PlatformSecret) -> Result<Holders, SourceError> {
    let reads = match secret {
        PlatformSecret::ModelProviderKey => Identity::ModelGateway,
        PlatformSecret::EmailCredential => Identity::EmailSender,
    };
    Ok(Holders {
        reads,
        writes: Identity::SecretImport,
    })
}

/// What an identity does to a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    Delete,
    /// Overwrite an entry that is there, never create one: the token exchange's check-and-set
    /// tombstone on its own connection's code, which makes taking the code single use
    /// (DEC-2061). Only the Kubernetes-auth exchange's templated policy grants it; the AppRole
    /// policies, retired with that exchange, never do.
    Take,
    /// Write an entry that is not there, never overwrite or read one: a connection's executor's
    /// one write of its account id to the personal-data vault (DEC-2339).
    Create,
}

/// Whether `identity` may `access` `path`: the table above.
pub fn allows(identity: &Identity, access: Access, path: &SecretPath) -> bool {
    match (identity, access, path) {
        (Identity::Api, Access::Write, SecretPath::Code(_))
        | (
            Identity::Api | Identity::ConnectionManager,
            Access::Delete,
            SecretPath::Code(_) | SecretPath::Token(_),
        )
        | (Identity::TokenExchange(_), Access::Read, SecretPath::ClientSecret)
        | (Identity::SecretImport, Access::Write, SecretPath::ClientSecret) => true,
        (
            Identity::TokenExchange(own),
            Access::Read | Access::Delete | Access::Take,
            SecretPath::Code(c),
        )
        | (Identity::TokenExchange(own), Access::Write, SecretPath::Token(c))
        | (Identity::Executor(own), Access::Read, SecretPath::Token(c)) => own == c,
        (identity, Access::Read, SecretPath::Platform(secret)) => {
            holders(*secret).is_ok_and(|h| h.reads == *identity)
        }
        (identity, Access::Write, SecretPath::Platform(secret)) => {
            holders(*secret).is_ok_and(|h| h.writes == *identity)
        }
        _ => false,
    }
}

/// Where in OpenBao the cell's secrets and roles live: the KV v2 mount and the AppRole auth
/// mount. Policy names start with the KV mount, so two sets of mounts never share a policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mounts {
    pub kv: String,
    pub auth: String,
    /// The Kubernetes auth mount a connection's executor logs in on (DEC-2060).
    pub kubernetes_executor: String,
    /// The Kubernetes auth mount a connection's token exchange logs in on (DEC-2060).
    pub kubernetes_exchange: String,
}

impl Default for Mounts {
    /// `mandate`, `approle`, `kubernetes-executor` and `kubernetes-exchange`, the cell's.
    fn default() -> Self {
        Self {
            kv: "mandate".to_owned(),
            auth: "approle".to_owned(),
            kubernetes_executor: "kubernetes-executor".to_owned(),
            kubernetes_exchange: "kubernetes-exchange".to_owned(),
        }
    }
}

/// The name of `identity`'s OpenBao policy under `mounts`.
pub fn policy_name(identity: &Identity, mounts: &Mounts) -> String {
    format!("{}-{}", mounts.kv, role_name(identity))
}

/// The AppRole name of `identity`, one per process type and connection.
pub fn role_name(identity: &Identity) -> String {
    match identity {
        Identity::Api => "api".to_owned(),
        Identity::ConnectionManager => "connection-manager".to_owned(),
        Identity::TokenExchange(c) => format!("exchange-{}", c.as_str()),
        Identity::Executor(c) => format!("executor-{}", c.as_str()),
        Identity::Runtime => "runtime".to_owned(),
        Identity::SecretImport => "secret-import".to_owned(),
        Identity::ModelGateway => "model-gateway".to_owned(),
        Identity::EmailSender => "email-sender".to_owned(),
    }
}

/// `connection`'s bytes in lowercase hexadecimal: a valid Kubernetes object name and path segment
/// for every id, and, being lowercase, never one OpenBao folds into another's, as it folds role
/// and policy names (DEC-2060).
pub(crate) fn connection_hex(connection: &ConnectionId) -> String {
    connection
        .as_str()
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The one segment a connection's entries are stored under, `connections/<segment>/`: `x` and
/// [`connection_hex`], the token exchange's ServiceAccount name, so the templated policies of
/// both kinds reach exactly the path every writer and reader uses, and no two ids, whatever
/// their case, share it (DEC-2060).
pub fn connection_segment(connection: &ConnectionId) -> Result<String, SourceError> {
    Ok(format!("{SEGMENT_PREFIX}{}", connection_hex(connection)))
}

/// What comes before the hex in [`connection_segment`], and so before the executor's
/// ServiceAccount name in its templated policy.
pub(crate) const SEGMENT_PREFIX: &str = "x";

/// The path of `path` under the mount, without the mount and the `data/` or `metadata/` prefix.
pub fn kv_path(path: &SecretPath) -> Result<String, SourceError> {
    Ok(match path {
        SecretPath::Code(c) => format!("connections/{}/code", connection_segment(c)?),
        SecretPath::Token(c) => format!("connections/{}/token", connection_segment(c)?),
        SecretPath::ClientSecret => "platform/alpaca-oauth-client-secret".to_owned(),
        SecretPath::AccountId(c, account) => format!(
            "personal/accounts/{}/{}",
            connection_segment(c)?,
            account.as_str()
        ),
        SecretPath::Platform(PlatformSecret::ModelProviderKey) => {
            "platform/model-provider-key".to_owned()
        }
        SecretPath::Platform(PlatformSecret::EmailCredential) => {
            "platform/email-credential".to_owned()
        }
    })
}

/// The OpenBao policy of `identity`, given every connection the cell holds, in HCL. Each path is
/// written out for each connection; none holds `*` or `+`. A runtime's policy grants nothing: it
/// is a comment alone, since OpenBao refuses an empty policy.
pub fn openbao_policy(
    identity: &Identity,
    connections: &[ConnectionId],
    mounts: &Mounts,
) -> Result<String, SourceError> {
    let mut paths: Vec<SecretPath> = vec![SecretPath::ClientSecret];
    paths.extend(PlatformSecret::ALL.map(SecretPath::Platform));
    for connection in connections {
        paths.push(SecretPath::Code(connection.clone()));
        paths.push(SecretPath::Token(connection.clone()));
    }
    let mut hcl = String::new();
    for path in &paths {
        for (access, prefix, capabilities) in [
            (Access::Read, "data", "\"read\""),
            (Access::Write, "data", "\"create\", \"update\""),
            (Access::Delete, "metadata", "\"delete\""),
        ] {
            if allows(identity, access, path) {
                hcl.push_str(&format!(
                    "path \"{}/{prefix}/{}\" {{\n  capabilities = [{capabilities}]\n}}\n",
                    mounts.kv,
                    kv_path(path)?
                ));
            }
        }
    }
    if *identity == Identity::ConnectionManager {
        for connection in connections {
            let role = format!("auth/{}/role/exchange-{}", mounts.auth, connection.as_str());
            for (suffix, capability) in [
                ("secret-id", "update"),
                ("secret-id-accessor/lookup", "update"),
                ("secret-id-accessor/destroy", "update"),
                ("role-id", "read"),
            ] {
                hcl.push_str(&format!(
                    "path \"{role}/{suffix}\" {{\n  capabilities = [\"{capability}\"]\n}}\n"
                ));
            }
        }
    }
    if hcl.is_empty() {
        hcl.push_str("# This identity reads, writes, and deletes nothing (infrastructure §5.2).\n");
    }
    Ok(hcl)
}

/// Refuses as [`SourceError::Denied`] what [`allows`] does not allow.
pub fn require(identity: &Identity, access: Access, path: &SecretPath) -> Result<(), SourceError> {
    if allows(identity, access, path) {
        Ok(())
    } else {
        Err(SourceError::Denied)
    }
}
