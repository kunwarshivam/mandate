//! The connect finish's own process, `mandate-api-server finish`
//! ([DEC-2348](../../../docs/project/decisions/DEC-2348.md) item 3): it holds the connection
//! manager's vault identity and nothing else, so the API's own process keeps the API's identity
//! alone. It binds only when its four settings are all set and valid; otherwise it does not start,
//! and names them, never their values.

use std::ffi::OsString;

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
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn finish_binding(
    _var: impl Fn(&str) -> Option<OsString>,
) -> Result<BoundFinish, Vec<&'static str>> {
    todo!()
}

/// The one line the finish writes when it does not start, naming settings only.
pub fn off_line(names: &[&str]) -> String {
    format!(
        "refused: finish off: {} missing or invalid",
        names.join(" ")
    )
}
