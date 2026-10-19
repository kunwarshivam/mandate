//! Per-connection processes log in with OpenBao's Kubernetes auth (DEC-2060): each connection's
//! executor runs as the ServiceAccount named by the connection id's hex, and its token exchange as
//! that name after an `x`, both in the workspace's namespace, each kind on its own auth mount; one
//! templated policy per kind reads the path segment, `x` and the hex, from the login's own
//! ServiceAccount name, which the vault checks with TokenReview on every login. An executor's
//! name has an even number of hex digits and an exchange's an odd number of characters, so no
//! name is ever both. So no policy is ever written per connection, and the dedicated
//! provisioner (DEC-2605 row 23) needs no power over policies: it may only create, on connect,
//! one role per connection, bound to exactly that ServiceAccount in exactly that namespace, with
//! the one policy, audience and lifetimes pinned.

use mandate_connections::record::ConnectionId;
use serde_json::{Value, json};

use crate::SourceError;
use crate::kube::is_label;
use crate::lease::LEASE_SECS;
use crate::openbao::EXECUTOR_TOKEN_MAX_SECS;
use crate::policy::{Identity, Mounts, SecretPath, kv_path};
use crate::source::GRANT_SECS;

/// The parameters a role request must send, in the order the provisioner's policy names them.
const REQUIRED: [&str; 4] = [
    "bound_service_account_names",
    "bound_service_account_namespaces",
    "audience",
    "token_policies",
];

/// One kind's place: its auth mount, its namespace, its policy, and its token's lifetimes.
struct Kind<'a> {
    mount: &'a str,
    namespace: &'a str,
    policy: String,
    ttl: i64,
    max_ttl: i64,
}

/// `connection`'s bytes in lowercase hexadecimal.
fn hex(connection: &ConnectionId) -> String {
    connection
        .as_str()
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Whether `segment` is one plain path segment: letters, digits, `_` and `-`, and not empty.
fn is_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `layout`, refused [`SourceError::Config`] unless its mounts are plain segments and its
/// namespaces Kubernetes labels.
fn checked(layout: &KubernetesLayout) -> Result<&KubernetesLayout, SourceError> {
    let plain = is_segment(&layout.executor_mount)
        && is_segment(&layout.exchange_mount)
        && is_label(&layout.namespace);
    if plain {
        Ok(layout)
    } else {
        Err(SourceError::Config)
    }
}

/// The executor's place and the exchange's, in that order.
fn kinds<'a>(mounts: &Mounts, layout: &'a KubernetesLayout) -> [Kind<'a>; 2] {
    [
        Kind {
            mount: &layout.executor_mount,
            namespace: &layout.namespace,
            policy: format!("{}-kubernetes-executor", mounts.kv),
            ttl: LEASE_SECS,
            max_ttl: EXECUTOR_TOKEN_MAX_SECS,
        },
        Kind {
            mount: &layout.exchange_mount,
            namespace: &layout.namespace,
            policy: format!("{}-kubernetes-exchange", mounts.kv),
            ttl: GRANT_SECS,
            max_ttl: GRANT_SECS,
        },
    ]
}

/// The audience every ServiceAccount token a process logs in with is projected for, so a token
/// the cluster's API server accepts is not one the vault accepts.
pub const AUDIENCE: &str = "mandate-vault";

/// Where the per-connection roles live: the Kubernetes auth mount of each kind, and the namespace
/// both kinds' ServiceAccounts are in, the workspace's own, so its default deny, its egress
/// policies and its admission rules cover them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KubernetesLayout {
    pub executor_mount: String,
    pub exchange_mount: String,
    pub namespace: String,
}

/// The ServiceAccount a connection's executor or token exchange runs as: the connection id's
/// bytes in lowercase hexadecimal, after an `x` for the exchange, a valid Kubernetes name for
/// every id. The exchange's name is the path segment both kinds' policies reach.
pub fn service_account_name(identity: &Identity) -> Result<String, SourceError> {
    match identity {
        Identity::Executor(connection) => Ok(hex(connection)),
        Identity::TokenExchange(connection) => Ok(format!("x{}", hex(connection))),
        Identity::Api
        | Identity::ConnectionManager
        | Identity::Runtime
        | Identity::SecretImport
        | Identity::ModelGateway
        | Identity::EmailSender => Err(SourceError::Config),
    }
}

/// The name of the templated policy of `identity`'s kind under `mounts`.
pub fn kubernetes_policy_name(identity: &Identity, mounts: &Mounts) -> Result<String, SourceError> {
    match identity {
        Identity::Executor(_) => Ok(format!("{}-kubernetes-executor", mounts.kv)),
        Identity::TokenExchange(_) => Ok(format!("{}-kubernetes-exchange", mounts.kv)),
        Identity::Api
        | Identity::ConnectionManager
        | Identity::Runtime
        | Identity::SecretImport
        | Identity::ModelGateway
        | Identity::EmailSender => Err(SourceError::Config),
    }
}

