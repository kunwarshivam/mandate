//! Who may do what (infrastructure §5.2): one closed table, [`allows`], that the local backend
//! enforces in code and [`openbao_policy`] turns into one OpenBao policy per identity, with every
//! path spelled out and no wildcard.
//!
//! | Identity | Read | Write | Delete |
//! |---|---|---|---|
//! | API | — | every connection's code | every connection's code and token |
//! | Connection manager | — | — | every connection's code and token; it also issues and revokes token-exchange grants |
//! | Token exchange for `c` (through a grant) | `c`'s code, the client secret | `c`'s token | `c`'s code |
//! | Executor for `c` | `c`'s token, as a lease | — | — |
//! | Runtime | — | — | — |
//! | Secret import | — | the client secret | — |

use mandate_connections::record::ConnectionId;

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
}

/// What an identity does to a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    Delete,
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
        (Identity::TokenExchange(own), Access::Read | Access::Delete, SecretPath::Code(c))
        | (Identity::TokenExchange(own), Access::Write, SecretPath::Token(c))
        | (Identity::Executor(own), Access::Read, SecretPath::Token(c)) => own == c,
        _ => false,
    }
}

/// Where in OpenBao the cell's secrets and roles live: the KV v2 mount and the AppRole auth
/// mount. Policy names start with the KV mount, so two sets of mounts never share a policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mounts {
    pub kv: String,
    pub auth: String,
}

impl Default for Mounts {
    /// `mandate` and `approle`, the cell's.
    fn default() -> Self {
        Self {
            kv: "mandate".to_owned(),
            auth: "approle".to_owned(),
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
    }
}

/// The path of `path` under the mount, without the mount and the `data/` or `metadata/` prefix.
pub fn kv_path(path: &SecretPath) -> String {
    match path {
        SecretPath::Code(c) => format!("connections/{}/code", c.as_str()),
        SecretPath::Token(c) => format!("connections/{}/token", c.as_str()),
        SecretPath::ClientSecret => "platform/alpaca-oauth-client-secret".to_owned(),
    }
}

/// The OpenBao policy of `identity`, given every connection the cell holds, in HCL. Each path is
/// written out for each connection; none holds `*` or `+`. A runtime's policy grants nothing: it
/// is a comment alone, since OpenBao refuses an empty policy.
pub fn openbao_policy(
    identity: &Identity,
    connections: &[ConnectionId],
    mounts: &Mounts,
) -> String {
    let mut paths: Vec<SecretPath> = vec![SecretPath::ClientSecret];
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
                    kv_path(path)
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
    hcl
}

/// Refuses as [`SourceError::Denied`] what [`allows`] does not allow.
pub fn require(identity: &Identity, access: Access, path: &SecretPath) -> Result<(), SourceError> {
    if allows(identity, access, path) {
        Ok(())
    } else {
        Err(SourceError::Denied)
    }
}
