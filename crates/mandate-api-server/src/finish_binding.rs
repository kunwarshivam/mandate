//! The connect finish's own process, `mandate-api-server finish`
//! ([DEC-2348](../../../docs/project/decisions/DEC-2348.md) item 3): it holds the connection
//! manager's vault identity and nothing else, so the API's own process keeps the API's identity
//! alone. It binds only when its four settings are all set and valid; otherwise it does not start,
//! and names them, never their values.

use std::ffi::OsString;
use std::path::PathBuf;

use mandate_identity::WorkspaceId;
use mandate_vault::mounted::read_mounted_credentials;
use mandate_vault::openbao::OpenBaoConfig;
use mandate_vault::policy::{Identity, Mounts};

use crate::config::check_database_url;
use crate::connect_binding::{address_in_form, ca_pem};
use crate::vault_codes::OpenBaoLogin;

/// The journal's connection string, the same one the API reads.
pub const DATABASE_URL: &str = "MANDATE_API_DATABASE_URL";

/// The hosted workspace's id, the same one the API reads.
pub const WORKSPACE: &str = "MANDATE_API_WORKSPACE";

/// The vault's `https://host:port`.
pub const FINISH_VAULT_ADDRESS: &str = "MANDATE_FINISH_VAULT_ADDRESS";

/// The directory holding `ca.crt`, `connection-manager.role-id` and
/// `connection-manager.secret-id`, each readable by the process alone. The connection manager's
/// pair must be there: any other identity's pair is never read in its place.
pub const FINISH_VAULT_DIR: &str = "MANDATE_FINISH_VAULT_DIR";

/// The four settings the finish needs, in the order they are judged and named.
pub const FINISH_SETTINGS: [&str; 4] = [
    DATABASE_URL,
    WORKSPACE,
    FINISH_VAULT_ADDRESS,
    FINISH_VAULT_DIR,
];

/// What a bound finish runs with.
#[derive(Debug)]
pub struct BoundFinish {
    pub database_url: String,
    /// The workspace's id as the control stream names it.
    pub workspace: String,
    /// The vault login, as the connection manager.
    pub login: OpenBaoLogin,
}

/// The finish `var` binds.
///
/// # Errors
/// The settings of [`FINISH_SETTINGS`] that are missing or invalid, in its order.
pub fn finish_binding(
    var: impl Fn(&str) -> Option<OsString>,
) -> Result<BoundFinish, Vec<&'static str>> {
    let text = |name: &str| var(name).and_then(|value| value.into_string().ok());
    let database_url =
        text(DATABASE_URL).filter(|url| !url.is_empty() && check_database_url(url).is_ok());
    let workspace = text(WORKSPACE)
        .and_then(|id| WorkspaceId::from_ulid_text(&id).ok())
        .and_then(|id| id.to_ulid_text().ok());
    let address = text(FINISH_VAULT_ADDRESS).filter(|address| address_in_form(address));
    let dir = text(FINISH_VAULT_DIR)
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute());
    let ca_pem = dir
        .as_deref()
        .filter(|dir| read_mounted_credentials(dir, &Identity::ConnectionManager).is_ok())
        .and_then(ca_pem);
    match (database_url, workspace, address, dir, ca_pem) {
        (Some(database_url), Some(workspace), Some(address), Some(dir), Some(ca_pem)) => {
            Ok(BoundFinish {
                database_url,
                workspace,
                login: OpenBaoLogin {
                    config: OpenBaoConfig {
                        address,
                        ca_pem,
                        mounts: Mounts::default(),
                    },
                    dir,
                    identity: Identity::ConnectionManager,
                },
            })
        }
        (database_url, workspace, address, dir, ca_pem) => {
            let valid = [
                database_url.is_some(),
                workspace.is_some(),
                address.is_some(),
                dir.is_some() && ca_pem.is_some(),
            ];
            Err(FINISH_SETTINGS
                .into_iter()
                .zip(valid)
                .filter(|(_, valid)| !valid)
                .map(|(name, _)| name)
                .collect())
        }
    }
}

/// The one line the finish writes when it does not start, naming settings only.
pub fn off_line(names: &[&str]) -> String {
    format!(
        "refused: finish off: {} missing or invalid",
        names.join(" ")
    )
}