/// The templated policy, in HCL, of `identity`'s kind, keyed by the ServiceAccount name in the
/// login's alias metadata on the auth mount whose accessor is `accessor`, an `x` before an
/// executor's. An executor reads its connection's token; a token exchange writes it, reads and
/// deletes its code, and reads the platform's client secret. Anything else is
/// [`SourceError::Config`].
pub fn kubernetes_policy(
    identity: &Identity,
    mounts: &Mounts,
    accessor: &str,
) -> Result<String, SourceError> {
    let own = format!("{{{{identity.entity.aliases.{accessor}.metadata.service_account_name}}}}");
    let block = |prefix: &str, path: &str, capabilities: &str| {
        format!(
            "path \"{}/{prefix}/{path}\" {{\n  capabilities = [{capabilities}]\n}}\n",
            mounts.kv
        )
    };
    match identity {
        Identity::Executor(_) => Ok(block(
            "data",
            &format!("connections/x{own}/token"),
            "\"read\"",
        )),
        Identity::TokenExchange(_) => Ok([
            block("data", &kv_path(&SecretPath::ClientSecret), "\"read\""),
            block("data", &format!("connections/{own}/code"), "\"read\""),
            block("metadata", &format!("connections/{own}/code"), "\"delete\""),
            block(
                "data",
                &format!("connections/{own}/token"),
                "\"create\", \"update\"",
            ),
        ]
        .concat()),
        Identity::Api
        | Identity::ConnectionManager
        | Identity::Runtime
        | Identity::SecretImport
        | Identity::ModelGateway
        | Identity::EmailSender => Err(SourceError::Config),
    }
}

/// The provisioner's policy, in HCL: it may create, update and read roles on the two Kubernetes
/// auth mounts and nothing else, every parameter but the ServiceAccount's name pinned to its one
/// value, so it can neither widen a role's namespace, policy, audience or lifetime nor reach a
/// policy, a token or a secret. A layout whose mounts are not one plain path segment, or whose
/// namespace is not a Kubernetes label, is [`SourceError::Config`].
pub fn provisioner_policy(
    mounts: &Mounts,
    layout: &KubernetesLayout,
) -> Result<String, SourceError> {
    let layout = checked(layout)?;
    let required: Vec<String> = REQUIRED.iter().map(|name| format!("\"{name}\"")).collect();
    Ok(kinds(mounts, layout)
        .iter()
        .map(|kind| {
            format!(
                "path \"auth/{mount}/role/+\" {{\n  \
                 capabilities = [\"create\", \"update\", \"read\"]\n  \
                 required_parameters = [{required}]\n  \
                 allowed_parameters = {{\n    \
                 \"alias_name_source\" = [\"serviceaccount_uid\"]\n    \
                 \"audience\" = [\"{AUDIENCE}\"]\n    \
                 \"bound_service_account_names\" = []\n    \
                 \"bound_service_account_namespaces\" = [\"{namespace}\"]\n    \
                 \"token_max_ttl\" = [\"{max_ttl}\"]\n    \
                 \"token_no_default_policy\" = [\"false\"]\n    \
                 \"token_policies\" = [\"{policy}\"]\n    \
                 \"token_ttl\" = [\"{ttl}\"]\n  \
                 }}\n}}\n",
                mount = kind.mount,
                required = required.join(", "),
                namespace = kind.namespace,
                max_ttl = kind.max_ttl,
                policy = kind.policy,
                ttl = kind.ttl,
            )
        })
        .collect())
}

/// The request that makes `identity`'s role: its path, `auth/<mount>/role/<connection id>`, and
/// its body, bound to exactly its ServiceAccount in exactly the workspace's namespace. Anything but an
/// executor or a token exchange is [`SourceError::Config`].
pub fn role_request(
    identity: &Identity,
    mounts: &Mounts,
    layout: &KubernetesLayout,
) -> Result<(String, Value), SourceError> {
    let layout = checked(layout)?;
    let [executor, exchange] = kinds(mounts, layout);
    let (kind, connection) = match identity {
        Identity::Executor(connection) => (executor, connection),
        Identity::TokenExchange(connection) => (exchange, connection),
        Identity::Api
        | Identity::ConnectionManager
        | Identity::Runtime
        | Identity::SecretImport
        | Identity::ModelGateway
        | Identity::EmailSender => return Err(SourceError::Config),
    };
    Ok((
        format!("auth/{}/role/{}", kind.mount, connection.as_str()),
        json!({
            "bound_service_account_names": [service_account_name(identity)?],
            "bound_service_account_namespaces": [kind.namespace],
            "audience": AUDIENCE,
            "alias_name_source": "serviceaccount_uid",
            "token_policies": [kind.policy],
            "token_ttl": kind.ttl.to_string(),
            "token_max_ttl": kind.max_ttl.to_string(),
            "token_no_default_policy": "false"
        }),
    ))
}
