//! `mandate evaluation run`'s reads (DEC-2244 items 2 and 4): whether the workspace is an internal
//! paper one, and the thesis records of its agents' streams, each read from a journal seeded as
//! the control services and the agents' runtimes write it.

mod common;

use std::ffi::OsString;

use common::{AGENT, CONTROL, Journal, OTHER_AGENT, agent_stream, at, owner, stream};
use mandate_canon::parse;
use mandate_cli::control::{ControlError, ControlJournal};
use mandate_cli::evaluation::{internal_paper, internal_paper_setting, thesis_records};
use mandate_journal::{AppendOutcome, ArtifactRef};

const NOW: i64 = 1_790_000_000;
const VERSION: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const ENVELOPE: &str = r#"{"actor":{"build":null,"id":"@A","kind":"system","version":"0.1.0"},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":@C,"correlation_id":null,"envelope_version":1,"environment":"@E","event_id":"@I","event_time":"2026-10-08T13:00:00.000000000Z","event_type":"@T","payload":@P,"pii_refs":[],"schema_version":1,"stream_id":"@S"}"#;

fn refused(reason: &'static str) -> ControlError {
    ControlError::Refused { reason }
}

/// Appends one event to `on` as its writer would, in `environment`, with `config_refs`; each
/// stream's event ids are its own, since the journal's are unique across streams.
fn seed(
    journal: &mut Journal,
    on: &str,
    (event_type, payload): (&str, &str),
    config_refs: &str,
    environment: &str,
) {
    let target = stream(on);
    let head = journal.head(&target).unwrap().seq;
    let tag = if on == CONTROL {
        9
    } else if on == agent_stream(AGENT) {
        8
    } else {
        7
    };
    let draft = ENVELOPE
        .replace("@A", "control_services")
        .replace("@C", config_refs)
        .replace("@E", environment)
        .replace("@I", &format!("{tag}{head:025}"))
        .replace("@T", event_type)
        .replace("@P", payload)
        .replace("@S", on);
    let epoch = journal.take_ownership(&target).unwrap();
    let outcome = journal.append(&target, head, epoch, at(NOW).at, &[draft.as_bytes()]);
    assert!(matches!(outcome, Ok(AppendOutcome::Committed(_))));
}

fn control(journal: &mut Journal, event_type: &str, payload: &str) {
    seed(journal, CONTROL, (event_type, payload), "{}", "paper");
}

