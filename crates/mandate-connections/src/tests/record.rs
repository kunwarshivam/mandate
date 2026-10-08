//! The connection record holds references only, and one account has one connection (connections
//! spec §3, §3.1, §8.1 check 4; CN-1, CN-5, CN-12; E7-11).

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use crate::ConnectError;
use crate::record::{
    AccountFingerprint, AccountPiiRef, AccountRef, Admission, AuthKind, Broker, Candidate,
    ConnectionId, ConnectionRecord, ConnectionState, ConnectionView, Environment,
    EstablishedMembers, NewRecord, Registry,
};

/// Bytes no output may show: a fingerprint is never printed, journaled, or returned.
const CANARY: [u8; 32] = [0xC7; 32];

fn id(text: &str) -> ConnectionId {
    ConnectionId(text.to_owned())
}

fn account_ref(n: u8) -> AccountRef {
    AccountRef(format!("01J8Z2ACCT00000000000000{n:02}"))
}

fn fingerprint(n: u8) -> AccountFingerprint {
    AccountFingerprint::from_vault_hash([n; 32])
}

fn scopes() -> BTreeSet<String> {
    BTreeSet::from(["data".to_owned(), "trading".to_owned()])
}

fn alpaca(connection: &str, n: u8) -> NewRecord {
    NewRecord {
        connection_id: id(connection),
        broker: Broker::Alpaca,
        environment: Environment::Paper,
        auth_kind: AuthKind::ApiKey,
        scopes: scopes(),
        account_ref: account_ref(n),
        account_pii_ref: AccountPiiRef(format!("pii_{n}")),
        fingerprint: fingerprint(n),
        contract_hash: None,
        terms_version: None,
    }
}

fn record(new: NewRecord) -> ConnectionRecord {
    ConnectionRecord::new(new).unwrap()
}

fn candidate(n: u8) -> Candidate {
    Candidate {
        broker: Broker::Alpaca,
        environment: Environment::Paper,
        fingerprint: fingerprint(n),
    }
}

#[test]
#[ignore = "pending E7-11"]
fn identifiers_are_validated() {
    for good in ["conn_alpaca_paper", "a", "A-9_z", &"x".repeat(64)] {
        assert_eq!(ConnectionId::new(good), Ok(id(good)), "{good:?}");
        assert_eq!(
            AccountPiiRef::new(good),
            Ok(AccountPiiRef(good.to_owned())),
            "{good:?}"
        );
    }
    for bad in [
        "",
        "conn:a",
        "conn a",
        "conn/a",
        "cönn",
        &"x".repeat(65),
        "conn\n",
    ] {
        assert_eq!(
            ConnectionId::new(bad),
            Err(ConnectError::InvalidConnectionId),
            "{bad:?}"
        );
        assert_eq!(
            AccountPiiRef::new(bad),
            Err(ConnectError::InvalidPiiRef),
            "{bad:?}"
        );
    }
    for good in ["01J8Z2ACCT00000000000000A1", "7ZZZZZZZZZZZZZZZZZZZZZZZZZ"] {
        assert_eq!(
            AccountRef::new(good),
            Ok(AccountRef(good.to_owned())),
            "{good:?}"
        );
    }
    for bad in [
        "",
        "01J8Z2ACCT00000000000000A",
        "01J8Z2ACCT00000000000000A12",
        "01j8z2acct00000000000000a1",
        "01J8Z2ACCT00000000000000AI",
        "01J8Z2ACCT00000000000000AL",
        "01J8Z2ACCT00000000000000AO",
        "01J8Z2ACCT00000000000000AU",
        "8ZZZZZZZZZZZZZZZZZZZZZZZZZ",
        " 1J8Z2ACCT00000000000000A1",
    ] {
        assert_eq!(
            AccountRef::new(bad),
            Err(ConnectError::InvalidAccountRef),
            "{bad:?}"
        );
    }
}

#[test]
fn a_fingerprint_prints_no_byte() {
    let shown = format!("{:?}", AccountFingerprint::from_vault_hash(CANARY));
    assert_eq!(shown, "AccountFingerprint(redacted)");
}

#[test]
#[ignore = "pending E7-11"]
fn a_record_shows_references_only() {
    let mut new = alpaca("conn_a", 1);
    new.fingerprint = AccountFingerprint::from_vault_hash(CANARY);
    let made = record(new);
    assert_eq!(
        made.view(),
        Ok(ConnectionView {
            connection_id: id("conn_a"),
            broker: Broker::Alpaca,
            environment: Environment::Paper,
            auth_kind: AuthKind::ApiKey,
            scopes: scopes(),
            state: ConnectionState::Active,
        })
    );
    assert_eq!(
        made.established_members(),
        Ok(EstablishedMembers {
            connection_id: id("conn_a"),
            broker: Broker::Alpaca,
            environment: Environment::Paper,
            scopes: scopes(),
            account_ref: account_ref(1),
        })
    );
    let printed = format!("{made:?}");
    for byte_form in ["199", "c7", "C7"] {
        assert!(
            !printed.contains(byte_form),
            "a fingerprint byte in {printed}"
        );
    }
    assert!(
        printed.contains("AccountFingerprint(redacted)"),
        "{printed}"
    );
}

