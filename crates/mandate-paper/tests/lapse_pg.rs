//! The live door's lapse over a real Postgres journal (DEC-933 item 3): `lapse_read` reads the
//! account stream's own rows, so a `FillApplied` on the run's account stream spends the record,
//! and a fill on another account's stream does not. The suite's PG class: it skips unless
//! `MANDATE_PG_URL` is set, as `tests/run.rs`'s placing test does (DEC-109). The drafts carry
//! no credential and the journal is a local test schema (`AGENTS.md` rules 7 and 8).

#[path = "../../mandate-journal/tests/common/mod.rs"]
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

use common::{T, edit, mark_draft, now, opened_draft};
use mandate_domain::Environment;
use mandate_journal::{AppendOutcome, StreamId};
use mandate_journal_pg::{APP_ROLE, PgJournal};
use mandate_num::Usd;
use mandate_paper::{Broker, Connection, LapseRefusal, PaperError, lapse_read};
use mandate_robinhood::AccountSnapshot;

/// The account the journal fixtures' stream binds: `acct:ws_1:ACCT1`.
const RECORDED: &str = "ACCT1";

fn live(account_ref: &str) -> Connection {
    Connection {
        broker: Broker::Robinhood,
        environment: Environment::Live,
        account_ref: account_ref.to_owned(),
    }
}

fn flat() -> AccountSnapshot {
    AccountSnapshot {
        cash: Usd::parse("1000").unwrap(),
        buying_power: Usd::parse("1000").unwrap(),
        positions: Vec::new(),
        open_orders: Vec::new(),
    }
}

/// The DSN of the test's own schema, acting as the journal's application role.
fn dsn(db: &support::TestDb) -> String {
    let url = std::env::var(support::URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    format!("{url}{joiner}options={options}")
}

/// A `FillApplied` on the fixture stream, event `n`, of an order that holds nothing else.
fn fill(n: u64) -> Vec<u8> {
    let fill = format!(
        r#"{{"fill_id":"F-{n}","client_order_id":"C-{n}","instrument_id":"inst","side":"buy","qty_gross":"1","price":"10","trade_date":"2026-09-21","risk_clock":"{T}","fees":[]}}"#
    );
    let hash = "4".repeat(64);
    let refs = format!(
        r#"{{"fee_config":"sha256:{hash}","instrument_snapshot":"sha256:{hash}","settlement_calendar":"sha256:{hash}","trading_calendar":"sha256:{hash}"}}"#
    );
    let typed = edit(&mark_draft(n, "1"), "event_type", Some(r#""FillApplied""#));
    let referred = edit(&typed, "config_refs", Some(&refs));
    edit(&referred, "payload", Some(&fill))
}

fn committed<E: std::fmt::Debug>(outcome: Result<AppendOutcome, E>) {
    assert!(
        matches!(outcome, Ok(AppendOutcome::Committed(_))),
        "{outcome:?}"
    );
}

/// The fixture stream opened, with one mark, and then `fill` when `filled`.
fn journaled(db: &support::TestDb, journal: &PgJournal, filled: bool) -> StreamId {
    let stream = common::stream();
    let epoch = db.block_on(journal.take_ownership(&stream)).unwrap();
    let drafts: Vec<Vec<u8>> = [opened_draft("paper"), mark_draft(1, "1")]
        .into_iter()
        .chain(filled.then(|| fill(2)))
        .collect();
    for (seq, draft) in (0_u64..).zip(&drafts) {
        committed(db.block_on(journal.append(&stream, seq, epoch, now(), &[draft])));
    }
    stream
}

/// A fill on the run's own account stream spends the record, read through the run's journal.
#[test]
fn a_fill_on_the_journaled_account_stream_spends_the_record() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let stream = journaled(&db, &db.journal(), true);
    let refused = lapse_read(&live(RECORDED), Some(&dsn(&db)), &stream, Some(&flat()));
    assert!(
        matches!(refused, Err(PaperError::Lapse(LapseRefusal::Spent))),
        "{refused:?}"
    );
}

/// The same stream without the fill leaves the record unspent: the rows are read, not assumed.
#[test]
fn a_journaled_stream_with_no_fill_leaves_the_record_unspent() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let stream = journaled(&db, &db.journal(), false);
    let started = lapse_read(&live(RECORDED), Some(&dsn(&db)), &stream, Some(&flat()));
    assert!(matches!(started, Ok(())), "{started:?}");
}

/// A fill on another account's stream spends nothing for this run's record: the run reads its
/// own account stream, which holds no fill.
#[test]
fn a_fill_on_another_accounts_stream_spends_nothing() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    journaled(&db, &db.journal(), true);
    let other = StreamId::parse("acct:ws_1:ACCT2").unwrap();
    let started = lapse_read(&live("ACCT2"), Some(&dsn(&db)), &other, Some(&flat()));
    assert!(matches!(started, Ok(())), "{started:?}");
}
