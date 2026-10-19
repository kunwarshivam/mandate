//! The broker an executor runs over and its paper credential (DEC-2306 items 1 to 3): the run
//! configuration's choice, the connection the account stream's latest connect check passed for,
//! and the credentials the vault's lease on that connection holds, each refusal named and none
//! naming the secret.

use mandate_alpaca::{Credentials, KEY_ID_VAR, SECRET_VAR};
use mandate_canon::{Key, Object, Value, to_canonical};
use mandate_connections::record::ConnectionId;
use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{
    AccountRef, AccountScope, BrokerOutcome, BrokerRequest, ConnectorError, WorkspaceId,
};
use mandate_executord::Broker;
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::connect::connect_check;
use mandate_executord::credential::{
    BrokerChoice, broker_choice, checked_connection, paper_credentials,
};
use mandate_executord::fixture::Ids;
use mandate_executord::paper::FixturePaper;
use mandate_store::MemoryStore;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::lease::Lease;
use secrecy::SecretSlice;

type Error = Box<dyn std::error::Error>;

const ACCOUNT: &str = "01J8Z2ACCT0000000000000001";
const PII: &str = "pii_01J8Z2PQ000000000000000001";
const KEY_ID: &str = "PKEXECUTORSENTINEL01";
const SECRET: &str = "executorSentinelSecretValue0123456789abcd";

fn scope() -> AccountScope {
    AccountScope {
        account: AccountRef(ACCOUNT.to_owned()),
        workspace: WorkspaceId("ws1".to_owned()),
    }
}

fn now() -> Result<UtcNanos, Error> {
    Ok(UtcNanos::parse_rfc3339("2026-10-19T15:00:00Z")?)
}

/// A broker whose account cannot be read, so its connect check fails.
struct Unreadable;

impl Broker for Unreadable {
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        mandate_alpaca::alpaca_profile()
    }
    fn call(&mut self, _request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        Err(ConnectorError::NotSent {
            code: "unreachable",
        })
    }
}

fn checked(store: &mut MemoryStore, connection: &str, passes: bool) -> Result<(), Error> {
    let checked = if passes {
        let mut broker = AlpacaBroker::new(FixturePaper::new(now()?))?;
        connect_check(store, &mut broker, &Ids, &scope(), connection, PII, now()?)?
    } else {
        connect_check(
            store,
            &mut Unreadable,
            &Ids,
            &scope(),
            connection,
            PII,
            now()?,
        )?
    };
    assert_eq!(checked.passed, passes);
    Ok(())
}

fn lease(secret: Vec<u8>) -> Result<Lease, Error> {
    Ok(Lease {
        connection: ConnectionId::new("conn1")?,
        secret: SecretSlice::from(secret),
        expires_at: UtcNanos::parse_rfc3339("2026-10-20T15:00:00Z")?,
        renew_at: UtcNanos::parse_rfc3339("2026-10-19T16:00:00Z")?,
    })
}

fn vault_secret(key_id: &str, secret: &str) -> Vec<u8> {
    let mut object = Object::new();
    for (name, value) in [("key_id", key_id), ("secret", secret)] {
        object.insert(
            Key::new(name).unwrap_or_else(|_| panic!("{name}")),
            Value::Str(value.to_owned()),
        );
    }
    to_canonical(&Value::Object(object))
}

/// DEC-2306 item 1: no flag and `fixture` are the fixture, `alpaca-paper` is the paper host, and
/// any other value, a live one included, is refused rather than guessed.
#[test]
#[ignore = "pending E21-30"]
fn the_run_configuration_names_the_broker() {
    assert_eq!(broker_choice(None), Ok(BrokerChoice::Fixture));
    assert_eq!(broker_choice(Some("fixture")), Ok(BrokerChoice::Fixture));
    assert_eq!(
        broker_choice(Some("alpaca-paper")),
        Ok(BrokerChoice::AlpacaPaper)
    );
    for other in ["", "alpaca", "alpaca-live", "ALPACA-PAPER", " alpaca-paper"] {
        assert_eq!(
            broker_choice(Some(other)),
            Err("broker_unknown"),
            "{other:?}"
        );
    }
}

/// DEC-2306 item 3: the connection is the one the latest connect check passed for. None, or a
/// latest check that failed, leaves the executor with no connection to lease.
#[test]
#[ignore = "pending E21-30"]
fn the_connection_is_the_one_the_latest_check_passed_for() -> Result<(), Error> {
    let mut store = MemoryStore::default();
    assert_eq!(
        checked_connection(&store, &scope()),
        Err("connection_unchecked")
    );
    checked(&mut store, "conn1", true)?;
    assert_eq!(
        checked_connection(&store, &scope()),
        Ok(ConnectionId::new("conn1")?)
    );
    checked(&mut store, "conn2", true)?;
    assert_eq!(
        checked_connection(&store, &scope()),
        Ok(ConnectionId::new("conn2")?)
    );
    checked(&mut store, "conn2", false)?;
    assert_eq!(
        checked_connection(&store, &scope()),
        Err("connection_unchecked")
    );
    Ok(())
}

/// DEC-2306 items 2 and 3: the lease's secret reads as the paper credentials, and the keeper
/// holds the lease to renew. A refused lease or a secret that does not read refuses the start,
/// and neither refusal names the secret.
#[test]
#[ignore = "pending E21-30"]
fn the_lease_holds_the_credentials_or_refuses_the_start() -> Result<(), Error> {
    let expected = Credentials::from_lookup(|name| match name {
        KEY_ID_VAR => Some(KEY_ID.to_owned()),
        SECRET_VAR => Some(SECRET.to_owned()),
        _ => None,
    })?;
    let (credentials, keeper) = paper_credentials(Ok(lease(vault_secret(KEY_ID, SECRET))?))?;
    assert_eq!(credentials, expected);
    assert_eq!(
        keeper.lease().map(|held| held.connection.clone()),
        Some(ConnectionId::new("conn1")?)
    );
    for refused in [
        SourceError::Missing,
        SourceError::Denied,
        SourceError::Unavailable,
    ] {
        assert_eq!(
            paper_credentials(Err(refused)).err(),
            Some("credential_unavailable")
        );
    }
    let malformed = format!(r#"{{"key_id":"{KEY_ID}","secret":"{SECRET}","x":"{SECRET}"}}"#);
    let refused = paper_credentials(Ok(lease(malformed.into_bytes())?)).err();
    assert_eq!(refused, Some("credential_malformed"));
    Ok(())
}
