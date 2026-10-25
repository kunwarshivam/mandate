//! The vault for `run --fixture`.

use mandate_connections::record::ConnectionId;
use mandate_vault::provisioner::{RoleProvisioner, RoleRefusal};

/// The vault for `run --fixture`: it makes and removes every role at once and keeps nothing, so
/// the loop, its probes and its drain run without a vault. A build without debug assertions has
/// none.
#[derive(Debug, Default, Clone, Copy)]
pub struct FixtureVault;

impl RoleProvisioner for FixtureVault {
    async fn make_roles(
        &self,
        _connection: &ConnectionId,
        _namespace: &str,
    ) -> Result<(), RoleRefusal> {
        Ok(())
    }

    async fn remove_exchange_role(
        &self,
        _connection: &ConnectionId,
        _namespace: &str,
    ) -> Result<(), RoleRefusal> {
        Ok(())
    }
}
