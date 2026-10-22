//! The exchanged executor's own connection and connect check (DEC-2336, DEC-2339): its connection
//! read back from the ServiceAccount it runs as, and the check composed from its lease, the paper
//! host's answer, and a reference minted from its random source.

use std::cell::RefCell;

use mandate_canon::{Value, decode_ulid, parse};
use mandate_connections::account_entry::{BrokerAccountId, decode};
use mandate_connections::record::ConnectionId;
use mandate_executor::{AccountRef, AccountScope, WorkspaceId};
use mandate_executord::credential::subject_connection;
use mandate_executord::fixture::Ids;
use mandate_executord::oauth_check::{Exchanged, exchanged_check};
use mandate_journal::StreamId;
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::personal::PiiRandom;
use secrecy::ExposeSecret;

type Error = Box<dyn std::error::Error>;

const STREAM: &str = "acct:ws1:01J8Z2ACCT0000000000000001";
const ENTRY: &[u8] =
    br#"{"access_token":"tok-1","format":1,"scope":["data","trading"],"token_type":"bearer"}"#;
/// The broker's account id: a fake one, in Alpaca's form.
const BROKER_ID: &str = "904837e3-3b76-47ec-b432-046db621571b";

fn scope() -> AccountScope {
    AccountScope {
        account: AccountRef("01J8Z2ACCT0000000000000001".to_owned()),
        workspace: WorkspaceId("ws1".to_owned()),
    }
}

fn now() -> Result<UtcNanos, Error> {
    Ok(UtcNanos::parse_rfc3339("2026-10-22T15:00:00.5Z")?)
}

/// A source that answers `bytes`, or fails.
struct Fixed(Option<[u8; 10]>);

impl PiiRandom for Fixed {
    fn fill(&mut self, bytes: &mut [u8; 10]) -> Result<(), SourceError> {
        *bytes = self.0.ok_or(SourceError::Unavailable)?;
        Ok(())
    }
}

fn held(leased: &[u8]) -> Result<Exchanged<'_>, Error> {
    Ok(Exchanged {
        connection: ConnectionId::new("conn1")?,
        leased,
        account: Some(BrokerAccountId::new(BROKER_ID)?),
    })
}

/// The check reads the scopes from the lease's token entry and keeps the account under a reference
/// minted at its time from its source: the entry and the journaled record hold that one reference.
#[test]
fn the_check_keeps_the_account_under_a_reference_minted_from_its_source() -> Result<(), Error> {
    let mut store = MemoryStore::default();
    let created = RefCell::new(Vec::new());
    let random = [0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13];
    let checked = exchanged_check(
        &mut store,
        &Ids,
        &scope(),
        held(ENTRY)?,
        &mut Fixed(Some(random)),
        |_, secret| {
            created.borrow_mut().push(secret.expose_secret().to_vec());
            Ok(())
        },
        now()?,
    )?;
    assert!(checked.passed);
    let created = created.borrow();
    let [entry] = created.as_slice() else {
        return Err("one create".into());
    };
    let pii_ref = decode(entry)?.pii_ref.as_str().to_owned();
    let ulid = decode_ulid(pii_ref.strip_prefix("pii_").ok_or("pii_")?)?;
    assert_eq!(ulid >> 80, 1_792_681_200_500);
    assert_eq!(ulid & ((1 << 80) - 1), 0x0a0b_0c0d_0e0f_1011_1213);
    let stream = StreamId::parse(STREAM).ok_or("stream")?;
    let rows = store.rows(&stream).ok_or("rows")?;
    let last = rows.last().ok_or("no rows")?;
    let payload = parse(&last.body)?
        .get("payload")
        .cloned()
        .ok_or("payload")?;
    assert_eq!(
        payload.get("account_pii_ref").and_then(Value::as_str),
        Some(pii_ref.as_str())
    );
    Ok(())
}

/// A lease that holds no token entry, or a source that cannot fill, stops the check before any
/// create or record, naming neither the secret nor the id.
#[test]
fn a_bad_lease_or_a_failed_source_creates_and_journals_nothing() -> Result<(), Error> {
    for (leased, random, want) in [
        (&b"tok-1"[..], Some([0; 10]), "credential_malformed"),
        (ENTRY, None, "entropy_unavailable"),
    ] {
        let mut store = MemoryStore::default();
        let created = RefCell::new(0_u32);
        let refused = exchanged_check(
            &mut store,
            &Ids,
            &scope(),
            held(leased)?,
            &mut Fixed(random),
            |_, _| {
                *created.borrow_mut() += 1;
                Ok(())
            },
            now()?,
        );
        assert_eq!(refused.map(drop), Err(want));
        assert_eq!(*created.borrow(), 0, "{want}");
        let stream = StreamId::parse(STREAM).ok_or("stream")?;
        assert_eq!(
            store.rows(&stream).map(|rows| rows.len()),
            Some(0),
            "{want}"
        );
    }
    Ok(())
}

/// The pod's ServiceAccount, its connection id's bytes in lowercase hex, reads back as that
/// connection, for every id form.
#[test]
fn the_subject_account_reads_back_as_its_connection() -> Result<(), Error> {
    for id in ["c1", "conn1", "01J8Z2CONN0000000000000001", "a_b-C"] {
        let hex: String = id.bytes().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            subject_connection(Some(hex))?,
            ConnectionId::new(id)?,
            "{id}"
        );
    }
    Ok(())
}

/// Anything but one connection's own name is refused: none, empty, upper-case digits, an odd
/// length, a character that is no hex digit, and bytes that are no connection id.
#[test]
fn any_other_subject_account_is_refused() {
    for configured in [
        None,
        Some(String::new()),
        Some("636F6E6E31".to_owned()),
        Some("636f6e6e3".to_owned()),
        Some("636f6e6e3g".to_owned()),
        Some("x636f6e6e31".to_owned()),
        Some("612062".to_owned()),
        Some("c3a9".to_owned()),
    ] {
        assert_eq!(
            subject_connection(configured.clone()),
            Err("executor_subject_account"),
            "{configured:?}"
        );
    }
}
