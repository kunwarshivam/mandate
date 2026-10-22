//! The connect finish's teardown over the vault ([DEC-2348](../../../docs/project/decisions/DEC-2348.md)
//! item 3): it deletes a connection's code and token as [`Identity::Api`], whose policy allows
//! those deletes and nothing else, and counts an absent entry as deleted.

use std::sync::Arc;

use mandate_connections::record::ConnectionId;

use crate::clock::Clock;
use mandate_vault::policy::{Identity, SecretPath};
use mandate_vault::source::SecretSource;

use crate::finish_pass::{DeleteFailed, Deleted, Teardown};
use crate::vault_codes::ApiLogin;

/// The [`Teardown`] the finish pass deletes through: a fresh login for each delete, made off the
/// async runtime.
pub struct VaultTeardown<L> {
    pub login: Arc<L>,
    pub clock: Arc<dyn Clock>,
}

impl<L: ApiLogin + 'static> Teardown for VaultTeardown<L> {
    fn delete<'a>(&'a self, connection: &'a ConnectionId) -> Deleted<'a> {
        let login = Arc::clone(&self.login);
        let clock = Arc::clone(&self.clock);
        let connection = connection.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                deleted(login.as_ref(), clock.as_ref(), &connection)
            })
            .await
            .unwrap_or(Err(DeleteFailed))
        })
    }
}

/// Logs in at the clock's time, requires [`Identity::Api`], then deletes `connection`'s code and
/// its token, both even when the first fails, on a current-thread runtime of its own.
fn deleted<L: ApiLogin>(
    login: &L,
    clock: &dyn Clock,
    connection: &ConnectionId,
) -> Result<(), DeleteFailed> {
    let now = clock.now().ok_or(DeleteFailed)?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| DeleteFailed)?
        .block_on(async {
            let session = login.login(now).await.map_err(|_| DeleteFailed)?;
            if session.identity() != &Identity::Api {
                return Err(DeleteFailed);
            }
            let code = session.delete(&SecretPath::Code(connection.clone())).await;
            let token = session.delete(&SecretPath::Token(connection.clone())).await;
            code.and(token).map_err(|_| DeleteFailed)
        })
}
