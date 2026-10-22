//! The connect check of an exchanged OAuth connection (DEC-2339; connections spec §5.2 step 4,
//! §8.1; journal spec §9.8): the account id goes to the connection's personal-data entry once,
//! the journal holds only its reference, and any refusal of the entry is an unreadable account.

use std::cell::RefCell;
use std::collections::BTreeMap;

use mandate_canon::{Value, parse};
use mandate_connections::account_entry::{BrokerAccountId, decode};
use mandate_connections::grant::{GrantedScopes, check_scope};
use mandate_connections::record::AccountRef as StreamRef;
use mandate_connections::record::{AccountPiiRef, ConnectionId};
use mandate_executor::{AccountRef, AccountScope, WorkspaceId};
use mandate_executord::fixture::Ids;
use mandate_executord::oauth_check::{OAuthCheck, leased_scopes, oauth_connect_check};
use mandate_journal::{AppendOutcome, StoredEvent, StreamId};
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::policy::SecretPath;
use secrecy::{ExposeSecret, SecretSlice};

type Error = Box<dyn std::error::Error>;

const ACCOUNT: &str = "01J8Z2ACCT0000000000000001";
const STREAM: &str = "acct:ws1:01J8Z2ACCT0000000000000001";
const PII: &str = "pii_01J8Z2PQ000000000000000001";
const RETRY_PII: &str = "pii_01J8Z2PQ000000000000000002";
/// The broker's account id: a fake one, in Alpaca's form.
const BROKER_ID: &str = "904837e3-3b76-47ec-b432-046db621571b";

fn scope() -> AccountScope {
    AccountScope {
        account: AccountRef(ACCOUNT.to_owned()),
        workspace: WorkspaceId("ws1".to_owned()),
    }
}

fn now() -> Result<UtcNanos, Error> {
    Ok(UtcNanos::parse_rfc3339("2026-10-22T15:00:00.5Z")?)
}

fn connection() -> Result<ConnectionId, Error> {
    Ok(ConnectionId::new("conn1")?)
}

fn granted() -> Result<GrantedScopes, Error> {
    Ok(check_scope("trading data")?)
}

fn check(account: Option<&str>, pii: &str) -> Result<OAuthCheck, Error> {
    Ok(OAuthCheck {
        connection: connection()?,
        granted: granted()?,
        account: account.map(BrokerAccountId::new).transpose()?,
        pii_ref: AccountPiiRef::new(pii)?,
    })
}

/// The personal-data vault as the executor's policy lets it see its entry: create once, no read,
/// no overwrite. Each path's bytes, and every create asked for.
#[derive(Default)]
struct CreateOnce {
    entries: RefCell<BTreeMap<String, Vec<u8>>>,
    asked: RefCell<Vec<String>>,
}

impl CreateOnce {
    fn create(&self, path: &SecretPath, secret: &SecretSlice<u8>) -> Result<(), SourceError> {
        let key = format!("{path:?}");
        self.asked.borrow_mut().push(key.clone());
        let mut entries = self.entries.borrow_mut();
        if entries.contains_key(&key) {
            return Err(SourceError::Denied);
        }
        entries.insert(key, secret.expose_secret().to_vec());
        Ok(())
    }
}

/// A journal that loses every append, as a process that crashes after the entry is created and
/// before its record commits; with `owned` false it cannot give the stream at all.
struct Losing {
    journal: MemoryStore,
    owned: bool,
}

impl Store for Losing {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        self.journal.take_ownership(stream).filter(|_| self.owned)
    }

    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        self.journal.rows(stream)
    }

    fn append(
        &mut self,
        _stream: &StreamId,
        _expected_head: u64,
        current_epoch: u64,
        _recorded_at: UtcNanos,
        _drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        AppendOutcome::Fenced { current_epoch }
    }
}

fn rows(store: &MemoryStore) -> Result<Vec<StoredEvent>, Error> {
    let stream = StreamId::parse(STREAM).ok_or("stream")?;
    Ok(store.rows(&stream).ok_or("rows")?)
}

/// The last `ConnectionChecked`'s payload and envelope `pii_refs`.
fn last_checked(store: &MemoryStore) -> Result<(Value, Value), Error> {
    let rows = rows(store)?;
    let row = rows
        .iter()
        .rev()
        .find(|row| row.event_type == "ConnectionChecked")
        .ok_or("no ConnectionChecked")?;
    let body = parse(&row.body)?;
    Ok((
        body.get("payload").cloned().ok_or("payload")?,
        body.get("pii_refs").cloned().ok_or("pii_refs")?,
    ))
}

