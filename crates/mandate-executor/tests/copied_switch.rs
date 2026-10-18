//! A kill switch that copies another stream's record (DEC-2003 item 4): the agent stream's
//! `KillSwitchActivated` an executor process applies. The account stream's own record names the
//! source as `copied_from`, so whether that source was applied is read back from this stream's
//! events, live and after a replay, and never from a mode another restriction may have set.

mod common;

use common::{FixedInstruments, FixedMandate, Shell, TestIds, agent, config, ports};
use mandate_canon::Value;
use mandate_executor::{Command, EventId, ExecutorState, Initiator, Input, KillScope, fold};

const SOURCE: &str = "01J8ZNB0R000000000000000K2";
const OTHER_SOURCE: &str = "01J8ZNB0R000000000000000K3";

/// A started executor on an opened stream, reconciled, as a production shell starts.
fn opened(ports: &mandate_executor::Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell
        .fold_one(&common::stream_opened())
        .expect("the stream opens");
    shell.restart_ready(ports)
}

fn switch(cause: Option<&str>) -> Input {
    Input::Command(Command::KillSwitch {
        scope: KillScope::Agent(agent(common::AGENT)),
        initiator: Initiator::Owner,
        confirmation: None,
        cause: cause.map(|id| EventId(id.to_owned())),
    })
}

/// A copied switch records its source, and the stream's own fold, live and replayed from the
/// account stream alone, reports exactly that source applied.
#[test]
fn a_copied_switch_records_its_source_and_the_fold_reads_it_back() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[common::FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = opened(&ports);
    let ran = shell.run(switch(Some(SOURCE)), &ports);
    let record = ran
        .draft("KillSwitchActivated")
        .expect("the switch is journaled");
    assert_eq!(
        record.payload.get("copied_from"),
        Some(&Value::Str(SOURCE.to_owned()))
    );
    let source = EventId(SOURCE.to_owned());
    let other = EventId(OTHER_SOURCE.to_owned());
    assert!(shell.state.applied_copy(&source));
    assert!(!shell.state.applied_copy(&other));

    let mut replayed = ExecutorState::new(common::scope());
    for event in &shell.account_journal {
        fold(&mut replayed, event).expect("the account stream replays");
    }
    assert!(replayed.applied_copy(&source));
    assert!(!replayed.applied_copy(&other));
}

/// A switch that copies nothing records `null`, and no source counts as applied by it.
#[test]
fn a_switch_that_copies_nothing_records_null() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[common::FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = opened(&ports);
    let ran = shell.run(switch(None), &ports);
    let record = ran
        .draft("KillSwitchActivated")
        .expect("the switch is journaled");
    assert_eq!(record.payload.get("copied_from"), Some(&Value::Null));
    assert!(!shell.state.applied_copy(&EventId(SOURCE.to_owned())));
}
