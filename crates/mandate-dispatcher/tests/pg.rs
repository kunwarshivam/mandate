//! The dispatcher's journal over a real Postgres journal (E8-10 slice D10a, DEC-2500 items 3 and
//! 8, notifications spec v0.8 §5.9): a differential oracle against the in-memory journal, a second
//! dispatcher that fences the first before its send, and a read that fails as the closed
//! `JournalUnavailable`. The suite's PG class: each test skips unless `MANDATE_PG_URL` is set, and
//! fails without it where `MANDATE_PG_REQUIRED` is (DEC-109, DEC-519). Each first checks, with no
//! database, that a DSN that does not parse is refused closed, so the adapter is reached on every
//! run. Each then runs its database part on its own thread under [`DEADLINE`], so an adapter whose
//! runtime stops driving between calls fails fast as a deadlock instead of hanging the suite past
//! its ten-minute budget (DEC-464). The journal is a local test schema and nothing here carries a
//! credential (`AGENTS.md` rules 7 and 8).

#[path = "../../mandate-journal/tests/common/mod.rs"]
#[allow(
    unused_imports,
    reason = "only the journal's support needs these; the suite runs in `mandate-journal-pg`"
)]
mod common;
#[path = "../../mandate-journal/tests/conformance/mod.rs"]
#[allow(
    unused_imports,
    unused_macros,
    reason = "only `support`'s backend trait is used here; the suite runs in `mandate-journal-pg`"
)]
mod conformance;
#[path = "../../mandate-journal-pg/tests/support/mod.rs"]
mod support;

use std::cell::RefCell;
use std::error::Error;
use std::panic;
use std::rc::Rc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use mandate_dispatcher::{Config, DispatchError, Journal, NoticeWriter, PgNotices, step};
use mandate_journal::{AppendOutcome, Environment, MemoryJournal, StoredEvent, StreamId};
use mandate_journal_pg::{APP_ROLE, PgJournal};
use mandate_notify::{
    AddressHandle, FixtureProvider, NotifyError, Origin, PushChannel, Recipient, SecureRandom,
};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{Config as Cases, TestCaseError, TestRunner};

type Checked = Result<(), Box<dyn Error>>;

const NTF: &str = "ntf:ws_1";
const A1: &str = "acct:ws_1:A1";
const AT: &str = "2026-10-17T14:00:00.000000000Z";
const BUILD: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const AUDIENCE: [(&str, PushChannel); 2] = [
    ("founder", PushChannel::Email),
    ("founder", PushChannel::Phone),
];
const KINDS: [&str; 3] = ["risk_limit", "agent_held", "kill_switch"];
const MARK: &str = r#"{"instrument_id":"inst","price":"1","source":"quote","feed":"iex",
    "risk_clock":"2026-10-17T14:00:00.000000000Z"}"#;

/// How long one test's database part may take: far beyond what a correct adapter needs, so only a
/// deadlock reaches it.
const DEADLINE: Duration = Duration::from_secs(60);

/// Runs `body` on its own thread and waits at most [`DEADLINE`] for it: a panic in `body` is
/// re-raised here with its own message, and a body still running at the deadline fails as a
/// deadlock. A transaction the adapter drops (an append that is `Fenced`) is rolled back by sqlx on
/// a spawned task, so an adapter whose runtime idles between calls keeps the stream's row locked
/// and the next writer waits forever.
fn within_deadline(body: fn() -> Checked) -> Checked {
    let (done, finished) = mpsc::channel();
    let running = thread::spawn(move || {
        let outcome = body().map_err(|e| e.to_string());
        let _ = done.send(outcome);
    });
    match finished.recv_timeout(DEADLINE) {
        Ok(outcome) => Ok(outcome?),
        Err(RecvTimeoutError::Timeout) => Err(format!(
            "deadlocked: the adapter's runtime must keep driving after an append (see pg.rs); \
             no answer within {DEADLINE:?}"
        )
        .into()),
        Err(RecvTimeoutError::Disconnected) => match running.join() {
            Err(panicked) => panic::resume_unwind(panicked),
            Ok(()) => Err("the test's thread ended without an outcome".into()),
        },
    }
}

