//! The broker an executor runs over and its paper credential (DEC-2306 items 1 to 3): the run
//! configuration's choice, the connection the account stream's latest connect check passed for,
//! and the OAuth access token the vault's lease on that connection holds, each refusal named and
//! none naming the secret.

use mandate_connections::record::ConnectionId;
use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{
    AccountRef, AccountScope, BrokerOutcome, BrokerRequest, ConnectorError, WorkspaceId,
};
use mandate_executord::Broker;
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::connect::connect_check;
use mandate_executord::credential::{
    BrokerChoice, broker_choice, checked_connection, paper_token, paper_transport, read_token,
};
use mandate_executord::fixture::Ids;
use mandate_executord::paper::FixturePaper;
use mandate_store::MemoryStore;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::lease::Lease;
use secrecy::{ExposeSecret, SecretSlice};

type Error = Box<dyn std::error::Error>;

const ACCOUNT: &str = "01J8Z2ACCT0000000000000001";
const PII: &str = "pii_01J8Z2PQ000000000000000001";
const TOKEN: &str = "executorSentinelToken-0123456789~abcd";

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

/// DEC-2624's token entry for `token`, written out here rather than by its encoder.
fn entry(token: &str) -> Vec<u8> {
    format!(
        r#"{{"access_token":"{token}","format":1,"scope":["data","trading"],"token_type":"bearer"}}"#
    )
    .into_bytes()
}

/// DEC-2306 item 1: no flag and `fixture` are the fixture, `alpaca-paper` is the paper host, and
/// any other value, a live one included, is refused rather than guessed.
#[test]
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

/// DEC-2306 items 2 and 3: the lease's secret decodes as the access token, and the keeper holds
/// the lease to renew. A refused lease or a secret that is not a token entry, a key pair among
/// them, refuses the start, and neither refusal names the secret.
#[test]
fn the_lease_holds_the_token_or_refuses_the_start() -> Result<(), Error> {
    let (token, keeper) = paper_token(Ok(lease(entry(TOKEN))?))?;
    assert_eq!(token.0.expose_secret(), TOKEN);
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
            paper_token(Err(refused)).err(),
            Some("credential_unavailable")
        );
    }
    let key_pair = format!(r#"{{"key_id":"{TOKEN}","secret":"{TOKEN}"}}"#);
    let refused = paper_token(Ok(lease(key_pair.into_bytes())?)).err();
    assert_eq!(refused, Some("credential_malformed"));
    Ok(())
}

/// A rotated secret is read the same way: the entry decodes to its token, and anything else,
/// another token type or a padded entry among them, is `credential_malformed`.
#[test]
fn a_secret_reads_as_its_token_or_is_malformed() -> Result<(), Error> {
    assert_eq!(read_token(&entry(TOKEN))?.0.expose_secret(), TOKEN);
    let mut padded = entry(TOKEN);
    padded.push(b'\n');
    let other_type = String::from_utf8(entry(TOKEN))?.replace("bearer", "Bearer");
    for secret in [padded, other_type.into_bytes(), entry(""), Vec::new()] {
        assert_eq!(read_token(&secret).err(), Some("credential_malformed"));
    }
    Ok(())
}

/// DEC-2306 item 2: the token builds the paper host's transport.
#[test]
fn the_token_builds_the_paper_transport() -> Result<(), Error> {
    paper_transport(read_token(&entry(TOKEN))?)?;
    Ok(())
}
