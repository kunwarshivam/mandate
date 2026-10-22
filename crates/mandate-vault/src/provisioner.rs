//! The role provisioner's two operations on the vault (DEC-2350 items 1 and 3; DEC-2630 item 7):
//! make a connection's two Kubernetes-auth roles, and remove its token exchange's. There is no
//! operation that removes an executor's role: an executor started for any reason, a kill switch's
//! among them, can always log in (DEC-2350 item 3), and the provisioner's policy refuses it too.

use mandate_connections::record::ConnectionId;

use crate::SourceError;

/// Why the vault did not make or remove a role, in §9.33's `ConnectionRolesRefused` vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleRefusal {
    /// The vault could not be reached, or was sealed.
    VaultUnavailable,
    /// The vault refused the role, or the request for it could not be formed.
    RoleRefused,
    /// The report of an operation E21-33 has not implemented yet (DEC-77); the implementation
    /// removes it.
    Unimplemented { story: &'static str },
}

impl RoleRefusal {
    /// The refusal's `reason` as `ConnectionRolesRefused` journals it.
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::VaultUnavailable => "vault_unavailable",
            Self::RoleRefused | Self::Unimplemented { .. } => "role_refused",
        }
    }

    /// The refusal a vault error is: an outage or a sealed vault is unavailable, and every other
    /// refusal, a denied or malformed request among them, refuses the role.
    #[must_use]
    pub fn of(error: &SourceError) -> Self {
        let _ = error;
        Self::Unimplemented { story: "E21-33" }
    }
}

/// The role provisioner's session with the vault, as [`crate::openbao::OpenBaoSource`] is when it
/// logs in as [`crate::policy::Identity::Provisioner`]: a trait, so the process scripts it in its
/// own tests.
#[allow(
    async_fn_in_trait,
    reason = "every caller is in this workspace and awaits on its own runtime; no Send bound is needed"
)]
pub trait RoleProvisioner {
    /// Makes `connection`'s executor role and then its token exchange's, each bound to its
    /// ServiceAccount in `namespace`, the workspace's, as
    /// [`crate::kubernetes::role_request`] forms them. Making a role that exists rewrites it to
    /// the same, so a make after a crash changes nothing.
    ///
    /// # Errors
    /// The [`RoleRefusal`] of the first role the vault did not make.
    async fn make_roles(
        &self,
        connection: &ConnectionId,
        namespace: &str,
    ) -> Result<(), RoleRefusal>;

    /// Removes `connection`'s token exchange's role; one that is not there changes nothing.
    ///
    /// # Errors
    /// The [`RoleRefusal`] when the vault did not remove it.
    async fn remove_exchange_role(
        &self,
        connection: &ConnectionId,
        namespace: &str,
    ) -> Result<(), RoleRefusal>;
}