/// Before any database: a DSN that does not parse is refused as the closed `JournalUnavailable`,
/// which carries no part of the DSN (`AGENTS.md` rule 7).
fn a_dsn_that_does_not_parse_is_refused_closed() {
    let refused = PgNotices::connect("not a dsn").err();
    assert_eq!(refused, Some(DispatchError::JournalUnavailable));
}

/// The DSN of the test's own schema, acting as the journal's application role.
fn dsn(db: &support::TestDb) -> Result<String, Box<dyn Error>> {
    let url = std::env::var(support::URL_VAR)?;
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    Ok(format!("{url}{joiner}options={options}"))
}

fn stream(id: &str) -> Result<StreamId, Box<dyn Error>> {
    Ok(StreamId::parse(id).ok_or(id)?)
}

fn now() -> Result<UtcNanos, Box<dyn Error>> {
    Ok(UtcNanos::parse(AT)?)
}

/// The event id `n` with its last digit `last`: 0 for a mark, 1 for its alert.
fn id(n: usize, last: u8) -> String {
    format!("01J8Z3M4{n:017}{last}")
}

fn draft(id: &str, event_type: &str, causation: &str, payload: &str) -> Vec<u8> {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{id}","stream_id":"{A1}",
        "event_type":"{event_type}","schema_version":1,"event_time":"{AT}","clock_source":"local",
        "causation_id":{causation},"correlation_id":null,
        "actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"{BUILD}"}},
        "config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
    )
    .into_bytes()
}

