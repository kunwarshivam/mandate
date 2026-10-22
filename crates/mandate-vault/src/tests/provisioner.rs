//! The role provisioner's refusals (DEC-2350 item 1): every vault error is one of §9.33's two
//! `ConnectionRolesRefused` reasons, an outage or a sealed vault `vault_unavailable` and every
//! other refusal `role_refused`.

use mandate_connections::record::ConnectionId;

use crate::SourceError;
use crate::openbao::{OpenBaoConfig, OpenBaoSource};
use crate::policy::{Identity, Mounts};
use crate::provisioner::{RoleProvisioner, RoleRefusal};

#[test]
fn every_vault_error_is_one_of_the_two_reasons() {
    for (error, refusal) in [
        (SourceError::Unavailable, RoleRefusal::VaultUnavailable),
        (SourceError::Sealed, RoleRefusal::VaultUnavailable),
        (SourceError::Denied, RoleRefusal::RoleRefused),
        (SourceError::Missing, RoleRefusal::RoleRefused),
        (SourceError::Malformed, RoleRefusal::RoleRefused),
        (SourceError::Config, RoleRefusal::RoleRefused),
        (SourceError::RoleMissing, RoleRefusal::RoleRefused),
    ] {
        assert_eq!(RoleRefusal::of(&error), refusal, "{error:?}");
    }
    assert_eq!(RoleRefusal::VaultUnavailable.reason(), "vault_unavailable");
    assert_eq!(RoleRefusal::RoleRefused.reason(), "role_refused");
}

/// A session as `identity` for a vault nobody listens at: a call that gets past the checks finds
/// no vault and is `vault_unavailable`.
fn session(identity: Identity) -> OpenBaoSource {
    let config = OpenBaoConfig {
        address: "https://127.0.0.1:1".to_owned(),
        ca_pem: include_bytes!("fixtures/isrg-root-x1.pem").to_vec(),
        mounts: Mounts::default(),
    };
    OpenBaoSource::unlogged(&config, identity).unwrap()
}

/// Only the provisioner's own session makes or removes roles (DEC-2350 item 5): any other
/// identity is refused `role_refused` before the vault is reached, whatever its policy would
/// answer, and the provisioner's goes on to the vault.
#[tokio::test]
#[ignore = "pending E21-33"]
async fn only_the_provisioners_session_makes_or_removes_roles() {
    let connection = ConnectionId::new("c1").unwrap();
    for identity in [
        Identity::Api,
        Identity::ConnectionManager,
        Identity::TokenExchange(connection.clone()),
        Identity::Executor(connection.clone()),
        Identity::Runtime,
        Identity::SecretImport,
        Identity::ModelGateway,
        Identity::EmailSender,
    ] {
        let vault = session(identity.clone());
        assert_eq!(
            vault.make_roles(&connection, "ws-1").await,
            Err(RoleRefusal::RoleRefused),
            "{identity:?}"
        );
        assert_eq!(
            vault.remove_exchange_role(&connection, "ws-1").await,
            Err(RoleRefusal::RoleRefused),
            "{identity:?}"
        );
    }
    let vault = session(Identity::Provisioner);
    assert_eq!(
        vault.make_roles(&connection, "ws-1").await,
        Err(RoleRefusal::VaultUnavailable)
    );
    assert_eq!(
        vault.remove_exchange_role(&connection, "ws-1").await,
        Err(RoleRefusal::VaultUnavailable)
    );
}