fn results(payload: &Value) -> Vec<(String, String, Option<String>)> {
    let text = |v: &Value, m: &str| v.get(m).and_then(Value::as_str).map(str::to_owned);
    match payload.get("results") {
        Some(Value::Array(results)) => results
            .iter()
            .map(|r| {
                (
                    text(r, "check").unwrap_or_default(),
                    text(r, "result").unwrap_or_default(),
                    text(r, "reason"),
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A read account goes to the connection's own entry, once, in the entry's form under the minted
/// reference; the journal opens the stream and records every check passed as an Alpaca paper
/// OAuth connection, holding the reference and never the id.
#[test]
fn a_read_account_is_kept_in_its_entry_and_only_its_reference_is_journaled() -> Result<(), Error> {
    let mut store = MemoryStore::default();
    let vault = CreateOnce::default();
    let checked = oauth_connect_check(
        &mut store,
        &Ids,
        &scope(),
        check(Some(BROKER_ID), PII)?,
        |path, secret| vault.create(path, secret),
        now()?,
    )?;
    assert!(checked.passed);
    let path = format!(
        "{:?}",
        SecretPath::AccountId(connection()?, StreamRef::new(ACCOUNT)?)
    );
    assert_eq!(vault.asked.borrow().as_slice(), std::slice::from_ref(&path));
    let entries = vault.entries.borrow();
    let entry = decode(entries.get(&path).ok_or("no entry")?)?;
    assert_eq!(entry.pii_ref.as_str(), PII);
    assert_eq!(entry.account_id, BrokerAccountId::new(BROKER_ID)?);
    let types: Vec<String> = rows(&store)?.into_iter().map(|r| r.event_type).collect();
    assert_eq!(types, ["StreamOpened", "ConnectionChecked"]);
    let (payload, pii_refs) = last_checked(&store)?;
    assert_eq!(
        payload.get("account_pii_ref").and_then(Value::as_str),
        Some(PII)
    );
    assert_eq!(pii_refs, Value::Array(vec![Value::Str(PII.to_owned())]));
    let passed = |check: &str| (check.to_owned(), "passed".to_owned(), None);
    assert_eq!(
        results(&payload),
        [passed("account"), passed("environment"), passed("scope")]
    );
    for row in rows(&store)? {
        assert!(
            !String::from_utf8_lossy(&row.body).contains(BROKER_ID),
            "{}",
            row.event_type
        );
    }
    Ok(())
}

/// An account the paper host did not answer creates no entry and is journaled as unreadable: a
/// null reference in the payload and none in the envelope (rule 62).
#[test]
fn an_unread_account_creates_nothing_and_journals_a_null_reference() -> Result<(), Error> {
    let mut store = MemoryStore::default();
    let vault = CreateOnce::default();
    let checked = oauth_connect_check(
        &mut store,
        &Ids,
        &scope(),
        check(None, PII)?,
        |path, secret| vault.create(path, secret),
        now()?,
    )?;
    assert!(!checked.passed);
    assert!(vault.asked.borrow().is_empty());
    let (payload, pii_refs) = last_checked(&store)?;
    assert_eq!(payload.get("account_pii_ref"), Some(&Value::Null));
    assert_eq!(pii_refs, Value::Array(Vec::new()));
    assert!(results(&payload).contains(&(
        "account".to_owned(),
        "failed".to_owned(),
        Some("account_unreadable".to_owned())
    )));
    Ok(())
}

/// DEC-2339 item 6: an executor that created its entry and crashed before the record committed
/// finds its retry's create refused, cannot read the entry back, and so fails closed: the retry
/// journals the account as unreadable, under no reference, and the first entry is left as it was.
#[test]
fn a_retry_after_a_crash_before_the_record_fails_closed() -> Result<(), Error> {
    let journal = MemoryStore::default();
    let vault = CreateOnce::default();
    let crashed = oauth_connect_check(
        &mut Losing {
            journal: journal.clone(),
            owned: true,
        },
        &Ids,
        &scope(),
        check(Some(BROKER_ID), PII)?,
        |path, secret| vault.create(path, secret),
        now()?,
    );
    assert_eq!(crashed.map(drop), Err("append_refused"));
    assert!(rows(&journal)?.is_empty());
    let mut store = journal.clone();
    let retried = oauth_connect_check(
        &mut store,
        &Ids,
        &scope(),
        check(Some(BROKER_ID), RETRY_PII)?,
        |path, secret| vault.create(path, secret),
        now()?,
    )?;
    assert!(!retried.passed);
    assert_eq!(vault.asked.borrow().len(), 2);
    let (payload, pii_refs) = last_checked(&store)?;
    assert_eq!(payload.get("account_pii_ref"), Some(&Value::Null));
    assert_eq!(pii_refs, Value::Array(Vec::new()));
    assert!(results(&payload).contains(&(
        "account".to_owned(),
        "failed".to_owned(),
        Some("account_unreadable".to_owned())
    )));
    let path = format!(
        "{:?}",
        SecretPath::AccountId(connection()?, StreamRef::new(ACCOUNT)?)
    );
    let entries = vault.entries.borrow();
    assert_eq!(
        decode(entries.get(&path).ok_or("no entry")?)?
            .pii_ref
            .as_str(),
        PII
    );
    Ok(())
}

/// Every refusal of the create, not only `Denied`, is an unreadable account: the check never
/// journals a reference it did not store.
#[test]
fn every_refused_create_journals_a_null_reference() -> Result<(), Error> {
    for refusal in [
        SourceError::Denied,
        SourceError::Unavailable,
        SourceError::Sealed,
        SourceError::Missing,
        SourceError::Config,
    ] {
        let mut store = MemoryStore::default();
        let checked = oauth_connect_check(
            &mut store,
            &Ids,
            &scope(),
            check(Some(BROKER_ID), PII)?,
            |_, _| Err(refusal.clone()),
            now()?,
        )?;
        assert!(!checked.passed, "{refusal:?}");
        let (payload, _) = last_checked(&store)?;
        assert_eq!(
            payload.get("account_pii_ref"),
            Some(&Value::Null),
            "{refusal:?}"
        );
    }
    Ok(())
}

/// A scope whose account is not an account stream's reference names no entry: the check creates
/// nothing, and journals the account as unreadable under no reference, rather than an entry at
/// some other path.
#[test]
fn an_account_that_is_no_stream_reference_creates_nothing() -> Result<(), Error> {
    let mut store = MemoryStore::default();
    let vault = CreateOnce::default();
    let scope = AccountScope {
        account: AccountRef("acct1".to_owned()),
        workspace: WorkspaceId("ws1".to_owned()),
    };
    let checked = oauth_connect_check(
        &mut store,
        &Ids,
        &scope,
        check(Some(BROKER_ID), PII)?,
        |path, secret| vault.create(path, secret),
        now()?,
    )?;
    assert!(!checked.passed);
    assert!(vault.asked.borrow().is_empty());
    let stream = StreamId::parse("acct:ws1:acct1").ok_or("stream")?;
    let rows = store.rows(&stream).ok_or("rows")?;
    let row = rows
        .iter()
        .rev()
        .find(|row| row.event_type == "ConnectionChecked")
        .ok_or("no ConnectionChecked")?;
    let payload = parse(&row.body)?.get("payload").cloned().ok_or("payload")?;
    assert_eq!(payload.get("account_pii_ref"), Some(&Value::Null));
    assert!(results(&payload).contains(&(
        "account".to_owned(),
        "failed".to_owned(),
        Some("account_unreadable".to_owned())
    )));
    Ok(())
}

/// The entry is created only once the account stream is this writer's: a journal that cannot give
/// the stream stops the check before any create.
#[test]
fn nothing_is_created_before_the_stream_is_taken() -> Result<(), Error> {
    let vault = CreateOnce::default();
    let refused = oauth_connect_check(
        &mut Losing {
            journal: MemoryStore::default(),
            owned: false,
        },
        &Ids,
        &scope(),
        check(Some(BROKER_ID), PII)?,
        |path, secret| vault.create(path, secret),
        now()?,
    );
    assert_eq!(refused.map(drop), Err("stream_unavailable"));
    assert!(vault.asked.borrow().is_empty());
    Ok(())
}

/// The scopes are read from the leased token entry by its one reader; any other bytes are
/// refused, naming nothing of them.
#[test]
fn the_scopes_are_read_from_the_leased_token_entry() -> Result<(), Error> {
    let entry =
        br#"{"access_token":"tok-1","format":1,"scope":["data","trading"],"token_type":"bearer"}"#;
    assert_eq!(leased_scopes(entry), Ok(granted()?));
    for malformed in [
        &br#"{"access_token":"tok-1","format":2,"scope":["data","trading"],"token_type":"bearer"}"#
            [..],
        b"tok-1",
        b"",
    ] {
        assert_eq!(leased_scopes(malformed), Err("credential_malformed"));
    }
    Ok(())
}
