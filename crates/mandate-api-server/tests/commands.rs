//! The owner commands' events (§3.11 item 10, DEC-1007): the members journal spec §9.11 lists, a
//! draft the journal itself accepts, the repeat's comparison, the scope's lookup, and the append
//! loop's answers under each failure the store can give.

mod common;

use common::{MemJournal, Scripted, WORKSPACE};
use mandate_api_server::commands::{
    ATTEMPTS, Command, Issued, KillScope, Settled, draft, in_workspace, same, settle,
};
use mandate_authn::Request;
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_identity::Permission;
use mandate_journal::{AppendOutcome, MemoryJournal, StoredEvent, StreamId};
use mandate_time::UtcNanos;

const PRINCIPAL: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const EVENT: &str = "01JBH3BTZ8K2M4N6P8R0S2T9ZZ";
const T0: i64 = 1_791_000_000;

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a time")
}

fn ctl() -> String {
    format!("ctl:{WORKSPACE}")
}

fn issued<'a>(command: &'a Command, event_id: &'a str, now: UtcNanos) -> Issued<'a> {
    Issued {
        workspace: WORKSPACE,
        principal: PRINCIPAL,
        event_id,
        environment: "paper",
        now,
        command,
    }
}

fn agent(id: &str) -> Command {
    Command::KillSwitch(KillScope::Agent(id.to_owned()))
}

fn every() -> Vec<Command> {
    vec![
        Command::Pause("agt_a".to_owned()),
        Command::Hold("agt_a".to_owned()),
        agent("agt_a"),
        Command::KillSwitch(KillScope::Connection("conn_1".to_owned())),
        Command::KillSwitch(KillScope::Workspace),
    ]
}

#[test]
fn each_command_names_its_operation_row_and_session_reach() {
    let named: Vec<(&str, Permission, Request)> = every()
        .iter()
        .map(|c| (c.operation(), c.permission(), c.request()))
        .collect();
    assert_eq!(
        named,
        [
            ("pause", Permission::Pause, Request::Pause),
            ("hold_openings", Permission::HoldNewOpenings, Request::Other),
            (
                "kill_switch",
                Permission::KillSwitchAgent,
                Request::KillSwitch
            ),
            (
                "kill_switch",
                Permission::KillSwitchConnectionOrWorkspace,
                Request::KillSwitch
            ),
            (
                "kill_switch",
                Permission::KillSwitchConnectionOrWorkspace,
                Request::KillSwitch
            ),
        ]
    );
}

/// The payload §9.11 lists, written out by hand for each command.
fn expected_payload(command: &Command) -> Value {
    let (agent, name, scope, subject) = match command {
        Command::Pause(a) => (format!("\"{a}\""), "pause", "agent", a.as_str()),
        Command::Hold(a) => (format!("\"{a}\""), "hold_openings", "agent", a.as_str()),
        Command::KillSwitch(KillScope::Agent(a)) => {
            (format!("\"{a}\""), "kill_switch", "agent", a.as_str())
        }
        Command::KillSwitch(KillScope::Connection(c)) => {
            ("null".to_owned(), "kill_switch", "connection", c.as_str())
        }
        Command::KillSwitch(KillScope::Workspace) => {
            ("null".to_owned(), "kill_switch", "workspace", WORKSPACE)
        }
    };
    parse(
        format!(
            r#"{{"agent":{agent},"command":"{name}","scope":"{scope}","subject":"{subject}","release":null,"warning_shown":null,"bid":null,"bid_size":null,"floor":null,"user":"{PRINCIPAL}","submitted_at":{T0},"step_up":null}}"#
        )
        .as_bytes(),
    )
    .expect("JSON")
}

