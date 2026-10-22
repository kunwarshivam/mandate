//! A source that holds no codes refuses a take, so a source written before DEC-2061 fails closed
//! rather than reporting a code as gone.

use mandate_connections::record::{AccountRef, ConnectionId};
use mandate_time::UtcNanos;
use secrecy::SecretSlice;

use crate::SourceError;
use crate::lease::Lease;
use crate::policy::{Identity, SecretPath};
use crate::source::{Grant, SecretSource};

/// A source that implements nothing it is not made to.
struct Codeless(Identity);

impl SecretSource for Codeless {
    fn identity(&self) -> &Identity {
        &self.0
    }

    async fn put(&self, _: &SecretPath, _: &SecretSlice<u8>) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }

    async fn read(&self, _: &SecretPath) -> Result<Option<SecretSlice<u8>>, SourceError> {
        Err(SourceError::Denied)
    }

    async fn delete(&self, _: &SecretPath) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }

    async fn lease(&self, _: &ConnectionId, _: UtcNanos) -> Result<Lease, SourceError> {
        Err(SourceError::Denied)
    }

    async fn renew(&self, _: &Lease, _: UtcNanos) -> Result<Lease, SourceError> {
        Err(SourceError::Denied)
    }

    async fn revoke(&self, _: Lease) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }

    async fn issue_grant(&self, _: &ConnectionId, _: UtcNanos) -> Result<Grant, SourceError> {
        Err(SourceError::Denied)
    }

    async fn revoke_grant(&self, _: &Grant) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_source_that_holds_no_codes_refuses_a_take() {
    let connection = ConnectionId::new("c1").map_err(drop);
    let account = AccountRef::new("01J8Z2ACCT0000000000000001").map_err(drop);
    assert!(connection.is_ok() && account.is_ok());
    if let (Ok(connection), Ok(account)) = (connection, account) {
        let source = Codeless(Identity::TokenExchange(connection.clone()));
        let took = source
            .take_code(&connection)
            .await
            .map(|code| code.is_some());
        assert_eq!(took, Err(SourceError::Denied));
    }
}

/// A source that holds no personal data refuses to create a connection's account entry, by the
/// trait's default (DEC-2339).
#[tokio::test(flavor = "current_thread")]
async fn a_source_that_holds_no_personal_data_refuses_a_create() {
    let connection = ConnectionId::new("c1").map_err(drop);
    let account = AccountRef::new("01J8Z2ACCT0000000000000001").map_err(drop);
    assert!(connection.is_ok() && account.is_ok());
    if let (Ok(connection), Ok(account)) = (connection, account) {
        let source = Codeless(Identity::Executor(connection.clone()));
        let created = source
            .create(
                &SecretPath::AccountId(connection, account),
                &SecretSlice::from(b"acct".to_vec()),
            )
            .await;
        assert_eq!(created, Err(SourceError::Denied));
    }
}
