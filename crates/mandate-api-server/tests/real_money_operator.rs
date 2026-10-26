//! The platform operator's setting of the customer real-money gate ([DEC-2134] items 3 and 6;
//! journal spec v0.88 §9.35): off always, on only for the one workspace
//! `MANDATE_API_FOUNDER_WORKSPACE` names and only with a decision, written as a `platform_operator`
//! that the journal's own validator admits, and read back by the gate. Every expectation is
//! computed here from DEC-2134's words, not by the module's code.
//!
//! [DEC-2134]: ../../../docs/project/decisions/DEC-2134.md

mod common;

use common::{MemJournal, WORKSPACE};
use mandate_api_server::operator::{
    Command, Refusal, Requested, Setting, admitted, command, drafted, set,
};
use mandate_api_server::real_money::{self, Setting as Gate};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::{Draft, StoredEvent};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const OTHER: &str = "01JB3K6F0C9R2V7N4M8Q1T5W40";
const OPERATOR: &str = "01JAA3G0A000000000000000P1";
const KEY: &str = "k1Z-9_operator01";
const KEY_2: &str = "k2Z-9_operator02";

fn on(decision: &str) -> Requested {
    Requested::On {
        decision: decision.to_owned(),
    }
}

fn now() -> UtcNanos {
    UtcNanos::from_parts(1_792_000_000, 0).expect("a time")
}

/// The founder settings DEC-2134 item 6 names, and the answer it gives an on request for
/// [`WORKSPACE`] with each.
fn founder_cases() -> Vec<(Option<String>, Result<(), Refusal>)> {
    vec![
        (None, Err(Refusal::FounderWorkspaceUnset)),
        (Some(String::new()), Err(Refusal::FounderWorkspaceUnset)),
        (
            Some(" , \t".to_owned()),
            Err(Refusal::FounderWorkspaceUnset),
        ),
        (Some(WORKSPACE.to_owned()), Ok(())),
        (Some(format!(" {WORKSPACE}\n")), Ok(())),
        (
            Some(OTHER.to_owned()),
            Err(Refusal::NotTheFoundersWorkspace),
        ),
        (
            Some(WORKSPACE.to_lowercase()),
            Err(Refusal::NotTheFoundersWorkspace),
        ),
        (
            Some(format!("{WORKSPACE},{OTHER}")),
            Err(Refusal::FounderWorkspaceAmbiguous),
        ),
        (
            Some(format!("{WORKSPACE} {WORKSPACE}")),
            Err(Refusal::FounderWorkspaceAmbiguous),
        ),
        (
            Some(format!("{OTHER},{WORKSPACE}")),
            Err(Refusal::FounderWorkspaceAmbiguous),
        ),
    ]
}

#[test]
fn off_is_always_allowed_and_on_only_for_the_one_founder_workspace() {
    for (founder, want) in founder_cases() {
        assert_eq!(
            admitted(&Requested::Off, WORKSPACE, founder.as_deref()),
            Ok(()),
            "off with {founder:?}"
        );
        assert_eq!(
            admitted(&on("DEC-2135"), WORKSPACE, founder.as_deref()),
            want,
            "on with {founder:?}"
        );
    }
}

#[test]
fn on_cites_a_decision_id() {
    for good in ["DEC-1", "DEC-2135", "DEC-99999"] {
        assert_eq!(
            admitted(&on(good), WORKSPACE, Some(WORKSPACE)),
            Ok(()),
            "{good}"
        );
    }
    for bad in [
        "",
        "DEC-",
        "DEC-0",
        "DEC-02135",
        "dec-2135",
        "DEC2135",
        "DEC-123456",
        "DEC-2135 ",
        "DEC-21a5",
    ] {
        assert_eq!(
            admitted(&on(bad), WORKSPACE, Some(WORKSPACE)),
            Err(Refusal::Decision),
            "{bad:?}"
        );
    }
    assert_eq!(
        admitted(&on("bad"), WORKSPACE, Some(OTHER)),
        Err(Refusal::NotTheFoundersWorkspace),
        "the founder setting is judged before the decision"
    );
}

