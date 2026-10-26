//! The exchanged executor's start (DEC-2339; connections spec §5.2 step 4): an account stream with
//! no `connect` check is checked once, over the lease the executor takes on its own connection,
//! and the lease is given back; a stream that holds one, passed or failed, is never checked again,
//! so no refusal is retried into a second create. The id read is hashed by the vault, and a
//! refused hash is an unreadable account (DEC-2345). The vault here is scripted, and nothing dials
//! a host.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mandate_canon::{Value, parse};
use mandate_connections::account_entry::BrokerAccountId;
use mandate_connections::grant::check_scope;
use mandate_connections::record::AccountRef as StreamRef;
use mandate_connections::record::{AccountPiiRef, ConnectionId};
use mandate_connections::vault::AccessToken;
use mandate_executor::{AccountRef, AccountScope, WorkspaceId};
use mandate_executord::credential::paper_account;
use mandate_executord::fixture::Ids;
use mandate_executord::oauth_check::{OAuthCheck, Start, exchanged_start, oauth_connect_check};
use mandate_executord::paper::FixturePaper;
use mandate_journal::{AppendOutcome, StoredEvent, StreamId};
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::fingerprint::AccountFingerprint;
use mandate_vault::lease::{Lease, lease_times};
use mandate_vault::personal::PiiRandom;
use mandate_vault::policy::{Identity, SecretPath};
use mandate_vault::source::{Grant, SecretSource};
use secrecy::{ExposeSecret, SecretSlice};

type Error = Box<dyn std::error::Error>;

const STREAM: &str = "acct:ws1:01J8Z2ACCT0000000000000001";
const TOKEN: &str = "exchangedStartSentinelToken-0123456789";
/// The broker's account id: a fake one, in Alpaca's form.
const BROKER_ID: &str = "904837e3-3b76-47ec-b432-046db621571b";
/// The vault's fingerprint of it: a fake one, in transit's form.
const HASH: &str = "vault:v1:q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA=";
const RANDOM: [u8; 10] = [0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13];

