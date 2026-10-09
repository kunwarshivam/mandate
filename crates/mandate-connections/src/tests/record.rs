//! The connection record holds references only, and one account has one connection (connections
//! spec §3, §3.1, §8.1 check 4, §9.1; CN-1, CN-5, CN-12; E7-11).

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use crate::ConnectError;
use crate::record::{
    AccountFingerprint, AccountPiiRef, AccountRef, Admission, AuthKind, Broker, Candidate,
    ConnectionId, ConnectionRecord, ConnectionState, ConnectionView, Environment,
    EstablishedMembers, NewRecord, Registry,
};

use ConnectionState::{Active, Degraded, Revoked, Suspended};

/// Bytes no output may show: a fingerprint is never printed, journaled, or returned.
const CANARY: [u8; 32] = [0xC7; 32];
const STATES: [ConnectionState; 4] = [Active, Degraded, Suspended, Revoked];
/// §9.1's moves, written out as pairs rather than derived from any rule the code could share.
const MOVES: [(ConnectionState, ConnectionState); 8] = [
    (Active, Degraded),
    (Active, Suspended),
    (Active, Revoked),
    (Degraded, Active),
    (Degraded, Suspended),
    (Degraded, Revoked),
    (Suspended, Active),
    (Suspended, Revoked),
];
const SHA: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
/// Exactly `sha256:` and 64 lowercase hex digits, every digit among them.
const EVERY_HEX_DIGIT: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
/// Digests a prefix-only or length-only check would take: each breaks `sha256:` plus exactly 64
/// lowercase hex digits in one way.
const MALFORMED_DIGESTS: [&str; 8] = [
    "sha512:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "SHA256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaag",
    "sha256:",
    "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
];

fn id(text: &str) -> ConnectionId {
    ConnectionId(text.to_owned())
}

fn account_ref(n: u8) -> AccountRef {
    AccountRef(format!("01J8Z2ACCT00000000000000{n:02}"))
}

fn fingerprint(n: u8) -> AccountFingerprint {
    AccountFingerprint::from_vault_hash([n; 32])
}

