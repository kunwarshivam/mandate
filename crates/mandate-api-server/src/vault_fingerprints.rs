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
use mandate_vault::fingerprint::ManagerFingerprints;

use crate::clock::Clock;
use crate::finish::{Fingerprints, Judging, Released};
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
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn judge<'a>(&'a self, _request: &'a RequestMembers, _account_pii_ref: &'a str) -> Judging<'a> {
        todo!()
    }

    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn release<'a>(
        &'a self,
        _connection: &'a ConnectionId,
        _account_ref: &'a AccountRef,
    ) -> Released<'a> {
        todo!()
    }
}
