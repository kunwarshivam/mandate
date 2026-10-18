//! The backtest and evaluation folds (§4.2, §4.7; DEC-1001 item 5, DEC-1002 items 6 and 7,
//! DEC-1143) against hand-built control-stream rows and the schemas' own example reports, and the
//! internal paper workspace's start checks.

mod backtest_rows;

use std::ffi::OsStr;

use backtest_rows::{
    BACKTEST_EXAMPLE, BACKTEST_ID, EVALUATION_EXAMPLE, backtest_payload, evaluation_payload,
    example_report, example_version, reference, row, row_in, set,
};
use mandate_api_server::backtests::{
    BacktestView, EvaluationView, Found, Outcome, Unserved, backtest, backtest_report, evaluation,
    evaluation_report,
};
use mandate_api_server::config::{
    INTERNAL_PAPER_WORKSPACE, StartRefusal, check_internal_paper, internal_paper_workspace,
};
use mandate_api_server::journal::JournalUnavailable;
use mandate_canon::{Int, Value, parse, to_canonical};
use mandate_journal::{ArtifactRef, StoredEvent};

fn completed(id: &str, report: &str) -> StoredEvent {
    row(
        2,
        "BacktestRunRecorded",
        &backtest_payload(id, &reference(0xef), "completed", Some(report), None),
    )
}

fn refused(id: &str, code: &str) -> StoredEvent {
    row(
        3,
        "BacktestRunRecorded",
        &backtest_payload(id, &reference(0xef), "refused", None, Some(code)),
    )
}

fn opened() -> StoredEvent {
    row(1, "StreamOpened", "{}")
}

fn completed_view(id: &str, report: &str) -> BacktestView {
    BacktestView {
        backtest_id: id.to_owned(),
        mandate_version: reference(0xef),
        outcome: Outcome::Completed {
            report_ref: ArtifactRef::parse(report).expect("a reference"),
        },
    }
}

#[test]
fn a_completed_and_a_refused_backtest_are_found_by_their_ids() {
    let control = vec![
        opened(),
        completed("bt_1", &reference(0x12)),
        refused("bt_2", "corporate_action_in_range"),
    ];
    let one = backtest(&control, "bt_1");
    assert_eq!(one, Found::One(completed_view("bt_1", &reference(0x12))));
    let Found::One(view) = one else {
        unreachable!()
    };
    assert_eq!(view.state(), "completed");
    let two = backtest(&control, "bt_2");
    assert_eq!(
        two,
        Found::One(BacktestView {
            backtest_id: "bt_2".to_owned(),
            mandate_version: reference(0xef),
            outcome: Outcome::Refused {
                refusal: "corporate_action_in_range".to_owned()
            },
        })
    );
    let Found::One(view) = two else {
        unreachable!()
    };
    assert_eq!(view.state(), "refused");
}

