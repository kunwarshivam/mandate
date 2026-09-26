//! The corporate actions stored next to a dataset (E2-4): canonical JSON that round-trips, is
//! rewritten only when it changes, and is refused when hand-edited or foreign.

mod common;

use std::fs;

use common::{Scratch, day};
use mandate_canon::DecStr;
use mandate_marketdata::actions::{
    ActionsError, CORPORATE_ACTIONS, RecordedActions, read_actions, write_actions,
};
use mandate_marketdata::dataset::Status;
use mandate_marketdata::model::{
    CashDividend, CorporateActions, DayRange, OtherAction, Split, Symbol, split_ratio,
};

fn symbol(s: &str) -> Symbol {
    Symbol::parse(s).unwrap()
}

fn range(first: &str, last: &str) -> DayRange {
    DayRange::new(day(first), day(last)).unwrap()
}

fn nvda_june_2024() -> RecordedActions {
    RecordedActions {
        range: range("2024-06-07", "2024-06-10"),
        actions: CorporateActions {
            symbol: symbol("NVDA"),
            splits: vec![Split {
                id: "s1".to_owned(),
                ex_date: day("2024-06-10"),
                ratio: split_ratio(10, 1).unwrap(),
            }],
            cash_dividends: vec![CashDividend {
                id: "d1".to_owned(),
                ex_date: day("2024-06-10"),
                record_date: Some(day("2024-06-11")),
                payable_date: None,
                rate: DecStr::parse("0.01").unwrap(),
                special: false,
                foreign: true,
            }],
            other: vec![
                OtherAction {
                    id: "o1".to_owned(),
                    kind: "name_change".to_owned(),
                    ex_date: None,
                    process_date: day("2024-06-08"),
                },
                OtherAction {
                    id: "o2".to_owned(),
                    kind: "spin_off".to_owned(),
                    ex_date: Some(day("2024-06-09")),
                    process_date: day("2024-06-09"),
                },
            ],
        },
    }
}

const NVDA_JSON: &str = concat!(
    r#"{"cash_dividends":[{"ex_date":"2024-06-10","foreign":true,"id":"d1","payable_date":null,"#,
    r#""rate":"0.01","record_date":"2024-06-11","special":false}],"first":"2024-06-07","#,
    r#""format":"mandate-corporate-actions/1","last":"2024-06-10","other":[{"ex_date":null,"#,
    r#""id":"o1","kind":"name_change","process_date":"2024-06-08"},{"ex_date":"2024-06-09","#,
    r#""id":"o2","kind":"spin_off","process_date":"2024-06-09"}],"splits":[{"ex_date":"2024-06-10","#,
    r#""id":"s1","new_shares":10,"old_shares":1}],"symbol":"NVDA"}"#,
);

#[test]
fn recorded_actions_are_canonical_json_that_reads_back_the_same() {
    let scratch = Scratch::new("actions-round-trip");
    let recorded = nvda_june_2024();
    assert_eq!(
        write_actions(scratch.path(), &recorded).unwrap(),
        Status::Written
    );
    let path = scratch.path().join(CORPORATE_ACTIONS);
    assert_eq!(fs::read_to_string(&path).unwrap(), NVDA_JSON);
    assert_eq!(
        read_actions(scratch.path(), &symbol("NVDA")).unwrap(),
        Some(recorded)
    );
    let names: Vec<String> = fs::read_dir(scratch.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [CORPORATE_ACTIONS], "no temporary file is left");
}

#[test]
fn no_actions_is_an_empty_record_of_its_range() {
    let scratch = Scratch::new("actions-none");
    let recorded = RecordedActions {
        range: range("2026-09-23", "2026-09-24"),
        actions: CorporateActions::none(symbol("BRK.B")),
    };
    write_actions(scratch.path(), &recorded).unwrap();
    assert_eq!(
        fs::read_to_string(scratch.path().join(CORPORATE_ACTIONS)).unwrap(),
        r#"{"cash_dividends":[],"first":"2026-09-23","format":"mandate-corporate-actions/1","last":"2026-09-24","other":[],"splits":[],"symbol":"BRK.B"}"#
    );
    assert_eq!(
        read_actions(scratch.path(), &symbol("BRK.B")).unwrap(),
        Some(recorded)
    );
}

