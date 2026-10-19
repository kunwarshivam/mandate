//! Per-connection processes log in with OpenBao's Kubernetes auth (DEC-2060): each connection's
//! executor and token exchange run as the ServiceAccount named by the connection id's hex, each
//! kind on its own auth mount and in its own namespace, and one templated policy per kind reads
//! the path segment from the login's own ServiceAccount name, which the vault checks with
//! TokenReview on every login. So no policy is ever written per connection, and the dedicated
//! provisioner (DEC-2605 row 23) needs no power over policies: it may only create, on connect,
//! one role per connection, bound to exactly that ServiceAccount in exactly that namespace, with
//! the one policy, audience and lifetimes pinned.

use serde_json::Value;

use crate::SourceError;
use crate::policy::{Identity, Mounts};

/// The audience every ServiceAccount token a process logs in with is projected for, so a token
/// the cluster's API server accepts is not one the vault accepts.
pub const AUDIENCE: &str = "mandate-vault";

/// Where the per-connection roles live: the Kubernetes auth mount of each kind, and the namespace
/// each kind's ServiceAccounts are in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KubernetesLayout {
    pub executor_mount: String,
    pub exchange_mount: String,
    pub executor_namespace: String,
    pub exchange_namespace: String,
}

/// The ServiceAccount a connection's executor or token exchange runs as: the connection id's
/// bytes in lowercase hexadecimal, a valid Kubernetes name for every id, and the path segment its
/// policy reads.
pub fn service_account_name(identity: &Identity) -> Result<String, SourceError> {
    let _ = identity;
    Err(SourceError::Unimplemented { story: "E21-32" })
}

/// The name of the templated policy of `identity`'s kind under `mounts`.
pub fn kubernetes_policy_name(identity: &Identity, mounts: &Mounts) -> Result<String, SourceError> {
    let _ = (identity, mounts);
    Err(SourceError::Unimplemented { story: "E21-32" })
}

/// The templated policy, in HCL, of `identity`'s kind, keyed by the ServiceAccount name in the
/// login's alias metadata on the auth mount whose accessor is `accessor`. An executor reads its
/// connection's token; a token exchange writes it, reads and deletes its code, and reads the
/// platform's client secret. Anything else is [`SourceError::Config`].
pub fn kubernetes_policy(
    identity: &Identity,
    mounts: &Mounts,
    accessor: &str,
) -> Result<String, SourceError> {
    let _ = (identity, mounts, accessor);
    Err(SourceError::Unimplemented { story: "E21-32" })
}

/// The provisioner's policy, in HCL: it may create, update and read roles on the two Kubernetes
/// auth mounts and nothing else, every parameter but the ServiceAccount's name pinned to its one
/// value, so it can neither widen a role's namespace, policy, audience or lifetime nor reach a
/// policy, a token or a secret. A layout whose mounts are not one plain path segment, or whose
/// namespaces are not Kubernetes labels, is [`SourceError::Config`].
pub fn provisioner_policy(
    mounts: &Mounts,
    layout: &KubernetesLayout,
) -> Result<String, SourceError> {
    let _ = (mounts, layout);
    Err(SourceError::Unimplemented { story: "E21-32" })
}

/// The request that makes `identity`'s role: its path, `auth/<mount>/role/<connection id>`, and
/// its body, bound to exactly its ServiceAccount in exactly its namespace. Anything but an
/// executor or a token exchange is [`SourceError::Config`].
pub fn role_request(
    identity: &Identity,
    mounts: &Mounts,
    layout: &KubernetesLayout,
) -> Result<(String, Value), SourceError> {
    let _ = (identity, mounts, layout);
    Err(SourceError::Unimplemented { story: "E21-32" })
}