#[test]
fn an_id_is_found_only_by_equality_and_only_on_its_own_record() {
    let control = vec![
        opened(),
        completed("bt_1", &reference(0x12)),
        row(
            3,
            "EvaluationRunRecorded",
            &evaluation_payload("bt_9", &reference(0x34)),
        ),
        row(4, "AgentDeployed", r#"{"backtest_id":"bt_8"}"#),
    ];
    for absent in ["bt_", "bt_10", "BT_1", "bt_1 ", "bt_9", "bt_8", ""] {
        assert_eq!(backtest(&control, absent), Found::Absent, "{absent:?}");
    }
    assert_eq!(backtest(&[], "bt_1"), Found::Absent);
}

#[test]
fn an_id_outside_the_id_grammar_names_nothing_even_on_a_record() {
    let long = "a".repeat(65);
    let control = vec![
        completed(&long, &reference(0x12)),
        completed("bt.1", &reference(0x12)),
    ];
    assert_eq!(backtest(&control, &long), Found::Absent);
    assert_eq!(backtest(&control, "bt.1"), Found::Absent);
    let longest = "a".repeat(64);
    assert_eq!(
        backtest(&[completed(&longest, &reference(0x12))], &longest),
        Found::One(completed_view(&longest, &reference(0x12)))
    );
}

#[test]
fn a_body_that_does_not_parse_is_skipped_as_the_other_reads_skip_it() {
    let mut broken = completed("bt_1", &reference(0x12));
    broken.body = b"not json".to_vec();
    assert_eq!(backtest(&[broken.clone()], "bt_1"), Found::Absent);
    let good = completed("bt_1", &reference(0x13));
    assert_eq!(
        backtest(&[broken, good], "bt_1"),
        Found::One(completed_view("bt_1", &reference(0x13)))
    );
}

#[test]
fn a_second_record_with_the_same_id_is_unreadable_never_first_or_last() {
    let first = completed("bt_1", &reference(0x12));
    for second in [
        completed("bt_1", &reference(0x12)),
        completed("bt_1", &reference(0x13)),
        refused("bt_1", "no_bars"),
    ] {
        assert_eq!(
            backtest(&[first.clone(), second.clone()], "bt_1"),
            Found::Unreadable,
            "{second:?}"
        );
        assert_eq!(
            backtest(&[second, first.clone()], "bt_1"),
            Found::Unreadable
        );
    }
    let three = [
        completed("bt_1", &reference(0x12)),
        completed("bt_1", &reference(0x12)),
        completed("bt_1", &reference(0x12)),
    ];
    assert_eq!(backtest(&three, "bt_1"), Found::Unreadable);
    let malformed_then_good = [
        versioned(completed("bt_1", &reference(0x12)), 2),
        completed("bt_1", &reference(0x12)),
    ];
    assert_eq!(backtest(&malformed_then_good, "bt_1"), Found::Unreadable);
}

fn versioned(mut row: StoredEvent, schema_version: u64) -> StoredEvent {
    row.schema_version = schema_version;
    row
}

/// `payload` with `member` set to `to`, as a `BacktestRunRecorded` row.
fn with(payload: &str, member: &str, to: Value) -> StoredEvent {
    let mut value = parse(payload.as_bytes()).expect("JSON");
    set(&mut value, member, to);
    let text = String::from_utf8(to_canonical(&value)).expect("UTF-8");
    row(2, "BacktestRunRecorded", &text)
}

fn s(text: &str) -> Value {
    Value::Str(text.to_owned())
}

fn n(value: u64) -> Value {
    Value::Int(Int::new(value).expect("an int"))
}

#[test]
fn a_record_that_breaks_section_9_18_is_unreadable_never_served() {
    let done = backtest_payload(
        "bt_1",
        &reference(0xef),
        "completed",
        Some(&reference(0x12)),
        None,
    );
    let no = backtest_payload("bt_1", &reference(0xef), "refused", None, Some("no_bars"));
    let cases = [
        ("marker", with(&done, "marker", s("paper"))),
        ("report version 1", with(&done, "report_version", n(1))),
        ("report version text", with(&done, "report_version", s("2"))),
        ("a running result", with(&done, "result", s("running"))),
        ("completed, no report", with(&done, "report", Value::Null)),
        (
            "completed, with a refusal",
            with(&done, "refusal", s("no_bars")),
        ),
        (
            "completed, report not a ref",
            with(&done, "report", s("sha256:12")),
        ),
        ("completed, report a number", with(&done, "report", n(12))),
        (
            "refused, with a report",
            with(&no, "report", s(&reference(0x12))),
        ),
        ("refused, no refusal", with(&no, "refusal", Value::Null)),
        ("refused, refusal a number", with(&no, "refusal", n(3))),
        ("refusal capitalized", with(&no, "refusal", s("No_bars"))),
        ("refusal with a hyphen", with(&no, "refusal", s("no-bars"))),
        (
            "refusal starting with a digit",
            with(&no, "refusal", s("1_bars")),
        ),
        ("refusal empty", with(&no, "refusal", s(""))),
        ("refusal too long", with(&no, "refusal", s(&"a".repeat(65)))),
        ("version not a ref", with(&done, "mandate_version", s("v1"))),
        (
            "version absent",
            with(&done, "mandate_version", Value::Null),
        ),
        (
            "schema version 2",
            versioned(completed("bt_1", &reference(0x12)), 2),
        ),
        (
            "schema version 0",
            versioned(completed("bt_1", &reference(0x12)), 0),
        ),
    ];
    for (what, row) in cases {
        assert_eq!(backtest(&[row], "bt_1"), Found::Unreadable, "{what}");
    }
    for code in ["a", "no_bars", "x9_0", &"a".repeat(64)] {
        assert!(
            matches!(
                backtest(&[with(&no, "refusal", s(code))], "bt_1"),
                Found::One(_)
            ),
            "{code}"
        );
    }
}

#[test]
fn an_evaluation_is_found_by_its_id_and_a_duplicate_or_malformed_one_is_unreadable() {
    let record = |seq, id: &str, report: &str| {
        row(
            seq,
            "EvaluationRunRecorded",
            &evaluation_payload(id, report),
        )
    };
    let control = vec![
        opened(),
        record(2, "ev_1", &reference(0x34)),
        completed("ev_2", &reference(0x12)),
    ];
    assert_eq!(
        evaluation(&control, "ev_1"),
        Found::One(EvaluationView {
            evaluation_id: "ev_1".to_owned(),
            report_ref: ArtifactRef::parse(&reference(0x34)).expect("a reference"),
        })
    );
    assert_eq!(evaluation(&control, "ev_2"), Found::Absent);
    assert_eq!(evaluation(&control, "ev_"), Found::Absent);
    assert_eq!(evaluation(&control, &"e".repeat(65)), Found::Absent);
    let twice = [
        record(2, "ev_1", &reference(0x34)),
        record(3, "ev_1", &reference(0x34)),
    ];
    assert_eq!(evaluation(&twice, "ev_1"), Found::Unreadable);
    let payload = evaluation_payload("ev_1", &reference(0x34));
    let edited = |member: &str, to: Value| {
        let mut value = parse(payload.as_bytes()).expect("JSON");
        set(&mut value, member, to);
        let text = String::from_utf8(to_canonical(&value)).expect("UTF-8");
        row(2, "EvaluationRunRecorded", &text)
    };
    for (what, row) in [
        ("report not a ref", edited("report", s("sha256:34"))),
        ("report null", edited("report", Value::Null)),
        ("report version 2", edited("report_version", n(2))),
        (
            "schema version 2",
            versioned(record(2, "ev_1", &reference(0x34)), 2),
        ),
    ] {
        assert_eq!(evaluation(&[row], "ev_1"), Found::Unreadable, "{what}");
    }
}

#[test]
fn a_backtest_report_is_served_only_as_its_own_versions_report_object() {
    let (bytes, _) = example_report(BACKTEST_EXAMPLE, "backtest");
    let view = BacktestView {
        backtest_id: BACKTEST_ID.to_owned(),
        mandate_version: example_version(),
        outcome: Outcome::Completed {
            report_ref: ArtifactRef::of(&bytes),
        },
    };
    assert_eq!(
        backtest_report(&bytes, &view),
        Ok(parse(&bytes).expect("JSON"))
    );
    let other_version = BacktestView {
        mandate_version: reference(0x77),
        ..view.clone()
    };
    assert_eq!(
        backtest_report(&bytes, &other_version),
        Err(Unserved::OtherVersion)
    );
    let report = parse(&bytes).expect("JSON");
    for (member, to, refused) in [
        ("kind", s("forward_paper_evaluation"), Unserved::NotAReport),
        ("report_version", n(1), Unserved::NotAReport),
        ("report_version", s("2"), Unserved::NotAReport),
        (
            "mandate_version",
            s(&reference(0x77)),
            Unserved::OtherVersion,
        ),
        ("mandate_version", Value::Null, Unserved::OtherVersion),
    ] {
        let mut edited = report.clone();
        set(&mut edited, member, to);
        assert_eq!(
            backtest_report(&to_canonical(&edited), &view),
            Err(refused),
            "{member}"
        );
    }
    for bytes in [b"not json".as_slice(), b"[]", b"\"backtest_report\""] {
        assert_eq!(backtest_report(bytes, &view), Err(Unserved::NotAReport));
    }
}

#[test]
fn an_evaluation_report_is_served_only_as_a_version_1_evaluation_object() {
    let (bytes, _) = example_report(EVALUATION_EXAMPLE, "evaluation");
    assert_eq!(evaluation_report(&bytes), Ok(parse(&bytes).expect("JSON")));
    let report = parse(&bytes).expect("JSON");
    for (member, to) in [
        ("kind", s("backtest_report")),
        ("report_version", n(2)),
        ("report_version", s("1")),
    ] {
        let mut edited = report.clone();
        set(&mut edited, member, to);
        assert_eq!(
            evaluation_report(&to_canonical(&edited)),
            Err(Unserved::NotAReport),
            "{member}"
        );
    }
    assert_eq!(evaluation_report(b"[1]"), Err(Unserved::NotAReport));
    assert_eq!(evaluation_report(b"{"), Err(Unserved::NotAReport));
}

#[test]
fn a_paper_stream_has_a_row_and_every_row_on_paper() {
    let shown = |rows: &[StoredEvent]| check_internal_paper(Ok(rows));
    assert_eq!(shown(&[opened(), row(2, "AgentDeployed", "{}")]), Ok(()));
    let refused = Err(StartRefusal::InternalWorkspaceNotPaper);
    assert_eq!(shown(&[]), refused, "an empty stream shows nothing");
    for environment in ["live", "Paper", "backtest", ""] {
        assert_eq!(
            shown(&[opened(), row_in(environment, 2, "AgentDeployed", "{}")]),
            refused,
            "{environment:?}"
        );
        assert_eq!(
            shown(&[row_in(environment, 1, "StreamOpened", "{}"), opened()]),
            refused,
            "{environment:?} first"
        );
    }
}

#[test]
fn the_internal_paper_setting_is_off_unless_exactly_true() {
    assert_eq!(internal_paper_workspace(None), Ok(false));
    assert_eq!(internal_paper_workspace(Some(OsStr::new(""))), Ok(false));
    assert_eq!(
        internal_paper_workspace(Some(OsStr::new("false"))),
        Ok(false)
    );
    assert_eq!(internal_paper_workspace(Some(OsStr::new("true"))), Ok(true));
    let refused = Err(StartRefusal::Setting {
        name: INTERNAL_PAPER_WORKSPACE,
    });
    for value in ["TRUE", "True", "1", "yes", "on", " true", "true ", "0"] {
        assert_eq!(
            internal_paper_workspace(Some(OsStr::new(value))),
            refused,
            "{value:?}"
        );
    }
    let invalid = std::os::unix::ffi::OsStrExt::from_bytes(b"tr\xffue");
    assert_eq!(internal_paper_workspace(Some(invalid)), refused);
    assert_eq!(
        INTERNAL_PAPER_WORKSPACE,
        "MANDATE_API_INTERNAL_PAPER_WORKSPACE"
    );
}

#[test]
fn the_start_check_refuses_the_setting_outside_a_paper_stream() {
    let paper_rows = [opened(), row(2, "AgentDeployed", "{}")];
    assert_eq!(check_internal_paper(Ok(&paper_rows)), Ok(()));
    let live = [row_in("live", 1, "StreamOpened", "{}")];
    let mixed = [opened(), row_in("live", 2, "AgentDeployed", "{}")];
    for (what, read) in [
        ("live", Ok(live.as_slice())),
        ("mixed", Ok(mixed.as_slice())),
        ("empty", Ok([].as_slice())),
        ("unreadable", Err(JournalUnavailable)),
    ] {
        assert_eq!(
            check_internal_paper(read),
            Err(StartRefusal::InternalWorkspaceNotPaper),
            "{what}"
        );
    }
    let text = StartRefusal::InternalWorkspaceNotPaper.to_string();
    assert!(text.contains(INTERNAL_PAPER_WORKSPACE), "{text}");
    assert!(text.contains("not shown to be paper"), "{text}");
}