#[test]
fn the_same_actions_leave_the_file_alone_and_different_ones_replace_it() {
    let scratch = Scratch::new("actions-rewrite");
    let path = scratch.path().join(CORPORATE_ACTIONS);
    let recorded = nvda_june_2024();
    write_actions(scratch.path(), &recorded).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(
        write_actions(scratch.path(), &recorded).unwrap(),
        Status::Unchanged
    );
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);

    let mut wider = recorded.clone();
    wider.range = range("2024-06-07", "2024-06-14");
    assert_eq!(
        write_actions(scratch.path(), &wider).unwrap(),
        Status::Written
    );
    assert_eq!(
        read_actions(scratch.path(), &symbol("NVDA")).unwrap(),
        Some(wider)
    );
}

#[test]
fn a_directory_without_the_file_has_no_recorded_actions() {
    let scratch = Scratch::new("actions-absent");
    assert_eq!(read_actions(scratch.path(), &symbol("SPY")).unwrap(), None);
}

#[test]
fn writing_creates_the_dataset_directory() {
    let scratch = Scratch::new("actions-mkdir");
    let dir = scratch.path().join("alpaca/sip/bars-1Day/NVDA");
    write_actions(&dir, &nvda_june_2024()).unwrap();
    assert!(dir.join(CORPORATE_ACTIONS).is_file());
}

fn refused(bytes: &str, reading: &str) -> ActionsError {
    let scratch = Scratch::new("actions-refused");
    fs::write(scratch.path().join(CORPORATE_ACTIONS), bytes).unwrap();
    read_actions(scratch.path(), &symbol(reading)).unwrap_err()
}

#[test]
fn a_hand_edited_foreign_or_malformed_file_is_refused() {
    let spaced = NVDA_JSON.replacen(",", ", ", 1);
    let other_format =
        NVDA_JSON.replace("mandate-corporate-actions/1", "mandate-corporate-actions/2");
    let reordered = NVDA_JSON.replace(
        r#""first":"2024-06-07","format":"mandate-corporate-actions/1","last":"2024-06-10""#,
        r#""first":"2024-06-10","format":"mandate-corporate-actions/1","last":"2024-06-07""#,
    );
    let bad_ratio = NVDA_JSON.replace(r#""new_shares":10"#, r#""new_shares":0"#);
    let bad_rate = NVDA_JSON.replace(r#""rate":"0.01""#, r#""rate":"1e-2""#);
    let numeric_rate = NVDA_JSON.replace(r#""rate":"0.01""#, r#""rate":1"#);
    let bad_date = NVDA_JSON.replace(
        r#""process_date":"2024-06-08""#,
        r#""process_date":"2024-6-8""#,
    );
    let missing = NVDA_JSON.replace(r#","symbol":"NVDA""#, "");
    let extra = NVDA_JSON.replace(r#""symbol":"NVDA""#, r#""symbol":"NVDA","zz":1"#);
    let bad_flag = NVDA_JSON.replace(r#""special":false"#, r#""special":"no""#);
    let bad_symbol = NVDA_JSON.replace(r#""symbol":"NVDA""#, r#""symbol":"nvda""#);
    for (text, reading) in [
        (NVDA_JSON, "SPY"),
        (spaced.as_str(), "NVDA"),
        (other_format.as_str(), "NVDA"),
        (reordered.as_str(), "NVDA"),
        (bad_ratio.as_str(), "NVDA"),
        (bad_rate.as_str(), "NVDA"),
        (numeric_rate.as_str(), "NVDA"),
        (bad_date.as_str(), "NVDA"),
        (missing.as_str(), "NVDA"),
        (extra.as_str(), "NVDA"),
        (bad_flag.as_str(), "NVDA"),
        (bad_symbol.as_str(), "NVDA"),
        ("not json", "NVDA"),
        ("[]", "NVDA"),
    ] {
        let err = refused(text, reading);
        assert!(
            matches!(err, ActionsError::Invalid { .. }),
            "{text}: {err:?}"
        );
        assert_eq!(err.code(), "actions_file", "{text}");
    }
}

#[test]
fn a_file_that_cannot_be_read_is_an_io_error() {
    let scratch = Scratch::new("actions-io");
    fs::create_dir(scratch.path().join(CORPORATE_ACTIONS)).unwrap();
    let err = read_actions(scratch.path(), &symbol("NVDA")).unwrap_err();
    assert!(matches!(err, ActionsError::Io { .. }), "{err:?}");
    assert_eq!(err.code(), "io");
    let err = write_actions(scratch.path(), &nvda_june_2024()).unwrap_err();
    assert_eq!(err.code(), "io", "{err:?}");
}