#[test]
fn the_draft_is_the_clis_owner_command_with_the_user_as_actor() {
    for command in every() {
        let bytes = draft(&issued(&command, EVENT, at(T0))).expect("a draft");
        let value = parse(&bytes).expect("JSON");
        assert_eq!(to_canonical(&value), bytes, "{command:?} is canonical");
        assert_eq!(
            value.get("payload"),
            Some(&expected_payload(&command)),
            "{command:?}"
        );
        let field = |k: &str| value.get(k).cloned().unwrap_or(Value::Null);
        assert_eq!(field("event_type"), Value::Str("OwnerCommandIssued".into()));
        assert_eq!(field("event_id"), Value::Str(EVENT.into()));
        assert_eq!(field("stream_id"), Value::Str(ctl()));
        assert_eq!(field("environment"), Value::Str("paper".into()));
        assert_eq!(field("event_time"), Value::Str(at(T0).to_string()));
        let actor = field("actor");
        assert_eq!(actor.get("kind"), Some(&Value::Str("user".into())));
        assert_eq!(actor.get("id"), Some(&Value::Str(PRINCIPAL.into())));
    }
}

/// A control stream the journal itself opened, as the CLI's opener does.
fn opened() -> (MemoryJournal, StreamId) {
    let mut journal = MemoryJournal::new();
    let stream = StreamId::parse(&ctl()).expect("a stream id");
    let opening = format!(
        r#"{{"actor":{{"build":"sha256:{zero}","id":"control_services","kind":"system","version":"0.1.0"}},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":{{}},"correlation_id":null,"environment":"paper","envelope_version":1,"event_id":"01JBH3BTZ8K2M4N6P8R0S2T001","event_time":"{t}","event_type":"StreamOpened","payload":{{"stream_type":"control","workspace_id":"{WORKSPACE}"}},"pii_refs":[],"schema_version":1,"stream_id":"{s}"}}"#,
        zero = "0".repeat(64),
        t = at(T0 - 10),
        s = ctl(),
    );
    let bytes = to_canonical(&parse(opening.as_bytes()).expect("JSON"));
    let epoch = journal.take_ownership(&stream);
    let outcome = journal.append(&stream, 0, epoch, at(T0 - 10), &[&bytes]);
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
    (journal, stream)
}

#[test]
fn the_journal_accepts_each_draft() {
    for (n, command) in every().into_iter().enumerate() {
        let (mut journal, stream) = opened();
        let id = format!("01JBH3BTZ8K2M4N6P8R0S2T1{n:02}");
        let bytes = draft(&issued(&command, &id, at(T0))).expect("a draft");
        let epoch = journal.take_ownership(&stream);
        let outcome = journal.append(&stream, 1, epoch, at(T0), &[&bytes]);
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "{command:?}: {outcome:?}"
        );
    }
}

