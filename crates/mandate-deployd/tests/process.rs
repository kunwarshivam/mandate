//! The process's wiring around the manager (E21-33 slice 2, DEC-1172): how a pass moves the
//! lifecycle, the chart's pod templates, and the alerts.

#[path = "../../mandate-deploy-manager/tests/common/mod.rs"]
mod harness;

use std::path::PathBuf;

use mandate_deploy_manager::orchestrator::CRASH_LOOP;
use mandate_deploy_manager::{Alerts, ControlJournal, Halt, Process};
use mandate_deployd::{MemoryControl, StderrAlerts, TemplateError, input, templates};
use mandate_journal::{AppendOutcome, StreamId};
use mandate_process::{Input, Kind, Lifecycle, State};
use mandate_time::UtcNanos;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mandate-deployd-{name}-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_pass_that_could_not_finish_is_a_heartbeat_and_a_refused_record_stops_the_process() {
    assert_eq!(input(&Ok(())), Input::Heartbeat);
    for halt in [Halt::Journal, Halt::Orchestrator, Halt::NotOpened] {
        assert_eq!(input(&Err(halt)), Input::Heartbeat);
    }
    assert_eq!(
        input(&Err(Halt::Refused {
            event_type: "ProcessStartIssued"
        })),
        Input::Failed
    );
}

#[test]
fn a_refused_record_after_start_exits_the_process_failed() {
    let at = UtcNanos::EPOCH;
    let mut lifecycle = Lifecycle::new(Kind::Service, std::time::Duration::from_secs(30), at);
    lifecycle.observe(at, Input::StartupPassed);
    assert_eq!(lifecycle.state(), State::Ready);
    lifecycle.observe(at, input(&Err(Halt::Journal)));
    assert_eq!(lifecycle.state(), State::Ready);
    lifecycle.observe(
        at,
        input(&Err(Halt::Refused {
            event_type: "ProcessStartIssued",
        })),
    );
    assert!(matches!(lifecycle.state(), State::Exited(_)));
}

#[test]
fn a_process_without_a_template_file_has_none_and_one_present_is_read() {
    let dir = dir("templates");
    assert!(templates(&dir).unwrap().is_empty());
    std::fs::write(
        dir.join("runtime.json"),
        r#"{"metadata":{"labels":{}},"spec":{"containers":[{"name":"runtime"}]}}"#,
    )
    .unwrap();
    let found = templates(&dir).unwrap();
    assert_eq!(
        found.keys().copied().collect::<Vec<_>>(),
        [Process::Runtime]
    );
    assert!(found[&Process::Runtime]["spec"]["containers"].is_array());
}

#[test]
fn a_template_that_is_not_a_json_object_refuses_the_start() {
    let dir = dir("bad");
    std::fs::write(dir.join("executor.json"), "[]").unwrap();
    assert_eq!(
        templates(&dir),
        Err(TemplateError::Unreadable(Process::Executor))
    );
    assert_eq!(
        TemplateError::Unreadable(Process::Executor).code(),
        "executor_template_unreadable"
    );
    std::fs::write(dir.join("executor.json"), "not json").unwrap();
    assert_eq!(
        templates(&dir),
        Err(TemplateError::Unreadable(Process::Executor))
    );
    std::fs::remove_file(dir.join("executor.json")).unwrap();
    std::fs::write(dir.join("runtime.json"), "{").unwrap();
    assert_eq!(
        templates(&dir),
        Err(TemplateError::Unreadable(Process::Runtime))
    );
    assert_eq!(
        TemplateError::Unreadable(Process::Runtime).code(),
        "runtime_template_unreadable"
    );
}

#[test]
fn an_alert_is_raised_with_its_code_and_runbook_only() {
    let mut alerts = StderrAlerts::default();
    alerts.raise(CRASH_LOOP);
    assert_eq!(alerts.raised, [CRASH_LOOP]);
}

#[test]
fn the_fixtures_control_stream_appends_at_its_head_and_reads_after_a_position() {
    let mut control = MemoryControl::new(StreamId::parse(harness::STREAM).unwrap());
    harness::block_on(async {
        assert!(control.rows_after(0).await.unwrap().is_empty());
        let opened = harness::opened();
        let first = control.append(&opened, harness::at(0)).await.unwrap();
        assert!(matches!(first, AppendOutcome::Committed(_)), "{first:?}");
        let account = "01J8Z2ACCT00000000000000A1";
        let established = harness::established(&harness::connection_of(account), account);
        let second = control.append(&established, harness::at(1)).await.unwrap();
        assert!(matches!(second, AppendOutcome::Committed(_)), "{second:?}");
        let again = control.append(&opened, harness::at(2)).await.unwrap();
        assert!(
            matches!(again, AppendOutcome::AlreadyCommitted(_)),
            "{again:?}"
        );
        let rows = control.rows_after(0).await.unwrap();
        assert_eq!(rows.iter().map(|r| r.seq).collect::<Vec<_>>(), [1, 2]);
        let after = control.rows_after(1).await.unwrap();
        assert_eq!(after.iter().map(|r| r.seq).collect::<Vec<_>>(), [2]);
    });
}
