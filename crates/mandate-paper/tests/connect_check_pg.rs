//! The connect check's journal ports over a real Postgres journal (DEC-946): `ProductionCheck`
//! takes the account stream's ownership at its head, appends the check's batch, and reads its
//! clock from the system; and its hardening is the binary's own. The suite's PG class: it skips
//! unless `MANDATE_PG_URL` is set (DEC-109). The drafts carry no credential and no account
//! number, and the journal is a local test schema (`AGENTS.md` rules 7 and 8).

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

use std::collections::BTreeMap;

use mandate_canon::{Value, parse};
use mandate_domain::Environment;
use mandate_journal::{AppendOutcome, StreamId, TrustedStart, verify_events};
use mandate_journal_pg::APP_ROLE;
use mandate_mcp::{ALLOWLIST, Listed};
use mandate_paper::{
    ACCOUNT_NUMBER_VAR, CheckArgs, CheckPorts, Observed, PaperError, ProductionCheck, SeenContract,
    connect_check,
};
use mandate_time::UtcNanos;
#[cfg(any(target_os = "linux", target_os = "android"))]
use rustix::process::{DumpableBehavior, dumpable_behavior};
use rustix::process::{Resource, getrlimit};

const ACCOUNT_REF: &str = "01J0R1CHECK0ACC0VNT000000A";
const STREAM: &str = "acct:ws_r1:01J0R1CHECK0ACC0VNT000000A";

/// The DSN of the test's own schema, acting as the journal's application role.
fn dsn(db: &support::TestDb) -> String {
    let url = std::env::var(support::URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    format!("{url}{joiner}options={options}")
}

/// The binary's journal and clock ports, with a passing read in place of the login.
struct Journaled(ProductionCheck);

impl CheckPorts for Journaled {
    fn harden(&mut self) -> Result<(), PaperError> {
        Ok(())
    }

    fn observe(&mut self, _account_number: &str) -> Result<Observed, PaperError> {
        Ok(Observed {
            contract: Some(SeenContract {
                listed: Listed {
                    names: ALLOWLIST.iter().map(|name| (*name).to_owned()).collect(),
                    allowlisted_present: true,
                },
                hash: "0".repeat(64),
            }),
            agentic_account_read: true,
        })
    }

    fn now(&self) -> UtcNanos {
        self.0.now()
    }

    fn own(&mut self, stream: &StreamId) -> Result<(u64, u64), PaperError> {
        self.0.own(stream)
    }

    fn append(
        &mut self,
        stream: &StreamId,
        head: u64,
        epoch: u64,
        at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> Result<AppendOutcome, PaperError> {
        self.0.append(stream, head, epoch, at, drafts)
    }
}

fn args(journal: String) -> CheckArgs {
    CheckArgs {
        workspace: "ws_r1".to_owned(),
        account_ref: ACCOUNT_REF.to_owned(),
        connection: "conn_rh_r1".to_owned(),
        journal,
        acknowledged: Environment::Live,
    }
}

/// Each ownership takes the next writer epoch at the stream's head, and the check's batch lands
/// at that head, stamped with the system clock, and verifies.
#[test]
fn the_binarys_journal_ports_own_the_stream_and_append_the_check() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let stream = StreamId::parse(STREAM).unwrap();
    let mut ports = ProductionCheck { journal: dsn(&db) };
    assert_eq!(ports.own(&stream).unwrap(), (1, 0));
    assert_eq!(ports.own(&stream).unwrap(), (2, 0));
    let before = ports.now();
    assert!(before > UtcNanos::parse("2026-01-01T00:00:00.000000000Z").unwrap());
    let vars = vec![(ACCOUNT_NUMBER_VAR.to_owned(), "5QRSENTINEL77".to_owned())];
    let mut journaled = Journaled(ProductionCheck { journal: dsn(&db) });
    let checked = connect_check(&args(dsn(&db)), &vars, &mut journaled).unwrap();
    let rows = db.block_on(db.journal().rows(&stream)).unwrap();
    let types: Vec<&str> = rows.iter().map(|row| row.event_type.as_str()).collect();
    assert_eq!(types, ["StreamOpened", "ConnectionChecked"]);
    assert_eq!(rows[1].event_id, checked.event_id);
    let body = parse(&rows[1].body).unwrap();
    let at = body.get("event_time").and_then(Value::as_str).unwrap();
    assert!(UtcNanos::parse(at).unwrap() >= before, "{at}");
    verify_events(&rows, TrustedStart::GENESIS, &BTreeMap::new()).unwrap();
    assert_eq!(ports.own(&stream).unwrap(), (4, 2));
}

/// An append the database cannot take is a journal refusal, never a check.
#[test]
fn an_append_at_a_stale_head_did_not_commit() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let stream = StreamId::parse(STREAM).unwrap();
    let mut ports = ProductionCheck { journal: dsn(&db) };
    let (epoch, _) = ports.own(&stream).unwrap();
    let outcome = ports.append(&stream, 7, epoch, ports.now(), &[b"{}".to_vec()]);
    assert!(
        !matches!(
            outcome,
            Ok(AppendOutcome::Committed(_) | AppendOutcome::AlreadyCommitted(_))
        ),
        "{outcome:?}"
    );
}

/// LT-9 through the connect check's own ports: `ProductionCheck::harden` turns the process's
/// core dumps off, read back from the kernel: a zero core-file limit, soft and hard, on every
/// unix target, and on Linux not dumpable either (DEC-954). Each test runs in its own process
/// under nextest.
#[test]
fn the_connect_checks_ports_harden_the_process() {
    let mut ports = ProductionCheck {
        journal: String::new(),
    };
    ports.harden().unwrap();
    let limit = getrlimit(Resource::Core);
    assert_eq!((limit.current, limit.maximum), (Some(0), Some(0)));
    #[cfg(any(target_os = "linux", target_os = "android"))]
    assert_eq!(dumpable_behavior().unwrap(), DumpableBehavior::NotDumpable);
}

/// The binary's ports never print the journal's DSN, which may hold a password (rule 7).
#[test]
fn the_connect_checks_ports_never_print_the_dsn() {
    let ports = ProductionCheck {
        journal: "postgres://paper:hunter2-sentinel@db-sentinel.invalid:1/journal".to_owned(),
    };
    let printed = format!("{ports:?}");
    assert!(printed.starts_with("ProductionCheck"), "{printed}");
    assert!(!printed.contains("hunter2"), "{printed}");
    assert!(!printed.contains("db-sentinel"), "{printed}");
}