fn stored(bytes: &[u8]) -> StoredEvent {
    let value = parse(bytes).expect("JSON");
    let text = |k: &str| {
        value
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    StoredEvent {
        stream_id: text("stream_id"),
        seq: 2,
        event_id: text("event_id"),
        event_type: text("event_type"),
        schema_version: 1,
        environment: text("environment"),
        recorded_at: at(T0).to_string(),
        prev_hash: Digest::ZERO,
        hash: Digest::of(bytes),
        body: bytes.to_vec(),
    }
}

/// `bytes` with `edit` applied to its parsed value, re-encoded.
fn edited(bytes: &[u8], edit: impl FnOnce(&mut mandate_canon::Object)) -> Vec<u8> {
    let Value::Object(mut fields) = parse(bytes).expect("JSON") else {
        panic!("an object");
    };
    edit(&mut fields);
    to_canonical(&Value::Object(fields))
}

fn set(fields: &mut mandate_canon::Object, path: &[&str], value: Value) {
    let (last, parents) = path.split_last().expect("a path");
    let mut at = fields;
    for parent in parents {
        let Some(Value::Object(inner)) = at.get_mut(*parent) else {
            panic!("{parent}");
        };
        at = inner;
    }
    at.insert(mandate_canon::Key::new(last).expect("a key"), value);
}

#[test]
fn a_repeat_is_the_same_command_whatever_its_second_time_or_build() {
    let command = Command::Pause("agt_a".to_owned());
    let first = draft(&issued(&command, EVENT, at(T0))).expect("a draft");
    let later = draft(&issued(&command, EVENT, at(T0 + 90))).expect("a draft");
    assert!(same(&stored(&first), &later));
    let rebuilt = edited(&first, |f| {
        set(
            f,
            &["actor", "build"],
            Value::Str(format!("sha256:{}", "1".repeat(64))),
        );
        set(f, &["actor", "version"], Value::Str("9.9.9".into()));
        set(f, &["environment"], Value::Str("live".into()));
    });
    assert!(same(&stored(&first), &rebuilt));
}

#[test]
fn a_repeat_that_changes_a_journaled_member_is_not_the_same() {
    let command = agent("agt_a");
    let first = draft(&issued(&command, EVENT, at(T0))).expect("a draft");
    let text = |s: &str| Value::Str(s.to_owned());
    let changes: Vec<(&[&str], Value)> = vec![
        (&["event_type"], text("OwnerAcknowledged")),
        (&["stream_id"], text("ctl:01JB3K6F0C9R2V7N4M8Q1T5W40")),
        (&["actor", "id"], text("01JB3K6F0C9R2V7N4M8Q1T5W41")),
        (&["actor", "kind"], text("client")),
        (&["payload", "scope"], text("workspace")),
        (&["payload", "subject"], text("agt_b")),
        (&["payload", "agent"], Value::Null),
        (&["payload", "user"], text("01JB3K6F0C9R2V7N4M8Q1T5W41")),
        (&["payload", "command"], text("pause")),
        (&["payload", "step_up"], text("x")),
    ];
    for (path, value) in changes {
        let other = edited(&first, |f| set(f, path, value.clone()));
        assert!(!same(&stored(&first), &other), "{path:?}");
    }
    assert!(!same(&stored(&first), b"not json"));
    let mut unparsable = stored(&first);
    unparsable.body = b"{".to_vec();
    assert!(!same(&unparsable, &first));
}

fn row(seq: u64, event_type: &str, payload: &str) -> StoredEvent {
    let text = format!(
        r#"{{"environment":"paper","event_id":"01JBH3BTZ8K2M4N6P8R0S2T{seq:03}","event_type":"{event_type}","payload":{payload},"stream_id":"{}"}}"#,
        ctl()
    );
    let mut row = stored(&to_canonical(&parse(text.as_bytes()).expect("JSON")));
    row.seq = seq;
    row
}

#[test]
fn a_scope_is_found_only_where_the_control_stream_names_it() {
    let control = vec![
        row(1, "StreamOpened", "{}"),
        row(
            2,
            "AgentDeployed",
            r#"{"agent_id":"agt_a","mandate_version":"sha256:01"}"#,
        ),
        row(3, "ConnectionRequested", r#"{"connection_id":"conn_1"}"#),
        row(
            4,
            "MandateDraftSaved",
            r#"{"agent_id":"agt_c","connection_id":7,"mandate_version":"sha256:01"}"#,
        ),
    ];
    let found = |c: &Command| in_workspace(&control, c);
    assert!(found(&Command::Pause("agt_a".to_owned())));
    assert!(found(&Command::Hold("agt_a".to_owned())));
    assert!(found(&agent("agt_a")));
    assert!(found(&Command::KillSwitch(KillScope::Connection(
        "conn_1".to_owned()
    ))));
    assert!(found(&Command::KillSwitch(KillScope::Workspace)));
    assert!(!found(&Command::Pause("agt_c".to_owned())));
    assert!(!found(&Command::Hold("agt_b".to_owned())));
    assert!(!found(&agent("conn_1")));
    assert!(!found(&Command::KillSwitch(KillScope::Connection(
        "agt_a".to_owned()
    ))));
    assert!(!in_workspace(
        &[],
        &Command::KillSwitch(KillScope::Workspace)
    ));
}

fn settled(journal: &MemJournal, bytes: &[u8]) -> Settled {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    runtime.block_on(settle(journal, &ctl(), EVENT, bytes, at(T0)))
}

fn pause() -> Vec<u8> {
    draft(&issued(&Command::Pause("agt_a".to_owned()), EVENT, at(T0))).expect("a draft")
}

#[test]
fn an_append_settles_at_the_head_under_a_fresh_epoch() {
    let journal = MemJournal::default();
    journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(ctl(), vec![row(1, "StreamOpened", "{}")]);
    *journal.epochs.lock().expect("unpoisoned") = [(ctl(), 6)].into();
    let bytes = pause();
    let Settled::Stored(event) = settled(&journal, &bytes) else {
        panic!("stored");
    };
    assert_eq!((event.seq, event.event_id.as_str()), (2, EVENT));
    assert_eq!(event.body, bytes);
    assert_eq!(
        *journal.appends.lock().expect("unpoisoned"),
        [(ctl(), 1, 7)]
    );
}

#[test]
fn a_fence_a_moved_head_or_an_outage_is_retried_with_the_same_bytes() {
    let journal = MemJournal::default();
    journal.script(&[
        Scripted::Fenced,
        Scripted::HeadMismatch,
        Scripted::Unavailable,
        Scripted::Failed,
    ]);
    let bytes = pause();
    let Settled::Stored(event) = settled(&journal, &bytes) else {
        panic!("stored on the last attempt");
    };
    assert_eq!(event.body, bytes);
    let epochs: Vec<u64> = journal
        .appends
        .lock()
        .expect("unpoisoned")
        .iter()
        .map(|a| a.2)
        .collect();
    assert_eq!(epochs, [1, 2, 3, 4, 5]);
}

#[test]
fn attempts_that_never_settle_store_nothing_and_say_so() {
    let journal = MemJournal::default();
    journal.script(&[Scripted::Unavailable; ATTEMPTS]);
    assert_eq!(settled(&journal, &pause()), Settled::Unavailable);
    assert_eq!(journal.appends.lock().expect("unpoisoned").len(), ATTEMPTS);
    assert!(journal.rows_of(&ctl()).is_empty());
}

#[test]
fn an_ambiguous_commit_no_later_attempt_confirms_is_unknown() {
    let journal = MemJournal::default();
    let mut steps = vec![Scripted::Ambiguous];
    steps.extend([Scripted::HeadMismatch; ATTEMPTS - 1]);
    journal.script(&steps);
    assert_eq!(settled(&journal, &pause()), Settled::Unknown);
}

#[test]
fn an_ambiguous_commit_that_landed_is_found_by_the_retry() {
    let journal = MemJournal::default();
    journal.script(&[Scripted::CommittedThenAmbiguous]);
    let Settled::Stored(event) = settled(&journal, &pause()) else {
        panic!("stored");
    };
    assert_eq!(event.seq, 1);
    assert_eq!(journal.rows_of(&ctl()).len(), 1);
}

#[test]
fn another_event_under_the_id_is_a_conflict_at_once() {
    let journal = MemJournal::default();
    journal.script(&[Scripted::Conflict]);
    assert_eq!(settled(&journal, &pause()), Settled::Conflict);
    assert_eq!(journal.appends.lock().expect("unpoisoned").len(), 1);
}

#[test]
fn a_committed_answer_without_the_event_is_not_taken_as_stored() {
    let journal = MemJournal::default();
    let other = draft(&issued(
        &Command::Pause("agt_a".to_owned()),
        "01JBH3BTZ8K2M4N6P8R0S2T8ZZ",
        at(T0),
    ))
    .expect("a draft");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let answer = runtime.block_on(settle(&journal, &ctl(), EVENT, &other, at(T0)));
    assert_eq!(answer, Settled::Unknown);
}
