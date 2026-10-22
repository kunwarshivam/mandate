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

use std::net::SocketAddrV4;

use serde_json::{Value, json};

use crate::SourceError;
use crate::kube::is_label;
use crate::lease::LEASE_SECS;
use crate::openbao::EXECUTOR_TOKEN_MAX_SECS;
use crate::policy::{
    Identity, Mounts, SEGMENT_PREFIX, SecretPath, connection_hex, connection_segment, kv_path,
};
use crate::source::GRANT_SECS;

/// The parameters a role request must send, every one it sends, in the order the provisioner's
/// policy names them, so no parameter is left to the mount's default.
const REQUIRED: [&str; 8] = [
    "alias_name_source",
    "audience",
    "bound_service_account_names",
    "bound_service_account_namespaces",
    "token_max_ttl",
    "token_no_default_policy",
    "token_policies",
    "token_ttl",
];

/// One kind's place: its auth mount, its namespace, its policy, and its token's lifetimes.
struct Kind<'a> {
    mount: &'a str,
    namespace: &'a str,
    policy: String,
    ttl: i64,
    max_ttl: i64,
}

/// Whether `segment` is one plain path segment: letters, digits, `_` and `-`, and not empty.
fn is_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Refuses, [`SourceError::Config`], Kubernetes mounts that are not plain path segments or a
/// namespace that is not a Kubernetes label.
fn checked(mounts: &Mounts, namespace: &str) -> Result<(), SourceError> {
    let plain = is_segment(&mounts.kubernetes_executor)
        && is_segment(&mounts.kubernetes_exchange)
        && is_label(namespace);
    if plain {
        Ok(())
    } else {
        Err(SourceError::Config)
    }
}

/// The executor's place and the exchange's, in that order.
fn kinds<'a>(mounts: &'a Mounts, namespace: &'a str) -> [Kind<'a>; 2] {
    [
        Kind {
            mount: &mounts.kubernetes_executor,
            namespace,
            policy: format!("{}-kubernetes-executor", mounts.kv),
            ttl: LEASE_SECS,
            max_ttl: EXECUTOR_TOKEN_MAX_SECS,
        },
        Kind {
            mount: &mounts.kubernetes_exchange,
            namespace,
            policy: format!("{}-kubernetes-exchange", mounts.kv),
            ttl: GRANT_SECS,
            max_ttl: GRANT_SECS,
        },
    ]
}

/// The audience every ServiceAccount token a process logs in with is projected for, so a token
/// the cluster's API server accepts is not one the vault accepts.
pub const AUDIENCE: &str = "mandate-vault";