/// DEC-2624's token entry for [`TOKEN`] with `scope`, written out here rather than by its encoder.
fn entry(scope: &str) -> Vec<u8> {
    format!(r#"{{"access_token":"{TOKEN}","format":1,"scope":[{scope}],"token_type":"bearer"}}"#)
        .into_bytes()
}

fn full() -> Vec<u8> {
    entry(r#""data","trading""#)
}

fn scope() -> AccountScope {
    AccountScope {
        account: AccountRef("01J8Z2ACCT0000000000000001".to_owned()),
        workspace: WorkspaceId("ws1".to_owned()),
    }
}

fn now() -> Result<UtcNanos, Error> {
    Ok(UtcNanos::parse_rfc3339("2026-10-22T15:00:00.5Z")?)
}

fn connection() -> Result<ConnectionId, Error> {
    Ok(ConnectionId::new("conn1")?)
}

/// What the scripted vault was asked.
#[derive(Debug, Default)]
struct Calls {
    leased: Vec<String>,
    revoked: usize,
    created: Vec<String>,
    hashed: usize,
}

/// A vault session whose one lease answers `secret`, whose hash of [`BROKER_ID`] answers `hash`,
/// and whose creates answer `create`.
struct Session {
    identity: Identity,
    secret: Result<Vec<u8>, SourceError>,
    hash: Result<(), SourceError>,
    create: Result<(), SourceError>,
    calls: Rc<RefCell<Calls>>,
}

impl Session {
    fn new(secret: Result<Vec<u8>, SourceError>) -> Result<(Self, Rc<RefCell<Calls>>), Error> {
        let calls = Rc::new(RefCell::new(Calls::default()));
        let session = Self {
            identity: Identity::Executor(connection()?),
            secret,
            hash: Ok(()),
            create: Ok(()),
            calls: Rc::clone(&calls),
        };
        Ok((session, calls))
    }
}

impl SecretSource for Session {
    fn identity(&self) -> &Identity {
        &self.identity
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
    async fn lease(&self, connection: &ConnectionId, now: UtcNanos) -> Result<Lease, SourceError> {
        self.calls
            .borrow_mut()
            .leased
            .push(connection.as_str().to_owned());
        let secret = self.secret.clone()?;
        let (renew_at, expires_at) = lease_times(now, None).ok_or(SourceError::Malformed)?;
        Ok(Lease {
            connection: connection.clone(),
            secret: SecretSlice::from(secret),
            expires_at,
            renew_at,
        })
    }
    async fn renew(&self, _: &Lease, _: UtcNanos) -> Result<Lease, SourceError> {
        Err(SourceError::Unavailable)
    }
    async fn revoke(&self, _: Lease) -> Result<(), SourceError> {
        self.calls.borrow_mut().revoked += 1;
        Ok(())
    }
    async fn issue_grant(&self, _: &ConnectionId, _: UtcNanos) -> Result<Grant, SourceError> {
        Err(SourceError::Denied)
    }
    async fn revoke_grant(&self, _: &Grant) -> Result<(), SourceError> {
        Err(SourceError::Denied)
    }
    async fn create(&self, path: &SecretPath, _: &SecretSlice<u8>) -> Result<(), SourceError> {
        self.calls.borrow_mut().created.push(format!("{path:?}"));
        self.create.clone()
    }
    async fn hmac_account(
        &self,
        account: &BrokerAccountId,
    ) -> Result<AccountFingerprint, SourceError> {
        self.calls.borrow_mut().hashed += 1;
        if account.secret().expose_secret() != BROKER_ID {
            return Err(SourceError::Malformed);
        }
        self.hash.clone()?;
        AccountFingerprint::new(HASH)
    }
}

/// A random source that answers [`RANDOM`], or fails.
struct Fixed(bool);

impl PiiRandom for Fixed {
    fn fill(&mut self, bytes: &mut [u8; 10]) -> Result<(), SourceError> {
        if !self.0 {
            return Err(SourceError::Unavailable);
        }
        *bytes = RANDOM;
        Ok(())
    }
}

fn rows(store: &MemoryStore) -> Vec<StoredEvent> {
    StreamId::parse(STREAM)
        .and_then(|stream| store.rows(&stream))
        .unwrap_or_default()
}

/// Each check's result in the last `ConnectionChecked`, as (check, result).
fn results(store: &MemoryStore) -> Result<Vec<(String, String)>, Error> {
    let rows = rows(store);
    let row = rows
        .iter()
        .rev()
        .find(|row| row.event_type == "ConnectionChecked")
        .ok_or("no ConnectionChecked")?;
    let body = parse(&row.body)?;
    let Some(Value::Array(results)) = body.get("payload").and_then(|p| p.get("results")) else {
        return Err("no results".into());
    };
    let text = |v: &Value, m: &str| v.get(m).and_then(Value::as_str).map(str::to_owned);
    Ok(results
        .iter()
        .map(|r| {
            (
                text(r, "check").unwrap_or_default(),
                text(r, "result").unwrap_or_default(),
            )
        })
        .collect())
}

/// The start's answer over `session`, with the account read `account` and a random source that
/// fills when `entropy`; the token the account read was handed is recorded in `handed`.
fn started(
    store: &mut MemoryStore,
    session: &Session,
    account: Option<&str>,
    entropy: bool,
    handed: &RefCell<Vec<String>>,
) -> Result<Result<Option<bool>, &'static str>, Error> {
    let account = account.map(BrokerAccountId::new).transpose()?;
    let connection = connection()?;
    let answer = exchanged_start(
        store,
        &Ids::of(&scope()),
        &scope(),
        Start {
            connection: &connection,
            vault: session,
            account: |token: AccessToken| {
                handed.borrow_mut().push(token.0.expose_secret().to_owned());
                account
            },
            random: &mut Fixed(entropy),
        },
        now()?,
    );
    Ok(answer.map(|checked| checked.map(|c| c.passed)))
}

/// A fresh stream is checked once, over the lease on the executor's own connection: the account
/// read is handed the lease's token, the id it read is hashed once, the fingerprint entry and then
/// the account entry are created once each, the record holds every check passed, and the lease is
/// given back.
#[test]
fn a_fresh_stream_is_checked_once_over_the_lease_which_is_given_back() -> Result<(), Error> {
    let mut store = MemoryStore::default();
    let (session, calls) = Session::new(Ok(full()))?;
    let handed = RefCell::new(Vec::new());
    let answer = started(&mut store, &session, Some(BROKER_ID), true, &handed)?;
    assert_eq!(answer, Ok(Some(true)));
    assert_eq!(handed.borrow().as_slice(), [TOKEN]);
    let calls = calls.borrow();
    assert_eq!(calls.leased, ["conn1"]);
    assert_eq!(calls.revoked, 1);
    assert_eq!(calls.hashed, 1);
    let request = StreamRef::new("01J8Z2ACCT0000000000000001")?;
    assert_eq!(
        calls.created,
        [
            format!(
                "{:?}",
                SecretPath::Fingerprint(connection()?, request.clone())
            ),
            format!("{:?}", SecretPath::AccountId(connection()?, request)),
        ]
    );
    let passed = |check: &str| (check.to_owned(), "passed".to_owned());
    assert_eq!(
        results(&store)?,
        [passed("account"), passed("environment"), passed("scope")]
    );
    Ok(())
}

/// Item (a): the scopes checked are read from the lease's own token entry and nothing else. A lease
/// whose secret is not a token entry stored with exactly the request (one with fewer scopes, or
/// any other bytes) refuses the start as `credential_malformed`, before any create or record, and
/// the lease is given back.
#[test]
fn the_scopes_are_read_from_the_lease_alone() -> Result<(), Error> {
    for secret in [entry(r#""data""#), b"not a token entry".to_vec()] {
        let mut store = MemoryStore::default();
        let (session, calls) = Session::new(Ok(secret))?;
        let answer = started(
            &mut store,
            &session,
            Some(BROKER_ID),
            true,
            &RefCell::default(),
        )?;
        assert_eq!(answer, Err("credential_malformed"));
        let calls = calls.borrow();
        assert_eq!(calls.leased, ["conn1"]);
        assert!(calls.created.is_empty());
        assert_eq!(calls.revoked, 1);
        assert!(rows(&store).is_empty());
    }
    Ok(())
}

/// A stream that holds a `connect` check, passed or failed, is not checked again: no lease, no
/// create, nothing appended. A refused connect stays refused, and the owner reconnects.
#[test]
fn a_stream_with_a_connect_check_runs_none() -> Result<(), Error> {
    for account in [Some(BROKER_ID), None] {
        let mut store = MemoryStore::default();
        oauth_connect_check(
            &mut store,
            &Ids::of(&scope()),
            &scope(),
            OAuthCheck {
                connection: connection()?,
                granted: check_scope("trading data")?,
                account: account.map(BrokerAccountId::new).transpose()?,
                fingerprint: account.map(|_| AccountFingerprint::new(HASH)).transpose()?,
                pii_ref: AccountPiiRef::new("pii_01J8Z2PQ000000000000000001")?,
            },
            |_, _| Ok(()),
            now()?,
        )?;
        let before = rows(&store).len();
        let (session, calls) = Session::new(Ok(full()))?;
        let answer = started(
            &mut store,
            &session,
            Some(BROKER_ID),
            true,
            &RefCell::default(),
        )?;
        assert_eq!(answer, Ok(None), "{account:?}");
        let calls = calls.borrow();
        assert!(calls.leased.is_empty() && calls.created.is_empty());
        assert_eq!(rows(&store).len(), before);
    }
    Ok(())
}

/// Item (c): a random source that cannot fill refuses the start as `entropy_unavailable` before any
/// create and before any record, and the lease is given back.
#[test]
fn an_entropy_failure_refuses_the_start_before_any_create() -> Result<(), Error> {
    let mut store = MemoryStore::default();
    let (session, calls) = Session::new(Ok(full()))?;
    let answer = started(
        &mut store,
        &session,
        Some(BROKER_ID),
        false,
        &RefCell::default(),
    )?;
    assert_eq!(answer, Err("entropy_unavailable"));
    let calls = calls.borrow();
    assert!(calls.created.is_empty());
    assert_eq!(calls.revoked, 1);
    assert!(
        rows(&store)
            .iter()
            .all(|row| row.event_type != "ConnectionChecked")
    );
    Ok(())
}

/// A hash the vault refuses is an unreadable account: nothing is created, the record holds a null
/// reference, and the lease is given back; an account the paper host did not answer is never sent
/// to be hashed (DEC-2345 item 7).
#[test]
fn a_refused_hash_is_an_unreadable_account() -> Result<(), Error> {
    for (account, hashed) in [(Some(BROKER_ID), 1), (None, 0)] {
        let mut store = MemoryStore::default();
        let (mut session, calls) = Session::new(Ok(full()))?;
        session.hash = Err(SourceError::Unavailable);
        let answer = started(&mut store, &session, account, true, &RefCell::default())?;
        assert_eq!(answer, Ok(Some(false)), "{account:?}");
        let calls = calls.borrow();
        assert_eq!(calls.hashed, hashed, "{account:?}");
        assert!(calls.created.is_empty(), "{account:?}");
        assert_eq!(calls.revoked, 1, "{account:?}");
        assert!(
            results(&store)?.contains(&("account".to_owned(), "failed".to_owned())),
            "{account:?}"
        );
    }
    Ok(())
}

/// A lease the vault refuses refuses the start, with nothing created and nothing appended.
#[test]
fn a_refused_lease_refuses_the_start() -> Result<(), Error> {
    let mut store = MemoryStore::default();
    let (session, calls) = Session::new(Err(SourceError::Denied))?;
    let answer = started(
        &mut store,
        &session,
        Some(BROKER_ID),
        true,
        &RefCell::default(),
    )?;
    assert_eq!(answer, Err("credential_unavailable"));
    assert!(calls.borrow().created.is_empty());
    assert!(rows(&store).is_empty());
    Ok(())
}

/// One route through the start: the leased secret, the vault's answers to the hash and the create,
/// whether the random source fills, and the answer expected.
type Route = (
    Vec<u8>,
    Result<(), SourceError>,
    Result<(), SourceError>,
    bool,
    Result<Option<bool>, &'static str>,
);

/// Item (b): on every route, passed or refused, no journaled row holds the account id, and no code
/// the start answers does, so the stderr line the binary prints from it cannot either.
#[test]
fn the_account_id_is_in_no_record_and_no_code() -> Result<(), Error> {
    let routes: [Route; 5] = [
        (full(), Ok(()), Ok(()), true, Ok(Some(true))),
        (
            entry(r#""data""#),
            Ok(()),
            Ok(()),
            true,
            Err("credential_malformed"),
        ),
        (
            full(),
            Err(SourceError::Unavailable),
            Ok(()),
            true,
            Ok(Some(false)),
        ),
        (
            full(),
            Ok(()),
            Err(SourceError::Denied),
            true,
            Ok(Some(false)),
        ),
        (full(), Ok(()), Ok(()), false, Err("entropy_unavailable")),
    ];
    for (secret, hash, create, entropy, expected) in routes {
        let mut store = MemoryStore::default();
        let (mut session, _) = Session::new(Ok(secret))?;
        session.hash = hash;
        session.create = create;
        let answer = started(
            &mut store,
            &session,
            Some(BROKER_ID),
            entropy,
            &RefCell::default(),
        )?;
        assert_eq!(answer, expected);
        if let Err(code) = answer {
            assert!(!code.contains(BROKER_ID), "{code}");
        }
        for row in rows(&store) {
            let body = String::from_utf8_lossy(&row.body);
            assert!(!body.contains(BROKER_ID), "{}", row.event_type);
            assert!(!body.contains(TOKEN), "{}", row.event_type);
        }
    }
    Ok(())
}

/// The account read the binary hands the leased token to reads the paper host's own id of the
/// account, over `mandate-alpaca`'s client.
#[test]
fn the_account_is_read_from_the_paper_host() -> Result<(), Error> {
    let id = paper_account(FixturePaper::new(now()?))?;
    assert_eq!(id, BrokerAccountId::new("fixture")?);
    Ok(())
}

/// A journal whose first `reads` reads of a stream fail, as a store that cannot be read for a
/// moment does.
struct Blind {
    journal: MemoryStore,
    reads: Cell<usize>,
}

impl Store for Blind {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        self.journal.take_ownership(stream)
    }

    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        let left = self.reads.get();
        if left > 0 {
            self.reads.set(left - 1);
            return None;
        }
        self.journal.rows(stream)
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        current_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        self.journal
            .append(stream, expected_head, current_epoch, recorded_at, drafts)
    }
}

/// A stream the start cannot read is never taken for one with no check: a refused connect stays
/// refused through a moment the journal cannot be read, with no lease, no create and no append.
#[test]
fn an_unreadable_stream_refuses_the_start() -> Result<(), Error> {
    let mut journal = MemoryStore::default();
    oauth_connect_check(
        &mut journal,
        &Ids::of(&scope()),
        &scope(),
        OAuthCheck {
            connection: connection()?,
            granted: check_scope("trading data")?,
            account: None,
            fingerprint: None,
            pii_ref: AccountPiiRef::new("pii_01J8Z2PQ000000000000000001")?,
        },
        |_, _| Ok(()),
        now()?,
    )?;
    let before = rows(&journal).len();
    let mut blind = Blind {
        journal: journal.clone(),
        reads: Cell::new(1),
    };
    let (session, calls) = Session::new(Ok(full()))?;
    let connection = connection()?;
    let answer = exchanged_start(
        &mut blind,
        &Ids::of(&scope()),
        &scope(),
        Start {
            connection: &connection,
            vault: &session,
            account: |_: AccessToken| BrokerAccountId::new(BROKER_ID).ok(),
            random: &mut Fixed(true),
        },
        now()?,
    );
    assert_eq!(
        answer.map(|checked| checked.is_some()),
        Err("stream_unavailable")
    );
    let calls = calls.borrow();
    assert!(calls.leased.is_empty() && calls.created.is_empty());
    assert_eq!(rows(&journal).len(), before);
    Ok(())
}