/// Account `n`'s fingerprint with one byte changed: another account.
fn near(n: u8, byte: usize) -> AccountFingerprint {
    let mut hash = [n; 32];
    hash[byte] ^= 0x01;
    AccountFingerprint::from_vault_hash(hash)
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

fn robinhood(connection: &str, n: u8) -> NewRecord {
    NewRecord {
        broker: Broker::Robinhood,
        environment: Environment::Live,
        auth_kind: AuthKind::McpOauth,
        contract_hash: Some(SHA.to_owned()),
        ..alpaca(connection, n)
    }
}

/// One change to a record about to be made.
type Change<'a> = &'a dyn Fn(&mut NewRecord);

fn record(new: NewRecord) -> ConnectionRecord {
    ConnectionRecord::new(new).unwrap()
}

fn empty() -> Registry {
    Registry::new(Vec::new(), false).unwrap()
}

fn candidate(n: u8) -> Candidate {
    Candidate {
        broker: Broker::Alpaca,
        environment: Environment::Paper,
        fingerprint: fingerprint(n),
    }
}

/// A record made as the registry stores it, without [`ConnectionRecord::new`]'s checks.
fn stored(new: NewRecord, state: ConnectionState) -> ConnectionRecord {
    ConnectionRecord {
        connection_id: new.connection_id,
        broker: new.broker,
        environment: new.environment,
        auth_kind: new.auth_kind,
        scopes: new.scopes,
        account_ref: new.account_ref,
        account_pii_ref: new.account_pii_ref,
        fingerprint: new.fingerprint,
        state,
        contract_hash: new.contract_hash,
        terms_version: new.terms_version,
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
        "01J8Z2ACCT00000000000000A\n",
    ] {
        assert_eq!(
            AccountRef::new(bad),
            Err(ConnectError::InvalidAccountRef),
            "{bad:?}"
        );
    }
}

#[test]
fn each_broker_has_its_journal_name() {
    for (broker, code) in [
        (Broker::Alpaca, "alpaca"),
        (Broker::Robinhood, "robinhood"),
        (Broker::KrakenDerivativesUs, "kraken_derivatives_us"),
    ] {
        assert_eq!(broker.code(), code);
    }
}

#[test]
#[ignore = "pending E7-11"]
fn a_broker_is_read_back_from_its_name_only() {
    for broker in [
        Broker::Alpaca,
        Broker::Robinhood,
        Broker::KrakenDerivativesUs,
    ] {
        assert_eq!(Broker::from_code(broker.code()), Ok(broker));
    }
    for bad in [
        "",
        "Alpaca",
        "alpaca ",
        "kraken",
        "robinhood_sim",
        "kraken-derivatives-us",
    ] {
        assert_eq!(
            Broker::from_code(bad),
            Err(ConnectError::InvalidRecord { member: "broker" }),
            "{bad:?}"
        );
    }
}

#[test]
fn names_read_back_as_the_journal_writes_them() {
    assert_eq!(id("conn_a").as_str(), "conn_a");
    assert_eq!(account_ref(7).as_str(), "01J8Z2ACCT0000000000000007");
    assert_eq!(AccountPiiRef("pii_7".to_owned()).as_str(), "pii_7");
    assert_eq!(Environment::Paper.as_str(), "paper");
    assert_eq!(Environment::Live.as_str(), "live");
    for (kind, code) in [
        (AuthKind::ApiKey, "api_key"),
        (AuthKind::Oauth, "oauth"),
        (AuthKind::McpOauth, "mcp_oauth"),
    ] {
        assert_eq!(kind.code(), code);
    }
}

#[test]
fn a_pii_ref_prints_nothing_of_itself() {
    let shown = format!("{:?}", AccountPiiRef("pii_acct_7Q2M".to_owned()));
    assert_eq!(shown, "AccountPiiRef(redacted)");
    let new_record = format!("{:?}", alpaca("conn_a", 3));
    assert!(!new_record.contains("pii_3"), "{new_record}");
}

#[test]
fn a_fingerprint_prints_no_byte() {
    let shown = format!("{:?}", AccountFingerprint::from_vault_hash(CANARY));
    assert_eq!(shown, "AccountFingerprint(redacted)");
}

/// The two infallible accessors, on a record built as stored: every member they return is the
/// record's own, and neither carries the fingerprint or the personal-data reference.
#[test]
fn a_record_is_viewed_as_built() {
    let mut new = robinhood("conn_r", 4);
    new.fingerprint = AccountFingerprint::from_vault_hash(CANARY);
    new.scopes = BTreeSet::from(["get_accounts".to_owned()]);
    let made = stored(new, Suspended);
    assert_eq!(
        made.view(),
        ConnectionView {
            connection_id: id("conn_r"),
            broker: Broker::Robinhood,
            environment: Environment::Live,
            auth_kind: AuthKind::McpOauth,
            scopes: BTreeSet::from(["get_accounts".to_owned()]),
            state: Suspended,
        }
    );
    assert_eq!(
        made.established_members(),
        EstablishedMembers {
            connection_id: id("conn_r"),
            broker: Broker::Robinhood,
            environment: Environment::Live,
            scopes: BTreeSet::from(["get_accounts".to_owned()]),
            account_ref: account_ref(4),
        }
    );
    let printed = format!("{made:?}");
    assert!(
        !printed.contains("pii_4") && !printed.contains("199"),
        "{printed}"
    );
}

/// Live: what a record shows is fixed by its types, not by [`ConnectionRecord::new`]'s checks, so
/// it is judged on a record built as stored (a pass-through `new` would pass it pending).
#[test]
fn a_record_shows_references_only() {
    let mut new = alpaca("conn_a", 1);
    new.fingerprint = AccountFingerprint::from_vault_hash(CANARY);
    let made = stored(new, Active);
    assert_eq!(made.view().state, Active);
    assert_eq!(made.established_members().account_ref, account_ref(1));
    let printed = format!("{made:?}");
    for byte_form in ["199", "c7", "C7", "pii_1"] {
        assert!(
            !printed.contains(byte_form),
            "a fingerprint byte in {printed}"
        );
    }
    assert!(printed.contains("AccountPiiRef(redacted)"), "{printed}");
    assert!(
        printed.contains("AccountFingerprint(redacted)"),
        "{printed}"
    );
}

#[test]
#[ignore = "pending E7-11"]
fn a_record_keeps_the_connections_rules() {
    let invalid = |member| Err(ConnectError::InvalidRecord { member });
    let with = |change: &dyn Fn(&mut NewRecord), base: NewRecord| {
        let mut new = base;
        change(&mut new);
        ConnectionRecord::new(new).map(|_| ())
    };
    let alpaca_live = with(&|n| n.environment = Environment::Live, alpaca("conn_a", 1));
    assert_eq!(
        alpaca_live,
        Err(ConnectError::EnvironmentRefused),
        "DEC-441 item 3"
    );
    let alpaca_live_oauth = with(
        &|n| {
            n.environment = Environment::Live;
            n.auth_kind = AuthKind::Oauth;
        },
        alpaca("conn_o", 1),
    );
    assert_eq!(
        alpaca_live_oauth,
        Err(ConnectError::EnvironmentRefused),
        "DEC-821 item 7: paper connections only"
    );
    let cases: [(&str, Change, NewRecord, &str); 15] = [
        ("no scopes", &|n| n.scopes.clear(), alpaca("a", 1), "scopes"),
        (
            "an empty scope",
            &|n| {
                n.scopes.insert(String::new());
            },
            alpaca("a", 1),
            "scopes",
        ),
        (
            "a spaced scope",
            &|n| {
                n.scopes.insert("read write".into());
            },
            alpaca("a", 1),
            "scopes",
        ),
        (
            "a control scope",
            &|n| {
                n.scopes.insert("read\n".into());
            },
            alpaca("a", 1),
            "scopes",
        ),
        (
            "a non-ASCII scope",
            &|n| {
                n.scopes.insert("tradé".into());
            },
            alpaca("a", 1),
            "scopes",
        ),
        (
            "a DEL scope",
            &|n| {
                n.scopes.insert("read\u{7f}".into());
            },
            alpaca("a", 1),
            "scopes",
        ),
        (
            "Robinhood by API key",
            &|n| n.auth_kind = AuthKind::ApiKey,
            robinhood("r", 2),
            "auth_kind",
        ),
        (
            "Robinhood by plain OAuth",
            &|n| n.auth_kind = AuthKind::Oauth,
            robinhood("r", 2),
            "auth_kind",
        ),
        (
            "Kraken by OAuth",
            &|n| {
                n.broker = Broker::KrakenDerivativesUs;
                n.auth_kind = AuthKind::Oauth;
            },
            alpaca("k", 3),
            "auth_kind",
        ),
        (
            "Alpaca over MCP",
            &|n| n.auth_kind = AuthKind::McpOauth,
            alpaca("a", 1),
            "auth_kind",
        ),
        (
            "a stray contract hash",
            &|n| n.contract_hash = Some(SHA.into()),
            alpaca("a", 1),
            "contract_hash",
        ),
        (
            "MCP without its contract",
            &|n| n.contract_hash = None,
            robinhood("r", 2),
            "contract_hash",
        ),
        (
            "an uppercase contract hash",
            &|n| n.contract_hash = Some(SHA.to_uppercase()),
            robinhood("r", 2),
            "contract_hash",
        ),
        (
            "terms on Alpaca",
            &|n| n.terms_version = Some(SHA.into()),
            alpaca("a", 1),
            "terms_version",
        ),
        (
            "terms not a hash",
            &|n| n.terms_version = Some("v3".into()),
            robinhood("r", 2),
            "terms_version",
        ),
    ];
    for (name, change, base, member) in cases {
        assert_eq!(with(change, base), invalid(member), "{name}");
    }
    for digest in MALFORMED_DIGESTS {
        let mut hashed = robinhood("r", 2);
        hashed.contract_hash = Some(digest.to_owned());
        assert_eq!(
            ConnectionRecord::new(hashed).map(|_| ()),
            invalid("contract_hash"),
            "contract_hash {digest:?}"
        );
        let mut termed = robinhood("r", 2);
        termed.terms_version = Some(digest.to_owned());
        assert_eq!(
            ConnectionRecord::new(termed).map(|_| ()),
            invalid("terms_version"),
            "terms_version {digest:?}"
        );
    }
    let accepted: [(Change, NewRecord); 6] = [
        (&|_| {}, robinhood("r", 2)),
        (&|n| n.terms_version = Some(SHA.into()), robinhood("r", 2)),
        (
            &|n| n.contract_hash = Some(EVERY_HEX_DIGIT.into()),
            robinhood("r", 2),
        ),
        (
            &|n| n.terms_version = Some(EVERY_HEX_DIGIT.into()),
            robinhood("r", 2),
        ),
        (&|n| n.auth_kind = AuthKind::Oauth, alpaca("o", 3)),
        (
            &|n| {
                n.broker = Broker::KrakenDerivativesUs;
                n.environment = Environment::Live;
            },
            alpaca("k", 4),
        ),
    ];
    for (change, base) in accepted {
        assert_eq!(with(change, base), Ok(()));
    }
}

#[test]
#[ignore = "pending E7-11"]
fn a_registry_starts_from_records_that_keep_cn_5() {
    let active = || stored(alpaca("conn_a", 1), Active);
    let both_orders = |revoked: NewRecord| {
        let revoked = stored(revoked, Revoked);
        [
            (
                id("conn_a"),
                Registry::new(vec![active(), revoked.clone()], false).err(),
            ),
            (
                revoked.connection_id.clone(),
                Registry::new(vec![revoked, active()], false).err(),
            ),
        ]
    };
    for (_, got) in both_orders(alpaca("conn_b", 2)) {
        assert_eq!(got, None);
    }
    for (_, got) in both_orders(alpaca("conn_a", 2)) {
        assert_eq!(got, Some(ConnectError::InvalidConnectionId));
    }
    let mut same_account = alpaca("conn_b", 2);
    same_account.fingerprint = fingerprint(1);
    for (first, got) in both_orders(same_account) {
        assert_eq!(
            got,
            Some(ConnectError::AlreadyConnected { existing: first }),
            "the first record of the two is named"
        );
    }
    let mut same_stream = alpaca("conn_b", 2);
    same_stream.account_ref = account_ref(1);
    for (_, got) in both_orders(same_stream) {
        assert_eq!(got, Some(ConnectError::InvalidAccountRef));
    }
}

/// Another account, under a new id, cannot take an account stream a record holds, live or
/// revoked (CN-5).
#[test]
#[ignore = "pending E7-11"]
fn an_account_ref_is_held_by_one_record() {
    for state in [Active, Revoked] {
        let mut registry = empty();
        registry.insert(record(alpaca("conn_a", 1))).unwrap();
        if state == Revoked {
            registry.set_state(&id("conn_a"), Revoked).unwrap();
        }
        let mut stream_taken = alpaca("conn_b", 2);
        stream_taken.account_ref = account_ref(1);
        assert_eq!(
            registry.insert(record(stream_taken)),
            Err(ConnectError::InvalidAccountRef),
            "{state:?}"
        );
        assert_eq!(registry.view(&id("conn_b")), Ok(None), "{state:?}");
    }
}

#[test]
#[ignore = "pending E7-11"]
fn a_fingerprint_matches_on_every_byte() {
    let mut registry = empty();
    registry.insert(record(alpaca("conn_a", 1))).unwrap();
    for byte in 0..32 {
        let other = Candidate {
            fingerprint: near(1, byte),
            ..candidate(1)
        };
        assert_eq!(registry.admit(&other), Ok(Admission::New), "byte {byte}");
        let mut new = alpaca(&format!("conn_n{byte}"), 2);
        new.fingerprint = near(1, byte);
        new.account_ref = AccountRef(format!("01J8Z2ACCT0000000000000N{byte:02}"));
        assert_eq!(empty_with_a(&new), Ok(()), "byte {byte}");
    }
    assert_eq!(
        registry.admit(&candidate(1)),
        Err(ConnectError::AlreadyConnected {
            existing: id("conn_a")
        })
    );
}

/// Inserts `new` into a registry holding account 1's connection.
fn empty_with_a(new: &NewRecord) -> Result<(), ConnectError> {
    let mut registry = empty();
    registry.insert(record(alpaca("conn_a", 1)))?;
    registry.insert(record(new.clone()))
}

#[test]
#[ignore = "pending E7-11"]
fn a_second_connection_to_a_connected_account_is_refused() {
    for state in [Active, Degraded, Suspended] {
        let mut registry = empty();
        registry.insert(record(alpaca("conn_a", 1))).unwrap();
        if state != Active {
            registry.set_state(&id("conn_a"), state).unwrap();
        }
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

/// Every pair of states, against §9.1's moves written out above: each listed move is taken, every
/// other one, a move to the same state among them, is refused and leaves the state as it was.
#[test]
#[ignore = "pending E7-11"]
fn states_move_only_as_section_9_allows() {
    let reach = |state| -> Registry {
        let mut registry = empty();
        registry.insert(record(alpaca("conn_a", 1))).unwrap();
        let path: &[ConnectionState] = match state {
            Active => &[],
            Degraded => &[Degraded],
            Suspended => &[Suspended],
            Revoked => &[Revoked],
        };
        for step in path {
            registry.set_state(&id("conn_a"), *step).unwrap();
        }
        registry
    };
    for from in STATES {
        for to in STATES {
            let mut registry = reach(from);
            let got = registry.set_state(&id("conn_a"), to);
            let state = registry.view(&id("conn_a")).unwrap().map(|v| v.state);
            if MOVES.contains(&(from, to)) {
                assert_eq!((got, state), (Ok(()), Some(to)), "{from:?} to {to:?}");
            } else {
                let refused = Err(ConnectError::InvalidTransition { from, to });
                assert_eq!((got, state), (refused, Some(from)), "{from:?} to {to:?}");
            }
        }
    }
    assert_eq!(
        empty().set_state(&id("conn_x"), Revoked),
        Err(ConnectError::UnknownConnection)
    );
}

#[test]
#[ignore = "pending E7-11"]
fn a_revoked_account_reconnects_on_its_own_connection() {
    let mut registry = empty();
    registry.insert(record(alpaca("conn_a", 1))).unwrap();
    registry.set_state(&id("conn_a"), Revoked).unwrap();
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
    let mut renewed = alpaca("conn_a", 1);
    renewed.auth_kind = AuthKind::Oauth;
    renewed.scopes = BTreeSet::from(["trading".to_owned()]);
    registry.insert(record(renewed)).unwrap();
    assert_eq!(
        registry.view(&id("conn_a")),
        Ok(Some(ConnectionView {
            connection_id: id("conn_a"),
            broker: Broker::Alpaca,
            environment: Environment::Paper,
            auth_kind: AuthKind::Oauth,
            scopes: BTreeSet::from(["trading".to_owned()]),
            state: Active,
        })),
        "the reconnect's record replaces the revoked one"
    );
    assert_eq!(
        registry.established(&id("conn_a")),
        Ok(Some(EstablishedMembers {
            connection_id: id("conn_a"),
            broker: Broker::Alpaca,
            environment: Environment::Paper,
            scopes: BTreeSet::from(["trading".to_owned()]),
            account_ref: account_ref(1),
        }))
    );
    assert_eq!(registry.records.len(), 1, "replaced, not added");
    assert_eq!(
        registry.insert(record(alpaca("conn_b", 1))),
        Err(ConnectError::AlreadyConnected {
            existing: id("conn_a")
        }),
        "a new id for a reconnected account would reset its loss carry (CN-12)"
    );
}

/// A reconnect continues its account stream and the state that stream last recorded (journal spec
/// §9.8): `suspended` stays `suspended` (connections spec §9.1), and `degraded` stays `degraded`,
/// since a reconnect is only the connection's condition and the owner must still acknowledge.
/// Each line is (the state the connection was revoked from, the state after the reconnect).
#[test]
#[ignore = "pending E7-11"]
fn a_reconnect_keeps_the_state_its_connection_was_revoked_from() {
    for (revoked_from, reconnected) in [
        (Active, Active),
        (Degraded, Degraded),
        (Suspended, Suspended),
    ] {
        let mut registry = empty();
        registry.insert(record(alpaca("conn_a", 1))).unwrap();
        if revoked_from != Active {
            registry.set_state(&id("conn_a"), revoked_from).unwrap();
        }
        registry.set_state(&id("conn_a"), Revoked).unwrap();
        assert_eq!(
            registry.view(&id("conn_a")).unwrap().map(|v| v.state),
            Some(Revoked),
            "revoked while revoked ({revoked_from:?})"
        );
        registry.insert(record(alpaca("conn_a", 1))).unwrap();
        assert_eq!(
            registry.view(&id("conn_a")).unwrap().map(|v| v.state),
            Some(reconnected),
            "revoked from {revoked_from:?}"
        );
        assert_eq!(registry.records.len(), 1, "replaced, not added");
    }
    let mut loaded = Registry::new(vec![stored(alpaca("conn_a", 1), Revoked)], false).unwrap();
    loaded.insert(record(alpaca("conn_a", 1))).unwrap();
    assert_eq!(
        loaded.view(&id("conn_a")).unwrap().map(|v| v.state),
        Some(Suspended),
        "a revoked record the registry was not told the earlier state of reconnects suspended, \
         the state that needs the most to leave (AGENTS.md rule 3)"
    );
}

#[test]
#[ignore = "pending E7-11"]
fn a_reconnect_keeps_its_broker_and_environment() {
    let mut registry = empty();
    registry.insert(record(alpaca("conn_a", 1))).unwrap();
    registry.set_state(&id("conn_a"), Revoked).unwrap();
    let others = [
        Candidate {
            environment: Environment::Live,
            ..candidate(1)
        },
        Candidate {
            broker: Broker::KrakenDerivativesUs,
            ..candidate(1)
        },
    ];
    for other in others {
        assert_eq!(
            registry.admit(&other),
            Err(ConnectError::ReconnectMismatch),
            "{other:?}"
        );
    }
    let mut kraken = alpaca("conn_a", 1);
    kraken.broker = Broker::KrakenDerivativesUs;
    assert_eq!(
        registry.insert(record(kraken)),
        Err(ConnectError::ReconnectMismatch),
        "insert re-checks the broker"
    );
    assert_eq!(
        registry.view(&id("conn_a")).unwrap().map(|v| v.state),
        Some(Revoked)
    );
    let mut kraken_demo = alpaca("conn_k", 5);
    kraken_demo.broker = Broker::KrakenDerivativesUs;
    registry.insert(record(kraken_demo.clone())).unwrap();
    registry.set_state(&id("conn_k"), Revoked).unwrap();
    let mut kraken_live = kraken_demo;
    kraken_live.environment = Environment::Live;
    assert_eq!(
        registry.insert(record(kraken_live)),
        Err(ConnectError::ReconnectMismatch),
        "insert re-checks the environment"
    );
    assert_eq!(
        registry.view(&id("conn_k")).unwrap().map(|v| v.environment),
        Some(Environment::Paper)
    );
}

#[test]
#[ignore = "pending E7-11"]
fn ids_are_unique_and_a_view_needs_a_record() {
    let mut registry = empty();
    registry.insert(record(alpaca("conn_a", 1))).unwrap();
    assert_eq!(
        registry.insert(record(alpaca("conn_a", 2))),
        Err(ConnectError::InvalidConnectionId)
    );
    registry.set_state(&id("conn_a"), Revoked).unwrap();
    assert_eq!(
        registry.insert(record(alpaca("conn_a", 2))),
        Err(ConnectError::InvalidConnectionId),
        "a revoked record's id is still its own, for another account"
    );
    assert_eq!(registry.view(&id("conn_x")), Ok(None));
    assert_eq!(registry.established(&id("conn_x")), Ok(None));
}

#[test]
#[ignore = "pending E7-11"]
fn every_connect_waits_while_the_fingerprint_key_rotates() {
    let revoked = {
        let mut registry = empty();
        registry.insert(record(alpaca("conn_a", 1))).unwrap();
        registry.set_state(&id("conn_a"), Revoked).unwrap();
        registry
    };
    let mut rotating = Registry::new(revoked.records.clone(), true).unwrap();
    for n in [1, 2] {
        assert_eq!(
            rotating.admit(&candidate(n)),
            Err(ConnectError::FingerprintRotating),
            "account {n}"
        );
    }
    for (new, what) in [
        (alpaca("conn_a", 1), "a reconnect"),
        (alpaca("conn_b", 2), "a new account"),
    ] {
        assert_eq!(
            rotating.insert(record(new)),
            Err(ConnectError::FingerprintRotating),
            "insert re-checks the rotation: {what}"
        );
    }
    assert_eq!(rotating.records, revoked.records, "nothing was added");
}

/// One step of a deployment's life.
#[derive(Debug, Clone)]
enum Step {
    /// Account `n` connects with the broker and environment drawn.
    Connect(u8, bool),
    /// Account `n`'s connection moves to a state.
    Move(u8, ConnectionState),
    /// A record for account `n` under another connection's id, or with another broker.
    Mismatched(u8),
    /// A new account, under a new id and fingerprint, with account `n`'s `account_ref`.
    StolenRef(u8),
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (0u8..4, any::<bool>()).prop_map(|(n, kraken)| Step::Connect(n, kraken)),
        (0u8..4, prop::sample::select(STATES.to_vec())).prop_map(|(n, s)| Step::Move(n, s)),
        (0u8..4).prop_map(Step::Mismatched),
        (0u8..4).prop_map(Step::StolenRef),
    ]
}

/// The registry's own invariant, read from its records: no two share a connection id, a
/// fingerprint, or an `account_ref` (CN-5).
fn holds_cn_5(registry: &Registry) -> bool {
    let records = &registry.records;
    records.iter().enumerate().all(|(i, a)| {
        records.iter().skip(i + 1).all(|b| {
            a.connection_id != b.connection_id
                && a.fingerprint != b.fingerprint
                && a.account_ref != b.account_ref
        })
    })
}

/// CN-5, CN-12 and §9.1, against an oracle that keeps its own map of which connection each account
/// was first bound to, with which broker, and in which state: an account is never connected twice;
/// once bound it keeps its connection id, `account_ref`, and broker through every revoke and
/// reconnect; a reconnect resumes the state its connection was revoked from; a state moves only as
/// `MOVES` lists; and the registry holds CN-5 after every step.
#[test]
#[ignore = "pending E7-11"]
fn one_account_one_connection_for_life() {
    let lives = prop::collection::vec(step(), 1..48);
    TestRunner::deterministic()
        .run(&lives, |steps| {
            let mut registry = empty();
            let mut bound: BTreeMap<u8, (ConnectionId, AccountRef, Broker)> = BTreeMap::new();
            let mut state: BTreeMap<u8, ConnectionState> = BTreeMap::new();
            let mut revoked_from: BTreeMap<u8, ConnectionState> = BTreeMap::new();
            let mut minted = 0u8;
            for step in steps {
                match step {
                    Step::Connect(n, kraken) => {
                        let broker = if kraken {
                            Broker::KrakenDerivativesUs
                        } else {
                            Broker::Alpaca
                        };
                        let asked = Candidate {
                            broker,
                            ..candidate(n)
                        };
                        let live = state.get(&n).is_some_and(|s| *s != Revoked);
                        let want = match (live, bound.get(&n)) {
                            (true, Some((existing, _, _))) => Err(ConnectError::AlreadyConnected {
                                existing: existing.clone(),
                            }),
                            (false, Some((_, _, first))) if *first != broker => {
                                Err(ConnectError::ReconnectMismatch)
                            }
                            (false, Some((connection_id, account_ref, _))) => {
                                Ok(Admission::Reconnect {
                                    connection_id: connection_id.clone(),
                                    account_ref: account_ref.clone(),
                                })
                            }
                            _ => Ok(Admission::New),
                        };
                        let got = registry.admit(&asked);
                        prop_assert_eq!(&got, &want);
                        let (connection_id, reference, resumed) = match got {
                            Ok(Admission::New) => {
                                minted += 1;
                                (id(&format!("conn_{minted}")), account_ref(minted), Active)
                            }
                            Ok(Admission::Reconnect {
                                connection_id,
                                account_ref,
                            }) => (connection_id, account_ref, revoked_from[&n]),
                            Err(_) => continue,
                        };
                        let mut new = alpaca(connection_id.as_str(), n);
                        new.broker = broker;
                        new.account_ref = reference.clone();
                        prop_assert_eq!(registry.insert(record(new)), Ok(()));
                        prop_assert_eq!(
                            registry.view(&connection_id).map(|v| v.map(|v| v.state)),
                            Ok(Some(resumed))
                        );
                        bound.entry(n).or_insert((connection_id, reference, broker));
                        state.insert(n, resumed);
                    }
                    Step::Move(n, to) => {
                        let Some((connection_id, _, _)) = bound.get(&n) else {
                            continue;
                        };
                        let from = state[&n];
                        let got = registry.set_state(connection_id, to);
                        if MOVES.contains(&(from, to)) {
                            prop_assert_eq!(got, Ok(()));
                            if to == Revoked {
                                revoked_from.insert(n, from);
                            }
                            state.insert(n, to);
                        } else {
                            prop_assert_eq!(got, Err(ConnectError::InvalidTransition { from, to }));
                        }
                    }
                    Step::Mismatched(n) => {
                        let Some((connection_id, _, _)) = bound.get(&n) else {
                            continue;
                        };
                        let mut stranger = alpaca(connection_id.as_str(), n.wrapping_add(100));
                        stranger.account_ref = account_ref(n.wrapping_add(100));
                        prop_assert_eq!(
                            registry.insert(record(stranger)),
                            Err(ConnectError::InvalidConnectionId)
                        );
                    }
                    Step::StolenRef(n) => {
                        let Some((_, reference, _)) = bound.get(&n) else {
                            continue;
                        };
                        let mut thief = alpaca("conn_thief", n.wrapping_add(200));
                        thief.account_ref = reference.clone();
                        prop_assert_eq!(
                            registry.insert(record(thief)),
                            Err(ConnectError::InvalidAccountRef)
                        );
                    }
                }
                prop_assert!(holds_cn_5(&registry), "{:?}", registry.records);
            }
            Ok(())
        })
        .unwrap();
}
