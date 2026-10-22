//! The connect finish's teardown over the vault ([DEC-2348](../../../docs/project/decisions/DEC-2348.md)
//! item 3): it deletes a connection's code and token as [`Identity::Api`], whose policy allows
//! those deletes and nothing else, and counts an absent entry as deleted.
//!
//! [`Identity::Api`]: mandate_vault::policy::Identity::Api

use std::sync::Arc;

use mandate_connections::record::ConnectionId;

use crate::clock::Clock;
use crate::finish_pass::{Deleted, Teardown};
use crate::vault_codes::ApiLogin;

/// The [`Teardown`] the finish pass deletes through: a fresh login for each delete, made off the
/// async runtime.
pub struct VaultTeardown<L> {
    pub login: Arc<L>,
    pub clock: Arc<dyn Clock>,
}

impl<L: ApiLogin + 'static> Teardown for VaultTeardown<L> {
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    fn delete<'a>(&'a self, _connection: &'a ConnectionId) -> Deleted<'a> {
        todo!()
    }
}
