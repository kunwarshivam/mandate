//! Each identity's AppRole credentials as a Kubernetes Secret (DEC-2630): the one naming rule the
//! provisioning step, the workspace chart and the deployment manager share, and the manifest the
//! step hands `kubectl` on its standard input. A manifest holds a secret id, so it is a
//! [`SecretString`] and never reaches a log, an error, or `Debug` output.

use std::collections::BTreeSet;

use secrecy::{ExposeSecret, SecretString};
use serde_json::json;

use crate::SourceError;
use crate::openbao::RoleCredentials;
use crate::policy::{Identity, connection_hex, role_name};

/// What every Secret's name starts with, before the identity's role.
pub const SECRET_PREFIX: &str = "vault-approle-";
/// The key, and so the mounted file's name, that holds the role id.
pub const ROLE_ID_KEY: &str = "role-id";
/// The key, and so the mounted file's name, that holds the secret id.
pub const SECRET_ID_KEY: &str = "secret-id";
/// The label every such Secret carries, naming what wrote it.
pub const MANAGED_BY_LABEL: &str = "app.kubernetes.io/managed-by";
/// That label's value.
pub const MANAGED_BY: &str = "mandate-vault-provision";
/// The annotation that names the vault role the Secret's credentials log in as. A role name may
/// hold a connection id's capitals and underscores, which a label value may not hold at its
/// length, so it is an annotation.
pub const ROLE_ANNOTATION: &str = "mandate.dev/vault-role";
/// The longest name a Kubernetes label, and so a namespace, may have.
const LABEL_MAX: usize = 63;

/// One identity's Secret: its name, and the manifest, as JSON, that creates it.
#[derive(Debug)]
pub struct SecretManifest {
    pub name: String,
    pub manifest: SecretString,
}

/// The name of `identity`'s Secret: `vault-approle-` and its role name, a connection id in it
/// written as the lowercase hexadecimal of its bytes, so two connections never share a name and
/// every name is a valid Kubernetes object name. A token exchange is refused
/// [`SourceError::Config`]: its credential is a single-use grant, never a standing Secret.
pub fn secret_name(identity: &Identity) -> Result<String, SourceError> {
    let role = match identity {
        Identity::TokenExchange(_) => return Err(SourceError::Config),
        Identity::Executor(connection) => format!("executor-{}", connection_hex(connection)),
        Identity::Api
        | Identity::ConnectionManager
        | Identity::Runtime
        | Identity::SecretImport
        | Identity::ModelGateway
        | Identity::EmailSender => role_name(identity),
    };
    Ok(format!("{SECRET_PREFIX}{role}"))
}

/// Whether `namespace` is a Kubernetes label: 1 to 63 lowercase letters, digits and hyphens,
/// starting and ending with a letter or digit.
pub(crate) fn is_label(namespace: &str) -> bool {
    let alphanumeric = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    namespace.len() <= LABEL_MAX
        && namespace
            .bytes()
            .all(|byte| alphanumeric(&byte) || byte == b'-')
        && namespace.as_bytes().first().is_some_and(alphanumeric)
        && namespace.as_bytes().last().is_some_and(alphanumeric)
}

/// The manifests of `credentials`' Secrets in `namespace`, in their order: each an `Opaque`
/// Secret named by [`secret_name`], labelled [`MANAGED_BY_LABEL`], annotated with its role, and
/// holding [`ROLE_ID_KEY`] and [`SECRET_ID_KEY`]. A namespace that is not a Kubernetes label, a
/// token exchange's credential, or two credentials of one identity refuse the whole set
/// [`SourceError::Config`], and no manifest is made.
pub fn kube_secrets(
    namespace: &str,
    credentials: &[RoleCredentials],
) -> Result<Vec<SecretManifest>, SourceError> {
    if !is_label(namespace) {
        return Err(SourceError::Config);
    }
    let mut names = BTreeSet::new();
    let mut manifests = Vec::new();
    for credential in credentials {
        let name = secret_name(&credential.identity)?;
        if !names.insert(name.clone()) {
            return Err(SourceError::Config);
        }
        let manifest = json!({
            "apiVersion": "v1",
            "kind": "Secret",
            "metadata": {
                "name": name,
                "namespace": namespace,
                "labels": {MANAGED_BY_LABEL: MANAGED_BY},
                "annotations": {ROLE_ANNOTATION: role_name(&credential.identity)}
            },
            "type": "Opaque",
            "stringData": {
                ROLE_ID_KEY: credential.role_id,
                SECRET_ID_KEY: credential.secret_id.expose_secret()
            }
        });
        manifests.push(SecretManifest {
            name,
            manifest: SecretString::from(manifest.to_string()),
        });
    }
    Ok(manifests)
}
