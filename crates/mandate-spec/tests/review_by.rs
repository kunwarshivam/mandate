//! `autonomy.review_by`, the review date (mandate spec §3, §6.6, §9.2; DEC-188, DEC-271 to
//! DEC-273; E6-14): the parse reads it as the date it names, and a version change classifies it by
//! its own row.
//!
//! Pending on E6-14's implementation PR: until then the parse refuses a document that sets a review
//! date as unimplemented, so it fails closed rather than loading without it.

mod common;

use common::{base, edit, s, with};
use mandate_canon::Value;
use mandate_spec::Mandate;
use mandate_spec::change::{ChangeClass, classify};
use mandate_time::Date;

use ChangeClass::{RiskIncreasing, RiskReducing};

fn day(text: &str) -> Date {
    Date::parse(text).expect("a date")
}

fn reviewed(text: &str) -> Value {
    with("/autonomy/review_by", Some(s(text)))
}

/// The class, the changed paths as text, and whether step-up is required.
fn verdict(old: &Value, new: &Value) -> (ChangeClass, Vec<String>, bool) {
    let c = classify(
        &Mandate::parse(old).expect("the old version parses"),
        &Mandate::parse(new).expect("the new version parses"),
    )
    .expect("two parsed mandates classify");
    let paths = c
        .changed_paths
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    (c.class, paths, c.step_up_required)
}

/// §3: the review date is a calendar date, read as the date it names; a document without one has
/// none; and a date the schema's pattern admits but the calendar does not is refused, never read as
/// "no review date", so a malformed date cannot switch the review off (DEC-271 item 2).
#[test]
fn a_review_date_parses_as_the_date_it_names() {
    let parsed = Mandate::parse(&reviewed("2026-12-23")).expect("a reviewed document parses");
    assert_eq!(parsed.autonomy.review_by, Some(day("2026-12-23")));
    let leap = Mandate::parse(&reviewed("2028-02-29")).expect("a leap day parses");
    assert_eq!(leap.autonomy.review_by, Some(day("2028-02-29")));
    let none = Mandate::parse(&base()).expect("the base parses");
    assert_eq!(none.autonomy.review_by, None);
    assert!(
        Mandate::parse(&reviewed("2026-11-31")).is_err(),
        "a date that is not on the calendar is refused rather than read as no review date"
    );
    assert!(
        Mandate::parse(&with("/autonomy/review_by", Some(Value::Null))).is_err(),
        "the schema's review date is a date, never null"
    );
}

/// §9.2's row for `autonomy.review_by` (DEC-273): setting one where there was none, or moving it
/// earlier, is risk-reducing; moving it later or removing it is risk-increasing and needs step-up.
/// Each change is exactly the one path, and a stricter rule beside an earlier date is still
/// risk-reducing, so the autonomy row does not read the review date as a change of its own.
#[test]
fn the_review_date_classifies_by_its_own_row() {
    let set = reviewed("2026-12-23");
    for (old, new, class) in [
        (base(), set.clone(), RiskReducing),
        (set.clone(), reviewed("2026-11-01"), RiskReducing),
        (set.clone(), reviewed("2027-03-01"), RiskIncreasing),
        (
            set.clone(),
            with("/autonomy/review_by", None),
            RiskIncreasing,
        ),
    ] {
        let (got, paths, step_up) = verdict(&old, &new);
        assert_eq!(got, class, "{old:?} -> {new:?}");
        assert_eq!(paths, vec!["/autonomy/review_by".to_owned()]);
        assert_eq!(
            step_up,
            class == RiskIncreasing,
            "step-up follows the class"
        );
    }
    let tightened = edit(
        &set,
        &[
            ("/autonomy/review_by", Some(s("2026-11-01"))),
            ("/autonomy/rules/0/then", Some(s("deny"))),
        ],
    );
    assert_eq!(verdict(&set, &tightened).0, RiskReducing);
    let later_and_tightened = edit(
        &set,
        &[
            ("/autonomy/review_by", Some(s("2027-03-01"))),
            ("/autonomy/rules/0/then", Some(s("deny"))),
        ],
    );
    assert_eq!(verdict(&set, &later_and_tightened).0, RiskIncreasing);
}
