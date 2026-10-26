//! Spec §6.5's strict parse: an exact schema-valid output within its bounds, or a rejection. A
//! rejection is no output (HI-6), so every case here asserts which side it lands on.

use mandate_num::{Conviction, Unit};
use mandate_research_run::agentic::RunKind;
use mandate_research_run::{OutputBounds, OutputRejection, parse_output};
use proptest::prelude::*;

const ONE_THESIS: &str = r#"{"theses":[{"instrument":"AAPL","direction":"long","conviction":"0.6","confidence":"0.7","horizon_s":432000,"thesis":"Services revenue keeps compounding.","invalidation":"Guidance is cut.","evidence":["n-1","bar:AAPL:1792094400"]}],"invalidations":[]}"#;

fn bounds() -> OutputBounds {
    OutputBounds::v0_1()
}

fn thesis_json(instrument: &str, conviction: &str, horizon: u64) -> String {
    format!(
        r#"{{"instrument":"{instrument}","direction":"long","conviction":"{conviction}","confidence":"0.5","horizon_s":{horizon},"thesis":"t","invalidation":"i","evidence":["n-1"]}}"#
    )
}

fn output_with(theses: &[String], invalidations: &str) -> String {
    format!(
        r#"{{"theses":[{}],"invalidations":[{invalidations}]}}"#,
        theses.join(",")
    )
}

fn rejected(text: &str) -> OutputRejection {
    match parse_output(text.as_bytes(), &bounds(), RunKind::Ideate) {
        Ok(output) => panic!("expected a rejection, parsed {output:?}"),
        Err(rejection) => rejection,
    }
}

#[test]
fn a_schema_valid_output_parses_into_typed_candidates() {
    let output = parse_output(ONE_THESIS.as_bytes(), &bounds(), RunKind::Ideate)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(output.theses.len(), 1);
    let thesis = &output.theses[0];
    assert_eq!(thesis.instrument, "AAPL");
    assert_eq!(thesis.direction, "long");
    assert_eq!(
        thesis.conviction,
        Conviction::parse("0.6").unwrap_or_else(|e| panic!("{e}"))
    );
    assert_eq!(
        thesis.confidence,
        Unit::parse("0.7").unwrap_or_else(|e| panic!("{e}"))
    );
    assert_eq!(thesis.horizon_s, 432_000);
    assert_eq!(thesis.evidence, vec!["n-1", "bar:AAPL:1792094400"]);
    assert_eq!(thesis.predecessor_thesis_id, None);
    assert!(output.invalidations.is_empty());
}