#[test]
#[ignore = "pending E7-11"]
fn a_record_keeps_the_connections_rules() {
    let mut live_key = alpaca("conn_a", 1);
    live_key.environment = Environment::Live;
    assert_eq!(
        ConnectionRecord::new(live_key),
        Err(ConnectError::EnvironmentRefused),
        "an Alpaca API key is paper only (DEC-441 item 3)"
    );
    let mut no_scopes = alpaca("conn_a", 1);
    no_scopes.scopes = BTreeSet::new();
    assert_eq!(
        ConnectionRecord::new(no_scopes),
        Err(ConnectError::InvalidRecord { member: "scopes" })
    );
    let mut robinhood_key = alpaca("conn_a", 1);
    robinhood_key.broker = Broker::Robinhood;
    robinhood_key.environment = Environment::Live;
    assert_eq!(
        ConnectionRecord::new(robinhood_key),
        Err(ConnectError::InvalidRecord {
            member: "auth_kind"
        }),
        "Robinhood connects over MCP only"
    );
    let mut stray_hash = alpaca("conn_a", 1);
    stray_hash.contract_hash = Some("sha256:".to_owned() + &"a".repeat(64));
    assert_eq!(
        ConnectionRecord::new(stray_hash),
        Err(ConnectError::InvalidRecord {
            member: "contract_hash"
        })
    );
    let mut mcp = alpaca("conn_r", 2);
    mcp.broker = Broker::Robinhood;
    mcp.environment = Environment::Live;
    mcp.auth_kind = AuthKind::McpOauth;
    assert_eq!(
        ConnectionRecord::new(mcp.clone()),
        Err(ConnectError::InvalidRecord {
            member: "contract_hash"
        }),
        "an MCP connection pins its contract (connections spec §6.2 rule 3)"
    );
    mcp.contract_hash = Some("sha256:".to_owned() + &"a".repeat(64));
    assert!(ConnectionRecord::new(mcp).is_ok());
    let mut oauth = alpaca("conn_o", 3);
    oauth.auth_kind = AuthKind::Oauth;
    assert!(ConnectionRecord::new(oauth).is_ok());
}

#[test]
#[ignore = "pending E7-11"]
fn a_second_connection_to_a_connected_account_is_refused() {
    for state in [
        ConnectionState::Active,
        ConnectionState::Degraded,
        ConnectionState::Suspended,
    ] {
        let mut registry = Registry::new(Vec::new(), false);
        registry.insert(record(alpaca("conn_a", 1))).unwrap();
        registry.set_state(&id("conn_a"), state).unwrap();
        assert_eq!(
            registry.admit(&candidate(1)),
            Err(ConnectError::AlreadyConnected {
                existing: id("conn_a")
            }),
            "{state:?}"
        );
        assert_eq!(
            registry.insert(record(alpaca("conn_b", 1))),
            Err(ConnectError::AlreadyConnected {
                existing: id("conn_a")
            }),
            "insert re-checks the account ({state:?})"
        );
        assert_eq!(registry.admit(&candidate(2)), Ok(Admission::New));
    }
}

#[test]
#[ignore = "pending E7-11"]
fn a_revoked_account_reconnects_on_its_own_connection() {
    let mut registry = Registry::new(Vec::new(), false);
    registry.insert(record(alpaca("conn_a", 1))).unwrap();
    registry
        .set_state(&id("conn_a"), ConnectionState::Revoked)
        .unwrap();
    assert_eq!(
        registry.insert(record(alpaca("conn_b", 1))),
        Err(ConnectError::AlreadyConnected {
            existing: id("conn_a")
        }),
        "a revoked account comes back only on its own connection (CN-12)"
    );
    let mut new_stream = alpaca("conn_a", 1);
    new_stream.account_ref = account_ref(9);
    assert_eq!(
        registry.insert(record(new_stream)),
        Err(ConnectError::AlreadyConnected {
            existing: id("conn_a")
        }),
        "and on its own account stream"
    );
    assert_eq!(
        registry.admit(&candidate(1)),
        Ok(Admission::Reconnect {
            connection_id: id("conn_a"),
            account_ref: account_ref(1),
        })
    );
    registry.insert(record(alpaca("conn_a", 1))).unwrap();
    assert_eq!(
        registry.view(&id("conn_a")).unwrap().map(|v| v.state),
        Some(ConnectionState::Active)
    );
    assert_eq!(
        registry.insert(record(alpaca("conn_b", 1))),
        Err(ConnectError::AlreadyConnected {
            existing: id("conn_a")
        }),
        "a new id for a reconnected account would reset its loss carry (CN-12)"
    );
    let mut other_environment = candidate(1);
    other_environment.environment = Environment::Live;
    registry
        .set_state(&id("conn_a"), ConnectionState::Revoked)
        .unwrap();
    assert_eq!(
        registry.admit(&other_environment),
        Err(ConnectError::ReconnectMismatch)
    );
}