fn deployed(journal: &mut Journal, agent: &str) {
    let payload = format!(r#"{{"agent_id":"{agent}","mandate_version":"{VERSION}"}}"#);
    control(journal, "AgentDeployed", &payload);
}

fn agent_event(journal: &mut Journal, agent: &str, event_type: &str, thesis: &str) {
    let payload = format!(r#"{{"thesis_id":"{thesis}"}}"#);
    let refs = format!(r#"{{"mandate_version":"{VERSION}"}}"#);
    seed(
        journal,
        &agent_stream(agent),
        (event_type, &payload),
        &refs,
        "paper",
    );
}

/// A paper workspace with its control stream opened.
fn workspace() -> Journal {
    let mut journal = Journal::default();
    control(
        &mut journal,
        "StreamOpened",
        r#"{"stream_type":"control","workspace_id":"ws1"}"#,
    );
    journal
}

/// DEC-2244 item 4, as DEC-1143 item 1 reads the API's setting.
#[test]
#[ignore = "pending E17-8"]
fn the_setting_is_on_only_for_exactly_true() {
    assert_eq!(
        internal_paper_setting(Some(OsString::from("true"))),
        Ok(true)
    );
    for off in [None, Some(OsString::new()), Some(OsString::from("false"))] {
        assert_eq!(internal_paper_setting(off.clone()), Ok(false), "{off:?}");
    }
    for bad in ["TRUE", "1", "yes", " true", "true "] {
        assert_eq!(
            internal_paper_setting(Some(OsString::from(bad))),
            Err(refused("internal_paper_setting")),
            "{bad}"
        );
    }
}

/// The setting on and every control row paper: an internal paper workspace. The setting off, an
/// empty stream, or one live row anywhere: refused.
#[test]
#[ignore = "pending E17-8"]
fn only_a_paper_stream_with_the_setting_on_is_internal_paper() {
    let journal = workspace();
    assert_eq!(internal_paper(&journal, &owner(), true), Ok(()));
    assert_eq!(
        internal_paper(&journal, &owner(), false),
        Err(refused("not_internal_paper"))
    );
    assert_eq!(
        internal_paper(&Journal::default(), &owner(), true),
        Err(refused("not_internal_paper"))
    );
    let mut mixed = workspace();
    seed(
        &mut mixed,
        CONTROL,
        ("MemberActivated", r#"{"user":"u2"}"#),
        "{}",
        "live",
    );
    deployed(&mut mixed, AGENT);
    assert_eq!(
        internal_paper(&mixed, &owner(), true),
        Err(refused("not_internal_paper"))
    );
}

/// Every deployed agent's stream is read once, in agent id order, and only its thesis records,
/// in stream order, each with its position, hash, bound version and payload.
#[test]
#[ignore = "pending E17-8"]
fn every_deployed_agents_thesis_records_are_read_in_order() {
    let mut journal = workspace();
    deployed(&mut journal, OTHER_AGENT);
    deployed(&mut journal, AGENT);
    deployed(&mut journal, OTHER_AGENT);
    agent_event(&mut journal, OTHER_AGENT, "ThesisProposed", "th_b1");
    agent_event(&mut journal, AGENT, "ThesisProposed", "th_a1");
    agent_event(&mut journal, AGENT, "ModelOutputRecorded", "not_a_thesis");
    agent_event(&mut journal, AGENT, "ThesisRevised", "th_a2");
    agent_event(&mut journal, AGENT, "ThesisInvalidated", "th_a1");
    let records = thesis_records(&journal, &owner()).unwrap();
    let read: Vec<(String, u64)> = records
        .iter()
        .map(|r| (r.stream_id.clone(), r.seq))
        .collect();
    assert_eq!(
        read,
        [
            (agent_stream(AGENT), 1),
            (agent_stream(AGENT), 3),
            (agent_stream(OTHER_AGENT), 1),
        ]
    );
    let version = ArtifactRef::parse(VERSION).unwrap().digest();
    for record in &records {
        let stored = journal
            .rows(&stream(&record.stream_id))
            .unwrap()
            .into_iter()
            .find(|row| row.seq == record.seq)
            .unwrap();
        assert_eq!(record.hash, stored.hash);
        assert_eq!(record.mandate_version, version);
        let envelope = parse(&stored.body).unwrap();
        assert_eq!(Some(&record.payload), envelope.get("payload"));
    }
}

/// An agent stream the control stream names no deployment for is never read, however its records
/// read.
#[test]
#[ignore = "pending E17-8"]
fn an_undeployed_agents_stream_is_not_read() {
    let mut journal = workspace();
    deployed(&mut journal, AGENT);
    agent_event(&mut journal, AGENT, "ThesisProposed", "th_a1");
    agent_event(&mut journal, OTHER_AGENT, "ThesisProposed", "th_b1");
    let records = thesis_records(&journal, &owner()).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].stream_id, agent_stream(AGENT));
}

/// A thesis record that binds no mandate version cannot be checked, so it refuses the read; no
/// thesis record at all refuses too (rule 145 needs one stream or more).
#[test]
#[ignore = "pending E17-8"]
fn an_unbound_record_or_no_record_refuses() {
    let mut journal = workspace();
    deployed(&mut journal, AGENT);
    assert_eq!(
        thesis_records(&journal, &owner()),
        Err(refused("no_theses"))
    );
    agent_event(&mut journal, AGENT, "ModelOutputRecorded", "not_a_thesis");
    assert_eq!(
        thesis_records(&journal, &owner()),
        Err(refused("no_theses"))
    );
    seed(
        &mut journal,
        &agent_stream(AGENT),
        ("ThesisProposed", r#"{"thesis_id":"th_a1"}"#),
        "{}",
        "paper",
    );
    assert_eq!(
        thesis_records(&journal, &owner()),
        Err(refused("record_unbound"))
    );
}