/// `A1`'s next batch: its `StreamOpened` when `n` is 0, then a mark and its alert of `kind` per
/// `(n, kind)`. Every `kill_switch` alert carries out one owner command, `id(9, 0)`.
fn batch(first: bool, alerts: &[(usize, &str)]) -> Vec<Vec<u8>> {
    let mut drafts = Vec::new();
    if first {
        let opened = r#"{"stream_type":"account","workspace_id":"ws_1","broker":"alpaca","account_ref":"A1"}"#;
        drafts.push(draft(&id(0, 0), "StreamOpened", "null", opened));
    }
    for &(n, kind) in alerts {
        let command = if kind == "kill_switch" {
            format!("\"{}\"", id(9, 0))
        } else {
            "null".to_owned()
        };
        let cause = format!("\"{}\"", id(n, 0));
        let alert = format!(r#"{{"subject":{cause},"kind":"{kind}","owner_command":{command}}}"#);
        drafts.push(draft(&id(n, 0), "MarkUpdated", "null", MARK));
        drafts.push(draft(&id(n, 1), "OwnerAlertSent", &cause, &alert));
    }
    drafts
}

/// A deterministic source: each draw is 16 copies of the next byte, so two runs mint the same ids.
struct Counter(u8);

impl SecureRandom for Counter {
    fn fill(&mut self, bytes: &mut [u8; 16]) -> Result<(), NotifyError> {
        self.0 = self.0.wrapping_add(1);
        *bytes = [self.0; 16];
        Ok(())
    }
}

/// The in-memory journal as the dispatcher reads it: the reference side of the differential.
#[derive(Clone, Default)]
struct Memory(Rc<RefCell<MemoryJournal>>);

impl Journal for Memory {
    fn committed(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, DispatchError> {
        Ok(self.0.borrow().rows(stream).to_vec())
    }
    fn append_notices(
        &mut self,
        writer: &NoticeWriter,
        head: u64,
        at: UtcNanos,
        drafts: &[&[u8]],
    ) -> AppendOutcome {
        match (writer.stream(), writer.epoch()) {
            (Ok(stream), Ok(epoch)) => self.0.borrow_mut().append(stream, head, epoch, at, drafts),
            _ => AppendOutcome::Unavailable,
        }
    }
}

fn fixture() -> Result<FixtureProvider, NotifyError> {
    let mut vault = Vec::new();
    for (recipient, channel) in AUDIENCE {
        let recipient = Recipient::parse(recipient)?;
        vault.push((AddressHandle { recipient, channel }, "address".to_owned()));
    }
    FixtureProvider::holding(vault)
}

/// One step by `writer` over `A1` through `journal`.
fn one_step(
    journal: &mut dyn Journal,
    writer: &NoticeWriter,
    random: &mut Counter,
    sends: &mut FixtureProvider,
) -> Result<Result<(), DispatchError>, Box<dyn Error>> {
    let subjects = [stream(A1)?];
    let origin = Origin::parse("https://app.example.invalid")?;
    let config = Config {
        writer,
        subjects: &subjects,
        audience: &AUDIENCE,
        origin: &origin,
        environment: Environment::Paper,
        build: BUILD,
    };
    Ok(step(&config, journal, random, sends, now()?))
}

/// What a store's notice stream and a provider's captures hold: each record's type and bytes, and
/// each send's address, key, and message.
type Observed = (Vec<(String, Vec<u8>)>, Vec<(AddressHandle, String, String)>);

fn observed(rows: &[StoredEvent], sends: &FixtureProvider) -> Result<Observed, NotifyError> {
    let records = rows
        .iter()
        .map(|r| (r.event_type.clone(), r.body.clone()))
        .collect();
    let sent = sends
        .sent()?
        .iter()
        .map(|s| (s.address.clone(), s.key.clone(), s.message.clone()))
        .collect();
    Ok((records, sent))
}

/// One run of `batches` on both stores: each batch of alert kinds commits on `A1`, then one step
/// runs on each. The two notice streams hold the same records, byte for byte, and the two
/// providers the same sends.
fn differential(db: &support::TestDb, batches: &[Vec<usize>]) -> Checked {
    let (a1, ntf) = (stream(A1)?, stream(NTF)?);
    let memory = Memory::default();
    let pg: PgJournal = db.journal();
    let mut notices = PgNotices::connect(&dsn(db)?)?;
    let pg_writer = notices.take(ntf.clone())?;
    let memory_writer = {
        let epoch = memory.0.borrow_mut().take_ownership(&ntf);
        NoticeWriter::new(ntf.clone(), epoch)?
    };
    let memory_owner = memory.0.borrow_mut().take_ownership(&a1);
    let pg_owner = db.block_on(pg.take_ownership(&a1))?;
    let (mut memory_random, mut pg_random) = (Counter(0), Counter(0));
    let (mut memory_sends, mut pg_sends) = (fixture()?, fixture()?);
    let mut n = 0usize;
    for (i, kinds) in batches.iter().enumerate() {
        let alerts: Vec<(usize, &str)> = kinds
            .iter()
            .map(|&k| {
                n = n.wrapping_add(1);
                (n, KINDS.get(k).copied().unwrap_or("risk_limit"))
            })
            .collect();
        let drafts = batch(i == 0, &alerts);
        let drafts: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
        if !drafts.is_empty() {
            let head = memory.0.borrow().head(&a1).seq;
            let kept = memory
                .0
                .borrow_mut()
                .append(&a1, head, memory_owner, now()?, &drafts);
            let put = db.block_on(pg.append(&a1, head, pg_owner, now()?, &drafts))?;
            assert_eq!((kept.name(), put.name()), ("Committed", "Committed"));
        }
        let mut memory_journal = memory.clone();
        let on_memory = one_step(
            &mut memory_journal,
            &memory_writer,
            &mut memory_random,
            &mut memory_sends,
        )?;
        let on_pg = one_step(&mut notices, &pg_writer, &mut pg_random, &mut pg_sends)?;
        assert_eq!(on_pg, on_memory, "batch {i}: the same answer");
        let memory_rows = memory.0.borrow().rows(&ntf).to_vec();
        let pg_rows = db.block_on(pg.rows(&ntf))?;
        assert_eq!(
            observed(&pg_rows, &pg_sends)?,
            observed(&memory_rows, &memory_sends)?,
            "batch {i}: the same notice stream and the same sends"
        );
    }
    Ok(())
}

#[test]
fn the_same_causes_give_the_same_notice_stream_on_postgres_as_in_memory() -> Checked {
    a_dsn_that_does_not_parse_is_refused_closed();
    within_deadline(the_same_notice_stream)
}

fn the_same_notice_stream() -> Checked {
    let batches = prop::collection::vec(prop::collection::vec(0..KINDS.len(), 0..3), 1..4);
    let mut runner = TestRunner::new(Cases {
        cases: 8,
        ..Cases::default()
    });
    let checked = runner.run(&batches, |batches| {
        let Some(db) = support::TestDb::new() else {
            return Ok(());
        };
        differential(&db, &batches).map_err(|e| TestCaseError::fail(e.to_string()))
    });
    assert_eq!(checked, Ok(()));
    Ok(())
}

#[test]
fn a_second_dispatcher_on_postgres_fences_the_first_before_its_send() -> Checked {
    a_dsn_that_does_not_parse_is_refused_closed();
    within_deadline(the_second_dispatcher_fences_the_first)
}

fn the_second_dispatcher_fences_the_first() -> Checked {
    let Some(db) = support::TestDb::new() else {
        return Ok(());
    };
    let (a1, ntf) = (stream(A1)?, stream(NTF)?);
    let pg = db.journal();
    let mut older = PgNotices::connect(&dsn(&db)?)?;
    let mut newer = PgNotices::connect(&dsn(&db)?)?;
    let older_writer = older.take(ntf.clone())?;
    let newer_writer = newer.take(ntf.clone())?;
    let owner = db.block_on(pg.take_ownership(&a1))?;
    let drafts = batch(true, &[(1, "risk_limit")]);
    let drafts: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
    let put = db.block_on(pg.append(&a1, 0, owner, now()?, &drafts))?;
    assert_eq!(put.name(), "Committed");
    let mut sends = fixture()?;
    let fenced = one_step(&mut older, &older_writer, &mut Counter(0), &mut sends)?;
    assert_eq!(
        fenced,
        Err(DispatchError::Fenced {
            current_epoch: newer_writer.epoch()?
        })
    );
    assert_eq!(
        sends.sent()?.len(),
        0,
        "the fenced dispatcher sends nothing"
    );
    assert_eq!(db.block_on(pg.rows(&ntf))?.len(), 0, "and appends nothing");
    one_step(&mut newer, &newer_writer, &mut Counter(0), &mut sends)??;
    assert_eq!(
        sends.sent()?.len(),
        2,
        "the newer one sends on both channels"
    );
    Ok(())
}

#[test]
fn a_read_that_fails_on_postgres_is_journal_unavailable_and_sends_nothing() -> Checked {
    a_dsn_that_does_not_parse_is_refused_closed();
    within_deadline(a_failed_read_sends_nothing)
}

fn a_failed_read_sends_nothing() -> Checked {
    let Some(db) = support::TestDb::new() else {
        return Ok(());
    };
    let (a1, ntf) = (stream(A1)?, stream(NTF)?);
    let pg = db.journal();
    let mut notices = PgNotices::connect(&dsn(&db)?)?;
    let writer = notices.take(ntf.clone())?;
    let owner = db.block_on(pg.take_ownership(&a1))?;
    let drafts = batch(true, &[(1, "risk_limit")]);
    let drafts: Vec<&[u8]> = drafts.iter().map(Vec::as_slice).collect();
    let put = db.block_on(pg.append(&a1, 0, owner, now()?, &drafts))?;
    assert_eq!(put.name(), "Committed");
    db.admin_execute(&format!("REVOKE SELECT ON events FROM {APP_ROLE}"));
    assert_eq!(
        notices.committed(&ntf),
        Err(DispatchError::JournalUnavailable)
    );
    let mut sends = fixture()?;
    let stepped = one_step(&mut notices, &writer, &mut Counter(0), &mut sends)?;
    assert_eq!(stepped, Err(DispatchError::JournalUnavailable));
    assert_eq!(sends.sent()?.len(), 0, "a failed read sends nothing");
    db.admin_execute(&format!("GRANT SELECT ON events TO {APP_ROLE}"));
    assert_eq!(db.block_on(pg.rows(&ntf))?.len(), 0, "and appends nothing");
    Ok(())
}