#[test]
#[ignore = "pending E7-11"]
fn ids_are_unique_and_a_view_needs_a_record() {
    let mut registry = Registry::new(Vec::new(), false);
    registry.insert(record(alpaca("conn_a", 1))).unwrap();
    assert_eq!(
        registry.insert(record(alpaca("conn_a", 2))),
        Err(ConnectError::InvalidConnectionId)
    );
    assert_eq!(registry.view(&id("conn_x")), Ok(None));
    assert_eq!(
        registry.set_state(&id("conn_x"), ConnectionState::Revoked),
        Err(ConnectError::InvalidConnectionId)
    );
}

#[test]
#[ignore = "pending E7-11"]
fn every_connect_waits_while_the_fingerprint_key_rotates() {
    let revoked = {
        let mut registry = Registry::new(Vec::new(), false);
        registry.insert(record(alpaca("conn_a", 1))).unwrap();
        registry
            .set_state(&id("conn_a"), ConnectionState::Revoked)
            .unwrap();
        registry
    };
    let rotating = Registry::new(revoked.records.clone(), true);
    for n in [1, 2] {
        assert_eq!(
            rotating.admit(&candidate(n)),
            Err(ConnectError::FingerprintRotating),
            "account {n}"
        );
    }
}

/// One step of a deployment's life.
#[derive(Debug, Clone)]
enum Step {
    Connect(u8),
    Revoke(u8),
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (0u8..4).prop_map(Step::Connect),
        (0u8..4).prop_map(Step::Revoke),
    ]
}

/// CN-5 and CN-12, against an oracle that keeps its own map of which connection each account was
/// first bound to and which accounts are live: an account is never connected twice, and once
/// bound it keeps its connection id and `account_ref` through every revoke and reconnect.
#[test]
#[ignore = "pending E7-11"]
fn one_account_one_connection_for_life() {
    let mut runner = TestRunner::deterministic();
    let lives = prop::collection::vec(step(), 1..40);
    runner
        .run(&lives, |steps| {
            let mut registry = Registry::new(Vec::new(), false);
            let mut bound: BTreeMap<u8, (ConnectionId, AccountRef)> = BTreeMap::new();
            let mut live: BTreeSet<u8> = BTreeSet::new();
            let mut minted = 0u8;
            for step in steps {
                match step {
                    Step::Connect(n) => {
                        let got = registry.admit(&candidate(n));
                        let want = match (live.contains(&n), bound.get(&n)) {
                            (true, Some((existing, _))) => Err(ConnectError::AlreadyConnected {
                                existing: existing.clone(),
                            }),
                            (false, Some((connection_id, account_ref))) => {
                                Ok(Admission::Reconnect {
                                    connection_id: connection_id.clone(),
                                    account_ref: account_ref.clone(),
                                })
                            }
                            _ => Ok(Admission::New),
                        };
                        prop_assert_eq!(&got, &want);
                        let (connection_id, reference) = match got {
                            Ok(Admission::New) => {
                                minted += 1;
                                (id(&format!("conn_{minted}")), account_ref(minted))
                            }
                            Ok(Admission::Reconnect {
                                connection_id,
                                account_ref,
                            }) => (connection_id, account_ref),
                            Err(_) => continue,
                        };
                        let mut new = alpaca(connection_id.0.as_str(), n);
                        new.account_ref = reference.clone();
                        prop_assert_eq!(registry.insert(record(new)), Ok(()));
                        bound.entry(n).or_insert((connection_id, reference));
                        live.insert(n);
                    }
                    Step::Revoke(n) => {
                        if let (true, Some((connection_id, _))) = (live.contains(&n), bound.get(&n))
                        {
                            prop_assert_eq!(
                                registry.set_state(connection_id, ConnectionState::Revoked),
                                Ok(())
                            );
                            live.remove(&n);
                        }
                    }
                }
            }
            Ok(())
        })
        .unwrap();
}