proptest! {
    /// Over random founder settings: on is admitted exactly when the setting's non-empty names,
    /// split at commas and whitespace, are the one hosted workspace.
    #[test]
    fn on_is_admitted_exactly_for_the_one_named_workspace(
        names in prop::collection::vec(prop::sample::select(vec![WORKSPACE, OTHER, ""]), 0..4),
        separator in prop::sample::select(vec![",", " ", "\t", ", "]),
    ) {
        let founder = names.join(separator);
        let named: Vec<&str> = names.iter().copied().filter(|n| !n.is_empty()).collect();
        let allowed = named == [WORKSPACE];
        let got = admitted(&on("DEC-2135"), WORKSPACE, Some(&founder));
        prop_assert_eq!(got.is_ok(), allowed, "{:?}: {:?}", founder, got);
    }
}

/// The drafted setting is one the journal's own validator admits, written by the operator as
/// itself, and the gate reads it as the setting it names.
#[test]
fn the_drafted_setting_is_a_valid_operators_record_the_gate_reads() {
    for (requested, enabled, decision) in [
        (on("DEC-2135"), true, Value::Str("DEC-2135".into())),
        (Requested::Off, false, Value::Null),
    ] {
        let setting = Setting {
            workspace: WORKSPACE,
            operator: OPERATOR,
            key: KEY,
            requested,
        };
        let (id, draft) = drafted(&setting, "paper", now()).expect("drafted");
        let parsed = Draft::parse(&draft).expect("the journal admits it");
        assert_eq!(parsed.event_type(), "RealMoneyGateChanged");
        assert_eq!(parsed.event_id(), id);
        let body = parse(&draft).expect("JSON");
        let member = |path: &[&str]| path.iter().try_fold(&body, |v, k| v.get(k)).cloned();
        assert_eq!(
            member(&["actor", "kind"]),
            Some(Value::Str("platform_operator".into()))
        );
        assert_eq!(member(&["actor", "id"]), Some(Value::Str(OPERATOR.into())));
        assert_eq!(member(&["payload", "enabled"]), Some(Value::Bool(enabled)));
        assert_eq!(member(&["payload", "decision"]), Some(decision));
        assert_eq!(
            member(&["stream_id"]),
            Some(Value::Str(format!("ctl:{WORKSPACE}")))
        );
        let row = stored(1, &id, &draft);
        let want = if enabled { Gate::On } else { Gate::Off };
        assert_eq!(real_money::setting(&[row]), want);
    }
}

#[test]
fn a_malformed_operator_workspace_or_key_drafts_nothing() {
    for (workspace, operator, key) in [
        ("not-a-ulid", OPERATOR, KEY),
        (WORKSPACE, "", KEY),
        (WORKSPACE, OPERATOR, "short"),
    ] {
        let setting = Setting {
            workspace,
            operator,
            key,
            requested: Requested::Off,
        };
        assert_eq!(
            drafted(&setting, "paper", now()),
            None,
            "{workspace} {operator} {key}"
        );
    }
}

fn stored(seq: u64, id: &str, body: &[u8]) -> StoredEvent {
    StoredEvent {
        stream_id: format!("ctl:{WORKSPACE}"),
        seq,
        event_id: id.to_owned(),
        event_type: "RealMoneyGateChanged".to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: "2026-10-26T09:00:00.000000000Z".to_owned(),
        prev_hash: Digest::ZERO,
        hash: Digest::of(body),
        body: body.to_vec(),
    }
}

/// A journal holding the workspace's control stream, opened in paper.
fn journal() -> MemJournal {
    let journal = MemJournal::default();
    let text = format!(
        r#"{{"environment":"paper","event_type":"StreamOpened","payload":{{}},"stream_id":"ctl:{WORKSPACE}"}}"#
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    let mut opened = stored(1, "01JBH3BTZ8K2M4N6P8R0S2T001", &body);
    opened.event_type = "StreamOpened".to_owned();
    journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(format!("ctl:{WORKSPACE}"), vec![opened]);
    journal
}

fn control(journal: &MemJournal) -> Vec<StoredEvent> {
    journal.rows_of(&format!("ctl:{WORKSPACE}"))
}

fn run<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(future)
}

fn setting(key: &str, requested: Requested) -> Setting<'_> {
    Setting {
        workspace: WORKSPACE,
        operator: OPERATOR,
        key,
        requested,
    }
}

#[test]
fn a_customer_workspace_is_never_turned_on_and_nothing_is_appended() {
    for (founder, want) in founder_cases() {
        let Err(refusal) = want else { continue };
        let journal = journal();
        let got = run(set(
            &journal,
            &setting(KEY, on("DEC-2135")),
            founder.as_deref(),
            now(),
        ));
        assert_eq!(got.err(), Some(refusal), "{founder:?}");
        assert_eq!(
            control(&journal).len(),
            1,
            "nothing appended with {founder:?}"
        );
        assert_eq!(real_money::setting(&control(&journal)), Gate::Off);
    }
}

