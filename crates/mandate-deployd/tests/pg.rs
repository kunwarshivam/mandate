//! The deployment manager's journal port over a real Postgres journal (E21-33 slice 2, DEC-1172):
//! a pass journals its record on the control stream before its effect, a retry is the same event,
//! and a record another control service appends in between moves no position. The suite's PG
//! class: it skips unless `MANDATE_PG_URL` is set, and fails without it where
//! `MANDATE_PG_REQUIRED` is (DEC-109, DEC-519). The journal is a local test schema and nothing here
//! carries a credential (`AGENTS.md` rules 7 and 8).

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
#[path = "../../mandate-deploy-manager/tests/common/mod.rs"]
mod harness;
#[path = "../../mandate-journal-pg/tests/support/mod.rs"]
mod support;

use mandate_canon::{Value, parse};
use mandate_deploy_manager::fake::Fake;
use mandate_deploy_manager::{ControlJournal, Deployer, Process, Slot};
use mandate_deployd::{PgControl, StderrAlerts};
use mandate_journal::{AppendOutcome, StreamId};
use mandate_journal_pg::{APP_ROLE, PgJournal};

/// The DSN of the test's own schema, acting as the journal's application role.
fn dsn(db: &support::TestDb) -> String {
    let url = std::env::var(support::URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    format!("{url}{joiner}options={options}")
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
}

fn control(dsn: &str) -> PgControl {
    PgControl::new(
        PgJournal::from_dsn(dsn).unwrap(),
        StreamId::parse(harness::STREAM).unwrap(),
    )
}

fn committed(outcome: &AppendOutcome) -> bool {
    matches!(
        outcome,
        AppendOutcome::Committed(_) | AppendOutcome::AlreadyCommitted(_)
    )
}

fn starts(rows: &[mandate_journal::StoredEvent], process: Process, subject: &str) -> usize {
    rows.iter()
        .filter_map(|row| parse(&row.body).ok())
        .filter(|body| body.get("event_type").and_then(Value::as_str) == Some("ProcessStartIssued"))
        .filter_map(|body| body.get("payload").cloned())
        .filter(|p| {
            p.get("process").and_then(Value::as_str) == Some(process.code())
                && p.get("subject").and_then(Value::as_str) == Some(subject)
        })
        .count()
}

#[test]
fn a_deployment_on_postgres_is_journaled_once_before_its_processes_start() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let dsn = dsn(&db);
    runtime().block_on(async {
        let mut services = control(&dsn);
        let account = "01J8Z2ACCT00000000000000A1";
        for draft in [
            harness::opened(),
            harness::established(&harness::connection_of(account), account),
            harness::deployed("agent_1", account),
        ] {
            let outcome = services.append(&[&draft], harness::at(0)).await.unwrap();
            assert!(committed(&outcome), "{outcome:?}");
        }
        let mut deployer = Deployer::new(
            harness::identity(),
            control(&dsn),
            Fake::new(),
            StderrAlerts::default(),
        );
        deployer.tick(harness::at(1)).await.unwrap();
        let rows = services.rows_after(0).await.unwrap();
        assert_eq!(starts(&rows, Process::Runtime, "agent_1"), 1);
        assert_eq!(starts(&rows, Process::Executor, account), 1);
        let runtime = Slot {
            process: Process::Runtime,
            subject: "agent_1".into(),
        };
        assert_eq!(
            deployer.orchestrator().live(&runtime),
            1,
            "the runtime starts once, after its record commits"
        );
        let head = rows.last().unwrap().seq;
        let later = services.rows_after(head).await.unwrap();
        assert!(
            later.is_empty(),
            "rows_after reads only rows after its position"
        );
    });
}

#[test]
fn a_restarted_manager_on_postgres_journals_no_second_start() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let dsn = dsn(&db);
    runtime().block_on(async {
        let mut services = control(&dsn);
        let account = "01J8Z2ACCT00000000000000A1";
        for draft in [
            harness::opened(),
            harness::established(&harness::connection_of(account), account),
            harness::deployed("agent_1", account),
        ] {
            services.append(&[&draft], harness::at(0)).await.unwrap();
        }
        let mut first = Deployer::new(
            harness::identity(),
            control(&dsn),
            Fake::new(),
            StderrAlerts::default(),
        );
        first.tick(harness::at(1)).await.unwrap();
        let (_, orchestrator, _) = first.into_ports();
        let mut second = Deployer::new(
            harness::identity(),
            control(&dsn),
            orchestrator,
            StderrAlerts::default(),
        );
        second.tick(harness::at(2)).await.unwrap();
        let rows = services.rows_after(0).await.unwrap();
        assert_eq!(starts(&rows, Process::Runtime, "agent_1"), 1);
        assert_eq!(starts(&rows, Process::Executor, account), 1);
    });
}