#[test]
fn a_non_long_direction_is_passed_through_for_admission_to_journal() {
    let text = ONE_THESIS.replace(r#""direction":"long""#, r#""direction":"short""#);
    let output =
        parse_output(text.as_bytes(), &bounds(), RunKind::Ideate).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(output.theses[0].direction, "short");
}

#[test]
fn a_predecessor_and_an_invalidation_parse() {
    let text = output_with(
        &[ONE_THESIS
            .split_once(r#""theses":["#)
            .and_then(|(_, rest)| rest.split_once(r#"],"invalidations""#))
            .map(|(thesis, _)| {
                thesis.replace(
                    r#""evidence""#,
                    r#""predecessor_thesis_id":"01JTHESIS0000000000000000A","evidence""#,
                )
            })
            .unwrap_or_default()],
        r#"{"thesis_id":"01JTHESIS0000000000000000B","evidence":["n-2"]}"#,
    );
    let output =
        parse_output(text.as_bytes(), &bounds(), RunKind::Ideate).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        output.theses[0].predecessor_thesis_id.as_deref(),
        Some("01JTHESIS0000000000000000A")
    );
    assert_eq!(output.invalidations.len(), 1);
    assert_eq!(
        output.invalidations[0].thesis_id,
        "01JTHESIS0000000000000000B"
    );
    assert_eq!(output.invalidations[0].evidence, vec!["n-2"]);
}

#[test]
fn a_float_is_not_json_here() {
    let text = ONE_THESIS.replace(r#""conviction":"0.6""#, r#""conviction":0.6"#);
    assert_eq!(rejected(&text), OutputRejection::NotJson);
}

#[test]
fn trailing_data_and_duplicate_keys_are_not_json() {
    assert_eq!(
        rejected(&format!("{ONE_THESIS} {{}}")),
        OutputRejection::NotJson
    );
    let duplicated = ONE_THESIS.replace(
        r#""direction":"long""#,
        r#""direction":"long","direction":"long""#,
    );
    assert_eq!(rejected(&duplicated), OutputRejection::NotJson);
    assert_eq!(rejected("not json at all"), OutputRejection::NotJson);
}

#[test]
fn an_unknown_member_is_a_shape_rejection_anywhere() {
    let top = ONE_THESIS.replacen('{', r#"{"orders":[],"#, 1);
    assert!(matches!(rejected(&top), OutputRejection::Shape { .. }));
    let inner = ONE_THESIS.replace(r#""direction""#, r#""qty":"100","direction""#);
    assert!(matches!(rejected(&inner), OutputRejection::Shape { .. }));
    let invalidation = output_with(&[], r#"{"thesis_id":"t","evidence":[],"sell":true}"#);
    assert!(matches!(
        rejected(&invalidation),
        OutputRejection::Shape { .. }
    ));
}

#[test]
fn a_missing_member_or_a_wrong_type_is_a_shape_rejection() {
    let missing = ONE_THESIS.replace(r#""invalidation":"Guidance is cut.","#, "");
    assert!(matches!(rejected(&missing), OutputRejection::Shape { .. }));
    let no_list = r#"{"theses":[]}"#;
    assert!(matches!(rejected(no_list), OutputRejection::Shape { .. }));
    let wrong = ONE_THESIS.replace(r#""horizon_s":432000"#, r#""horizon_s":"432000""#);
    assert!(matches!(rejected(&wrong), OutputRejection::Shape { .. }));
    assert!(matches!(rejected("[]"), OutputRejection::Shape { .. }));
}

#[test]
fn conviction_and_confidence_outside_their_intervals_are_range_rejections() {
    let high = ONE_THESIS.replace(r#""conviction":"0.6""#, r#""conviction":"1.5""#);
    assert!(matches!(rejected(&high), OutputRejection::Range { .. }));
    let low = ONE_THESIS.replace(r#""conviction":"0.6""#, r#""conviction":"-1.01""#);
    assert!(matches!(rejected(&low), OutputRejection::Range { .. }));
    let unit = ONE_THESIS.replace(r#""confidence":"0.7""#, r#""confidence":"-0.1""#);
    assert!(matches!(rejected(&unit), OutputRejection::Range { .. }));
    let text = ONE_THESIS.replace(r#""conviction":"0.6""#, r#""conviction":"lots""#);
    assert!(matches!(rejected(&text), OutputRejection::Range { .. }));
}

#[test]
fn the_horizon_runs_from_one_day_to_thirty_days() {
    for (horizon, ok) in [
        (86_399, false),
        (86_400, true),
        (2_592_000, true),
        (2_592_001, false),
    ] {
        let text = output_with(&[thesis_json("AAPL", "0.5", horizon)], "");
        assert_eq!(
            parse_output(text.as_bytes(), &bounds(), RunKind::Ideate).is_ok(),
            ok,
            "horizon {horizon}"
        );
    }
}

#[test]
fn more_than_four_theses_is_too_many() {
    let four: Vec<String> = ["AAPL", "MSFT", "NVDA", "AMZN"]
        .iter()
        .map(|s| thesis_json(s, "0.5", 86_400))
        .collect();
    assert!(
        parse_output(
            output_with(&four, "").as_bytes(),
            &bounds(),
            RunKind::Ideate
        )
        .is_ok()
    );
    let mut five = four;
    five.push(thesis_json("GOOG", "0.5", 86_400));
    assert!(matches!(
        rejected(&output_with(&five, "")),
        OutputRejection::TooMany { .. }
    ));
}

#[test]
fn two_invalidations_of_one_thesis_are_refused() {
    let text = output_with(
        &[],
        r#"{"thesis_id":"t-1","evidence":[]},{"thesis_id":"t-1","evidence":["n-1"]}"#,
    );
    assert_eq!(rejected(&text), OutputRejection::DuplicateInvalidation);
}

#[test]
fn text_must_be_present_and_within_its_bound() {
    let empty = ONE_THESIS.replace(
        r#""invalidation":"Guidance is cut.""#,
        r#""invalidation":"""#,
    );
    assert!(matches!(rejected(&empty), OutputRejection::Range { .. }));
    let long = ONE_THESIS.replace("Services revenue keeps compounding.", &"x".repeat(2_001));
    assert!(matches!(rejected(&long), OutputRejection::Range { .. }));
}

#[test]
fn blank_or_control_text_is_a_range_rejection_wherever_it_is() {
    let fields = [
        r#""invalidation":"Guidance is cut.""#,
        r#""thesis":"Services revenue keeps compounding.""#,
        r#""instrument":"AAPL""#,
        r#""direction":"long""#,
    ];
    for field in fields {
        let (name, _) = field.split_once(':').unwrap_or_default();
        for bad in [r#""   ""#, r#""\t""#, r#""a\u0000b""#, r#""line\nbreak""#] {
            let text = ONE_THESIS.replace(field, &format!("{name}:{bad}"));
            assert!(
                matches!(rejected(&text), OutputRejection::Range { .. }),
                "{name} = {bad}"
            );
        }
    }
    let blank_id = ONE_THESIS.replace(r#""n-1""#, r#"" ""#);
    assert!(matches!(rejected(&blank_id), OutputRejection::Range { .. }));
    let blank_thesis = output_with(&[], r#"{"thesis_id":"  ","evidence":[]}"#);
    assert!(matches!(
        rejected(&blank_thesis),
        OutputRejection::Range { .. }
    ));
}

#[test]
fn evidence_is_bounded() {
    let many: Vec<String> = (0..21).map(|n| format!(r#""n-{n}""#)).collect();
    let text = ONE_THESIS.replace(
        r#"["n-1","bar:AAPL:1792094400"]"#,
        &format!("[{}]", many.join(",")),
    );
    assert!(matches!(rejected(&text), OutputRejection::TooMany { .. }));
}

#[test]
fn an_output_over_its_byte_bound_is_not_read_at_all() {
    let padding = " ".repeat(65_537);
    assert_eq!(
        rejected(&format!("{ONE_THESIS}{padding}")),
        OutputRejection::TooLong
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Arbitrary bytes either parse into candidates inside every bound, checked here by an oracle
    /// that recounts and re-ranges the parsed values its own way, or are rejected. Nothing panics.
    #[test]
    fn arbitrary_bytes_never_yield_an_out_of_bounds_candidate(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        if let Ok(output) = parse_output(&bytes, &bounds(), RunKind::Ideate) {
            prop_assert!(output.theses.len() <= 4);
            for thesis in &output.theses {
                prop_assert!((86_400..=2_592_000).contains(&thesis.horizon_s));
                prop_assert!(!thesis.thesis.is_empty() && thesis.thesis.len() <= 2_000);
            }
        }
    }

    /// Schema-shaped outputs with random values: one parses exactly when the oracle, which
    /// works in integer thousandths and writes its own canonical text, puts the conviction in
    /// [−1, 1], the confidence in [0, 1], and the horizon in [1 day, 30 days].
    #[test]
    fn random_values_parse_only_inside_their_ranges(
        conviction in -2_000i64..=2_000,
        confidence in -500i64..=1_500,
        horizon in 0u64..5_000_000,
    ) {
        let text = format!(
            r#"{{"theses":[{{"instrument":"AAPL","direction":"long","conviction":"{}","confidence":"{}","horizon_s":{horizon},"thesis":"t","invalidation":"i","evidence":[]}}],"invalidations":[]}}"#,
            thousandths(conviction),
            thousandths(confidence),
        );
        let expected = (-1_000..=1_000).contains(&conviction)
            && (0..=1_000).contains(&confidence)
            && (86_400..=2_592_000).contains(&horizon);
        prop_assert_eq!(parse_output(text.as_bytes(), &bounds(), RunKind::Ideate).is_ok(), expected, "{}", text);
    }
}

/// `n / 1000` as canonical decimal text: no trailing zeros, no `-0`.
fn thousandths(n: i64) -> String {
    let sign = if n < 0 { "-" } else { "" };
    let magnitude = n.unsigned_abs();
    let (whole, fraction) = (magnitude / 1_000, magnitude % 1_000);
    if fraction == 0 {
        return format!("{sign}{whole}");
    }
    let digits = format!("{fraction:03}");
    format!("{sign}{whole}.{}", digits.trim_end_matches('0'))
}

#[test]
fn every_bound_admits_its_own_value_and_refuses_one_past_it() {
    let exact = OutputBounds {
        max_output_bytes: u32::try_from(ONE_THESIS.len()).unwrap_or(0),
        ..bounds()
    };
    assert!(parse_output(ONE_THESIS.as_bytes(), &exact, RunKind::Ideate).is_ok());
    let short = OutputBounds {
        max_output_bytes: exact.max_output_bytes - 1,
        ..bounds()
    };
    assert_eq!(
        parse_output(ONE_THESIS.as_bytes(), &short, RunKind::Ideate).map(|_| ()),
        Err(OutputRejection::TooLong)
    );
    let full_text = ONE_THESIS.replace("Services revenue keeps compounding.", &"x".repeat(2_000));
    assert!(
        parse_output(full_text.as_bytes(), &bounds(), RunKind::Ideate).is_ok(),
        "2,000 bytes is in bounds"
    );
    for (length, ok) in [(128, true), (129, false)] {
        let text = ONE_THESIS.replace(r#""n-1""#, &format!("\"{}\"", "e".repeat(length)));
        assert_eq!(
            parse_output(text.as_bytes(), &bounds(), RunKind::Ideate).is_ok(),
            ok,
            "evidence id of {length}"
        );
    }
    let control = ONE_THESIS.replace(r#""n-1""#, r#""n\u0001x""#);
    assert!(matches!(rejected(&control), OutputRejection::Range { .. }));
}

/// §6.9: a `monitor` run's output is exactly `invalidations`, the gateway's schema for the kind.
/// Its invalidations parse; a `theses` member, even empty, is refused, and an ideate run's output
/// still needs both members.
#[test]
fn a_monitor_output_is_invalidations_alone() {
    let monitor = r#"{"invalidations":[{"thesis_id":"01ACTIVE","evidence":["n-1"]}]}"#;
    let output = parse_output(monitor.as_bytes(), &bounds(), RunKind::Monitor)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(output.theses.is_empty());
    assert_eq!(output.invalidations.len(), 1);
    assert_eq!(output.invalidations[0].thesis_id, "01ACTIVE");
    for refused in [
        r#"{"theses":[],"invalidations":[]}"#,
        ONE_THESIS,
        r#"{}"#,
        r#"{"invalidations":[],"note":"x"}"#,
    ] {
        assert_eq!(
            parse_output(refused.as_bytes(), &bounds(), RunKind::Monitor)
                .map(|_| ())
                .map_err(|e| e.code()),
            Err("shape"),
            "{refused}"
        );
    }
    assert_eq!(
        parse_output(monitor.as_bytes(), &bounds(), RunKind::Ideate)
            .map(|_| ())
            .map_err(|e| e.code()),
        Err("shape"),
        "an ideate run's output names both members"
    );
    assert_eq!(
        parse_output(
            r#"{"invalidations":[{"thesis_id":"01A","evidence":[]},{"thesis_id":"01A","evidence":[]}]}"#
                .as_bytes(),
            &bounds(),
            RunKind::Monitor
        )
        .map(|_| ())
        .map_err(|e| e.code()),
        Err("duplicate_invalidation")
    );
}
