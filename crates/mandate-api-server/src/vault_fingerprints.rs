//! The connect finish's fingerprint port over the vault
//! ([DEC-2348](../../../docs/project/decisions/DEC-2348.md) item 4, DEC-2345): checks 3 and 4
//! judged from the fingerprint entry the request's executor wrote, read and claimed as
//! [`Identity::ConnectionManager`]. The API receives the hash and the personal-data reference,
//! never the account id (connections spec §3.1).
//!
//! [`Identity::ConnectionManager`]: mandate_vault::policy::Identity::ConnectionManager

use std::sync::Arc;

use mandate_connections::manager::RequestMembers;
use mandate_connections::record::{AccountRef, ConnectionId};
use mandate_vault::fingerprint::{Claim, ManagerFingerprints};
use mandate_vault::policy::Identity;
use mandate_vault::source::SecretSource;

use crate::clock::Clock;
use crate::finish::{Fingerprints, Judged, Judging, ReleaseFailed, Released};
use crate::vault_codes::ApiLogin;

/// The [`Fingerprints`] the finish pass judges and releases through: a fresh login for each call,
/// made off the async runtime, as the teardown's.
pub struct VaultFingerprints<L> {
    pub login: Arc<L>,
    pub clock: Arc<dyn Clock>,
}

impl<L> Fingerprints for VaultFingerprints<L>
where
    L: ApiLogin + 'static,
    L::Session: ManagerFingerprints,
{
    fn judge<'a>(&'a self, request: &'a RequestMembers, account_pii_ref: &'a str) -> Judging<'a> {
        let login = Arc::clone(&self.login);
        let clock = Arc::clone(&self.clock);
        let connection = request.connection_id.clone();
        let account = request.account_ref.clone();
        let pii_ref = account_pii_ref.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                off_runtime(async {
                    let session = session(login.as_ref(), clock.as_ref()).await?;
                    Ok(judged(&session, &connection, &account, &pii_ref).await)
                })
                .unwrap_or(Judged::CannotAnswer)
            })
            .await
            .unwrap_or(Judged::CannotAnswer)
        })
    }

    fn release<'a>(
        &'a self,
        connection: &'a ConnectionId,
        account_ref: &'a AccountRef,
    ) -> Released<'a> {
        let login = Arc::clone(&self.login);
        let clock = Arc::clone(&self.clock);
        let connection = connection.clone();
        let account = account_ref.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                off_runtime(async {
                    let session = session(login.as_ref(), clock.as_ref()).await?;
                    session
                        .release(&connection, &account)
                        .await
                        .map_err(|_| Unreached)
                })
                .map_err(|Unreached| ReleaseFailed)
            })
            .await
            .unwrap_or(Err(ReleaseFailed))
        })
    }
}

/// A call that did not reach the connection manager's vault session, or that it refused.
struct Unreached;

/// The answer of `work`, run on a current-thread runtime of its own, since a vault session's
/// futures are not `Send`; [`Unreached`] when no runtime can be built.
fn off_runtime<T>(work: impl Future<Output = Result<T, Unreached>>) -> Result<T, Unreached> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Unreached)
        .and_then(|runtime| runtime.block_on(work))
}

/// A fresh session at the clock's time, required to be the connection manager's.
async fn session<L: ApiLogin>(login: &L, clock: &dyn Clock) -> Result<L::Session, Unreached> {
    let now = clock.now().ok_or(Unreached)?;
    let session = login.login(now).await.map_err(|_| Unreached)?;
    if session.identity() == &Identity::ConnectionManager {
        Ok(session)
    } else {
        Err(Unreached)
    }
}

/// Checks 3 and 4 for the request `account` of `connection`: its fingerprint entry, whose
/// reference must be the check's `pii_ref`, then a claim of its hash (DEC-2345 items 3 and 4).
async fn judged<S: ManagerFingerprints>(
    session: &S,
    connection: &ConnectionId,
    account: &AccountRef,
    pii_ref: &str,
) -> Judged {
    let Ok((fingerprint, entry_pii_ref)) = session.fingerprint(connection, account).await else {
        return Judged::CannotAnswer;
    };
    if entry_pii_ref.as_str() != pii_ref {
        return Judged::CannotAnswer;
    }
    match session.claim(connection, &fingerprint).await {
        Ok(Claim::Claimed) => Judged::Unique,
        Ok(Claim::HeldBy(holder)) => Judged::AlreadyConnected(holder),
        Err(_) => Judged::CannotAnswer,
    }
}