#[test]
fn the_founders_workspace_is_turned_on_then_off_and_the_gate_reads_each() {
    let journal = journal();
    let founder = Some(WORKSPACE);
    run(set(&journal, &setting(KEY, on("DEC-2135")), founder, now())).expect("on");
    assert_eq!(real_money::setting(&control(&journal)), Gate::On);
    run(set(&journal, &setting(KEY_2, Requested::Off), None, now()))
        .expect("off, with no founder setting");
    assert_eq!(real_money::setting(&control(&journal)), Gate::Off);
    assert_eq!(control(&journal).len(), 3);
}

#[test]
fn a_retry_under_its_key_appends_nothing_and_the_key_for_another_setting_is_refused() {
    let journal = journal();
    let first = run(set(&journal, &setting(KEY, Requested::Off), None, now())).expect("off");
    let again = run(set(&journal, &setting(KEY, Requested::Off), None, now())).expect("the retry");
    assert_eq!(first.event_id, again.event_id);
    assert_eq!(control(&journal).len(), 2);
    let reused = run(set(
        &journal,
        &setting(KEY, on("DEC-2135")),
        Some(WORKSPACE),
        now(),
    ));
    assert_eq!(reused.err(), Some(Refusal::KeyReused));
    assert_eq!(control(&journal).len(), 2);
    assert_eq!(real_money::setting(&control(&journal)), Gate::Off);
}

#[test]
fn a_workspace_with_no_control_stream_is_refused() {
    let journal = MemJournal::default();
    let got = run(set(&journal, &setting(KEY, Requested::Off), None, now()));
    assert_eq!(got.err(), Some(Refusal::JournalUnavailable));
    let failing = self::journal();
    failing
        .failing
        .lock()
        .expect("unpoisoned")
        .insert(format!("ctl:{WORKSPACE}"));
    let got = run(set(&failing, &setting(KEY, Requested::Off), None, now()));
    assert_eq!(got.err(), Some(Refusal::JournalUnavailable));
}

#[test]
fn the_command_line_names_one_verb_and_each_flag_once() {
    let parsed = |args: &[&str]| command(args.iter().map(|a| (*a).to_owned()));
    assert_eq!(
        parsed(&[
            "on",
            "--decision",
            "DEC-2135",
            "--operator",
            OPERATOR,
            "--key",
            KEY
        ]),
        Ok(Command {
            requested: on("DEC-2135"),
            operator: OPERATOR.to_owned(),
            key: KEY.to_owned()
        })
    );
    assert_eq!(
        parsed(&["off", "--key", KEY, "--operator", OPERATOR]),
        Ok(Command {
            requested: Requested::Off,
            operator: OPERATOR.to_owned(),
            key: KEY.to_owned()
        })
    );
    for bad in [
        &[][..],
        &["on", "--operator", OPERATOR, "--key", KEY][..],
        &[
            "off",
            "--decision",
            "DEC-1",
            "--operator",
            OPERATOR,
            "--key",
            KEY,
        ][..],
        &["enable", "--operator", OPERATOR, "--key", KEY][..],
        &["off", "--operator", OPERATOR][..],
        &["off", "--key", KEY][..],
        &["off", "--operator", OPERATOR, "--key"][..],
        &[
            "off",
            "--operator",
            OPERATOR,
            "--operator",
            OPERATOR,
            "--key",
            KEY,
        ][..],
        &[
            "off",
            "--operator",
            OPERATOR,
            "--key",
            KEY,
            "--workspace",
            OTHER,
        ][..],
    ] {
        assert_eq!(parsed(bad), Err(Refusal::Arguments), "{bad:?}");
    }
}

#[test]
fn each_refusal_has_its_own_code() {
    let codes = [
        Refusal::FounderWorkspaceUnset,
        Refusal::FounderWorkspaceAmbiguous,
        Refusal::NotTheFoundersWorkspace,
        Refusal::Decision,
        Refusal::Arguments,
        Refusal::KeyReused,
        Refusal::JournalUnavailable,
    ]
    .map(Refusal::code);
    assert_eq!(
        codes,
        [
            "founder_workspace_unset",
            "founder_workspace_ambiguous",
            "not_the_founders_workspace",
            "decision",
            "arguments",
            "key_reused",
            "journal_unavailable",
        ]
    );
}