/// The ServiceAccount a connection's executor or token exchange runs as: the connection id's
/// bytes in lowercase hexadecimal, after an `x` for the exchange, a valid Kubernetes name for
/// every id. The exchange's name is the path segment both kinds' policies reach.
pub fn service_account_name(identity: &Identity) -> Result<String, SourceError> {
    match identity {
        Identity::Executor(connection) => Ok(connection_hex(connection)),
        Identity::TokenExchange(connection) => connection_segment(connection),
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
/// executor's, so both kinds reach [`crate::policy::connection_segment`]. An executor reads its
/// connection's token; a token exchange writes it, reads and deletes its code, and reads the
/// platform's client secret. Anything else, or an accessor that is not one plain path segment (a
/// quote, a brace or a newline would end the template or the policy), is [`SourceError::Config`].
pub fn kubernetes_policy(
    identity: &Identity,
    mounts: &Mounts,
    accessor: &str,
) -> Result<String, SourceError> {
    if !is_segment(accessor) {
        return Err(SourceError::Config);
    }
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
            &format!("connections/{SEGMENT_PREFIX}{own}/token"),
            "\"read\"",
        )),
        Identity::TokenExchange(_) => Ok([
            block("data", &kv_path(&SecretPath::ClientSecret)?, "\"read\""),
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
/// policy, a token or a secret. Both kinds' ServiceAccounts are in `namespace`, the workspace's
/// own, so its default deny, egress policies and admission rules cover them. Kubernetes mounts
/// that are not one plain path segment, or a namespace that is not a Kubernetes label, are
/// [`SourceError::Config`].
pub fn provisioner_policy(mounts: &Mounts, namespace: &str) -> Result<String, SourceError> {
    checked(mounts, namespace)?;
    let required: Vec<String> = REQUIRED.iter().map(|name| format!("\"{name}\"")).collect();
    Ok(kinds(mounts, namespace)
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

/// The request that makes `identity`'s role: its path, `auth/<mount>/role/<name>`, the role named
/// by its ServiceAccount, since OpenBao folds role names to lowercase and two ids differing in case
/// would otherwise share one, and its body, bound to exactly its ServiceAccount in exactly `namespace`, the workspace's. Anything
/// but an executor or a token exchange, or mounts or a namespace [`provisioner_policy`] refuses, is
/// [`SourceError::Config`].
pub fn role_request(
    identity: &Identity,
    mounts: &Mounts,
    namespace: &str,
) -> Result<(String, Value), SourceError> {
    checked(mounts, namespace)?;
    let [executor, exchange] = kinds(mounts, namespace);
    let kind = match identity {
        Identity::Executor(_) => executor,
        Identity::TokenExchange(_) => exchange,
        Identity::Api
        | Identity::ConnectionManager
        | Identity::Runtime
        | Identity::SecretImport
        | Identity::ModelGateway
        | Identity::EmailSender => return Err(SourceError::Config),
    };
    let name = service_account_name(identity)?;
    Ok((
        format!("auth/{}/role/{name}", kind.mount),
        json!({
            "bound_service_account_names": [name],
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

/// How the vault reaches the cluster's API server for TokenReview (DEC-2060 item 7): its
/// `https://` address and, outside a pod, the PEM certificate of the CA that signed it. In the
/// cluster the vault reads its own pod's CA and ServiceAccount token, so `ca_pem` is `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KubernetesAuth {
    pub host: String,
    pub ca_pem: Option<String>,
}

/// The API server's address in `host`, which must be exactly `https://`, an IPv4 address and a
/// port from 1 to 65535, written the one way [`SocketAddrV4`] writes it: the vault sends its own
/// ServiceAccount token there on every TokenReview, so a name, userinfo, a path, a query, a
/// fragment, whitespace, a leading zero or a missing port is [`SourceError::Config`].
pub fn api_server_address(host: &str) -> Result<SocketAddrV4, SourceError> {
    let address: SocketAddrV4 = host
        .strip_prefix("https://")
        .and_then(|rest| rest.parse().ok())
        .ok_or(SourceError::Config)?;
    if address.port() == 0 || format!("https://{address}") != host {
        return Err(SourceError::Config);
    }
    Ok(address)
}

/// The requests that set both Kubernetes auth mounts up, executor's first: for each, enabling
/// the mount (`sys/auth/<mount>`, type `kubernetes`), then its configuration
/// (`auth/<mount>/config`). Mounts that are not plain path segments, a host that is not
/// [`api_server_address`]'s form, or a CA that is not one PEM certificate is [`SourceError::Config`].
pub fn kubernetes_mount_requests(
    mounts: &Mounts,
    auth: &KubernetesAuth,
) -> Result<Vec<(String, Value)>, SourceError> {
    let plain = is_segment(&mounts.kubernetes_executor) && is_segment(&mounts.kubernetes_exchange);
    let https = api_server_address(&auth.host).is_ok();
    let ca = auth.ca_pem.as_deref().is_none_or(is_certificate);
    if !(plain && https && ca) {
        return Err(SourceError::Config);
    }
    let mut config = serde_json::Map::new();
    config.insert("kubernetes_host".to_owned(), json!(auth.host));
    if let Some(pem) = &auth.ca_pem {
        config.insert("kubernetes_ca_cert".to_owned(), json!(pem));
    }
    Ok([&mounts.kubernetes_executor, &mounts.kubernetes_exchange]
        .into_iter()
        .flat_map(|mount| {
            [
                (format!("sys/auth/{mount}"), json!({"type": "kubernetes"})),
                (
                    format!("auth/{mount}/config"),
                    Value::Object(config.clone()),
                ),
            ]
        })
        .collect())
}

/// Whether `pem` is one PEM certificate block, begun and ended.
fn is_certificate(pem: &str) -> bool {
    let pem = pem.trim();
    pem.starts_with("-----BEGIN CERTIFICATE-----") && pem.ends_with("-----END CERTIFICATE-----")
}

/// The accessor of `mount` in the vault's `sys/auth` answer, the name the templated policies
/// key the login's alias by. One missing, or one that is not a plain path segment, so that it
/// could end the template, is [`SourceError::Malformed`].
pub fn mount_accessor(sys_auth: Option<&Value>, mount: &str) -> Result<String, SourceError> {
    sys_auth
        .and_then(|listed| listed.get("data"))
        .and_then(|data| data.get(format!("{mount}/")))
        .and_then(|entry| entry.get("accessor"))
        .and_then(Value::as_str)
        .filter(|accessor| is_segment(accessor))
        .map(str::to_owned)
        .ok_or(SourceError::Malformed)
}
