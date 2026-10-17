//! E21-29 (DEC-1010): the scheduler stream's five records journal spec v0.47 §9.20 closes, with
//! rules 159 to 164. The expected reasons and paths are written out from §9.20, not read from the
//! journal's own schema.
//!
//! A case is one line: `base | edits | expected`. `base` names a draft below; `edits` are
//! space-separated `member=value` pairs on its payload (`^member` for the envelope), where a value
//! that is not JSON is a bare string, and one of digits, `-`, and `.` is a decimal string; `expected` is `ok` or `reason@path`.

mod common;

use common::{T, edit, event_id};
use mandate_canon::{Value, parse};
use mandate_journal::Draft;
use proptest::prelude::*;

const CLOCK: &str = "clock:ws_1";
const STREAMS: [&str; 4] = ["acct:ws_1:ACCT1", "agent:ws_1:agent_a", "ctl:ws_1", CLOCK];
const RECORDS: [&str; 5] = [
    "StreamOpened",
    "ClockAdvanced",
    "TradingDayStarted",
    "ClockOffsetRecorded",
    "ClockToleranceExceeded",
];

/// Three answering servers whose offsets, by magnitude, are 0.001, 0.002, and 0.003: the median
/// by magnitude is `0.002`.
const SAMPLES: &str = r#"[{"server":"a.time","stratum":2,"offset":"0.003","delay":"0.02"},{"server":"b.time","stratum":1,"offset":"-0.001","delay":"0.01"},{"server":"c.time","stratum":3,"offset":"0.002","delay":"0.03"}]"#;

fn envelope(stream: &str, event_type: &str, causation: &str, payload: &str) -> Vec<u8> {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"{stream}",
        "event_type":"{event_type}","schema_version":1,"event_time":"{T}","clock_source":"scheduler",
        "causation_id":{causation},"correlation_id":null,
        "actor":{{"kind":"system","id":"scheduler","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#,
        event_id(7),
        "3".repeat(64)
    )
    .into_bytes()
}

/// The base draft `name`: `opened`, `advanced`, `day`, `measured`, or `exceeded`.
fn base(name: &str) -> Vec<u8> {
    let cause = format!("\"{}\"", event_id(6));
    let (event_type, causation, payload) = match name {
        "opened" => (
            "StreamOpened",
            "null".to_owned(),
            r#"{"stream_type":"scheduler","workspace_id":"ws_1"}"#.to_owned(),
        ),
        "advanced" => (
            "ClockAdvanced",
            "null".to_owned(),
            format!(r#"{{"clock":"{T}"}}"#),
        ),
        "day" => (
            "TradingDayStarted",
            "null".to_owned(),
            r#"{"date":"2026-09-21"}"#.to_owned(),
        ),
        "measured" => (
            "ClockOffsetRecorded",
            "null".to_owned(),
            format!(
                r#"{{"trigger":"periodic","samples":{SAMPLES},"offset":"0.002","tolerance":"0.05","within_tolerance":true}}"#
            ),
        ),
        _ => (
            "ClockToleranceExceeded",
            cause,
            r#"{"offset":"0.2","tolerance":"0.05"}"#.to_owned(),
        ),
    };
    envelope(CLOCK, event_type, &causation, &payload)
}

/// `draft` with each `member=value` edit of `edits` applied (see the module docs).
fn set(draft: &[u8], edits: &str) -> Vec<u8> {
    edits.split_whitespace().fold(draft.to_vec(), |d, pair| {
        let (member, value) = pair.split_once('=').unwrap();
        let decimal = value
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'-' || b == b'.');
        let json = if decimal {
            format!("\"{value}\"")
        } else {
            parse(value.as_bytes()).map_or_else(|_| format!("\"{value}\""), |_| value.into())
        };
        let path = member
            .strip_prefix('^')
            .map_or_else(|| format!("payload.{member}"), str::to_owned);
        edit(&d, &path, Some(&json))
    })
}

/// `ok`, or the refusal as `reason@path`.
fn outcome(draft: &[u8]) -> String {
    Draft::parse(draft).map_or_else(
        |e| format!("{}@{}", e.reason.code(), e.path),
        |_| "ok".into(),
    )
}

fn run(cases: &[&str]) {
    let failed: Vec<String> = cases
        .iter()
        .filter_map(|case| {
            let [name, edits, want] = case.split('|').map(str::trim).collect::<Vec<_>>()[..] else {
                panic!("`{case}` is not `base | edits | expected`")
            };
            let got = outcome(&set(&base(name), edits));
            (got != want).then(|| format!("{case}: got {got}"))
        })
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// One sample per `(server, offset)`, at stratum 2 with a delay of 0.01 seconds.
fn samples(of: &[(&str, &str)]) -> String {
    let each: Vec<String> = of
        .iter()
        .map(|(server, offset)| {
            format!(r#"{{"server":"{server}","stratum":2,"offset":"{offset}","delay":"0.01"}}"#)
        })
        .collect();
    format!("[{}]", each.join(","))
}

#[test]
fn every_base_record_is_admitted_on_the_scheduler_stream() {
    for name in ["opened", "advanced", "day", "measured", "exceeded"] {
        assert_eq!(outcome(&base(name)), "ok", "{name}");
    }
}

#[test]
fn every_member_is_required_and_no_unlisted_member_is_admitted() {
    for name in ["opened", "advanced", "day", "measured", "exceeded"] {
        let draft = base(name);
        let Some(Value::Object(members)) = parse(&draft).unwrap().get("payload").cloned() else {
            panic!("a payload is an object")
        };
        for member in members.keys() {
            let path = format!("payload.{}", member.as_str());
            assert_eq!(
                outcome(&edit(&draft, &path, None)),
                format!("schema@{path}"),
                "{name}"
            );
        }
        assert_eq!(
            outcome(&set(&draft, "zz_unlisted=true")),
            "schema@payload.zz_unlisted",
            "{name}"
        );
    }
    let sample = r#"[{"server":"a.time","stratum":2,"offset":"0.002","delay":"0.01","zz":1}]"#;
    run(&[&format!(
        "measured | samples={sample} offset=0.002 | schema@payload.samples[0].zz"
    )]);
}

#[test]
fn the_records_are_closed_at_version_1_on_the_scheduler_stream_only() {
    for (record, name) in RECORDS
        .iter()
        .zip(["opened", "advanced", "day", "measured", "exceeded"])
    {
        let version_2 = edit(&base(name), "schema_version", Some("2"));
        assert_eq!(outcome(&version_2), "unknown_schema@payload", "{record}");
    }
    for record in ["ClockOffsetRecorded", "ClockToleranceExceeded"] {
        for stream in &STREAMS[..3] {
            let draft = envelope(stream, record, "null", r#"{"unregistered":true}"#);
            assert_eq!(
                outcome(&draft),
                "wrong_stream@event_type",
                "{record} on {stream}"
            );
        }
    }
}

#[test]
fn member_types_are_checked_before_any_rule() {
    run(&[
        "opened | stream_type=agent | non_canonical@payload.stream_type",
        "opened | workspace_id=ws.1 | non_canonical@payload.workspace_id",
        "advanced | clock=2026-09-21T14:00:00.500000000Z | non_canonical@payload.clock",
        "advanced | clock=true | schema@payload.clock",
        "day | date=2026-13-01 | non_canonical@payload.date",
        "measured | trigger=hourly | non_canonical@payload.trigger",
        "measured | offset=abc | non_canonical@payload.offset",
        "measured | tolerance=null | schema@payload.tolerance",
        "measured | within_tolerance=yes | schema@payload.within_tolerance",
        "measured | samples=x | schema@payload.samples",
        r#"measured | samples=[{"server":"","stratum":2,"offset":"0","delay":"0"}] offset=0 | non_canonical@payload.samples[0].server"#,
        r#"measured | samples=[{"server":"a","stratum":"2","offset":"0","delay":"0"}] tolerance=0 | schema@payload.samples[0].stratum"#,
        "exceeded | offset=1.0.0 | non_canonical@payload.offset",
        "exceeded | tolerance=x | non_canonical@payload.tolerance",
    ]);
}

#[test]
fn rule_159_the_scheduler_writes_as_a_system_actor_on_its_clock() {
    let user = r#"{"kind":"user","id":"u_1","version":"1","build":null}"#;
    for name in ["opened", "advanced", "day", "measured", "exceeded"] {
        run(&[
            &format!("{name} | ^clock_source=local | schema@clock_source"),
            &format!("{name} | ^clock_source=broker | schema@clock_source"),
            &format!("{name} | ^actor={user} | schema@actor.kind"),
        ]);
    }
}

#[test]
fn rule_160_the_stream_opened_names_its_own_stream() {
    run(&[
        "opened | workspace_id=ws_2 | stream_mismatch@stream_id",
        "opened | workspace_id=ws_1 | ok",
    ]);
}

#[test]
fn rule_161_a_tolerance_strata_delays_and_sorted_servers() {
    let unsorted = samples(&[("b.time", "0.001"), ("a.time", "0.002")]);
    let repeated = samples(&[("a.time", "0.001"), ("a.time", "0.002")]);
    let stratum =
        |s: u8| format!(r#"[{{"server":"a","stratum":{s},"offset":"0.001","delay":"0.01"}}]"#);
    run(&[
        "measured | tolerance=0 | schema@payload.tolerance",
        "measured | tolerance=-0.05 | schema@payload.tolerance",
        "measured | tolerance=0.000000001 within_tolerance=false | ok",
        &format!(
            "measured | samples={} offset=0.001 | schema@payload.samples[0].stratum",
            stratum(0)
        ),
        &format!(
            "measured | samples={} offset=0.001 | schema@payload.samples[0].stratum",
            stratum(16)
        ),
        &format!("measured | samples={} offset=0.001 | ok", stratum(1)),
        &format!("measured | samples={} offset=0.001 | ok", stratum(15)),
        r#"measured | samples=[{"server":"a","stratum":2,"offset":"0","delay":"-0.01"}] offset=0 | schema@payload.samples[0].delay"#,
        r#"measured | samples=[{"server":"a","stratum":2,"offset":"0","delay":"0"}] offset=0 | ok"#,
        &format!("measured | samples={unsorted} offset=0.002 | non_canonical@payload.samples"),
        &format!("measured | samples={repeated} offset=0.002 | non_canonical@payload.samples"),
        "measured | tolerance=0 within_tolerance=false | schema@payload.tolerance",
    ]);
}

#[test]
fn rule_162_the_offset_is_the_median_by_magnitude_and_of_two_the_larger() {
    let two = samples(&[("a", "0.1"), ("b", "0.01")]);
    let opposite = samples(&[("a", "0.05"), ("b", "-0.05")]);
    let four = samples(&[("a", "-0.4"), ("b", "0.1"), ("c", "-0.2"), ("d", "0.3")]);
    run(&[
        "measured | offset=0.003 | schema@payload.offset",
        "measured | offset=-0.002 | schema@payload.offset",
        "measured | offset=null within_tolerance=false | schema@payload.offset",
        "measured | offset=0.0020 | ok",
        "measured | samples=[] offset=null within_tolerance=false | ok",
        "measured | samples=[] offset=0 within_tolerance=false | schema@payload.offset",
        &format!("measured | samples={two} offset=0.1 within_tolerance=false | ok"),
        &format!(
            "measured | samples={two} offset=0.01 within_tolerance=true | schema@payload.offset"
        ),
        &format!("measured | samples={opposite} offset=0.05 | ok"),
        &format!("measured | samples={opposite} offset=-0.05 | schema@payload.offset"),
        &format!("measured | samples={four} offset=0.3 within_tolerance=false | ok"),
        &format!(
            "measured | samples={four} offset=-0.2 within_tolerance=false | schema@payload.offset"
        ),
    ]);
}

#[test]
fn rule_163_within_tolerance_only_with_an_offset_at_most_the_tolerance() {
    let at = |offset: &str| samples(&[("a", offset)]);
    run(&[
        "measured | within_tolerance=false | schema@payload.within_tolerance",
        "measured | samples=[] offset=null within_tolerance=true | schema@payload.within_tolerance",
        &format!("measured | samples={} offset=0.05 | ok", at("0.05")),
        &format!("measured | samples={} offset=-0.05 | ok", at("-0.05")),
        &format!(
            "measured | samples={} offset=0.050000001 | schema@payload.within_tolerance",
            at("0.050000001")
        ),
        &format!(
            "measured | samples={} offset=-0.050000001 within_tolerance=false | ok",
            at("-0.050000001")
        ),
        &format!(
            "measured | samples={} offset=12 within_tolerance=false | ok",
            at("12")
        ),
        &format!(
            "measured | samples={} offset=0.9 tolerance=1 | ok",
            at("0.9")
        ),
        &format!(
            "measured | samples={} offset=10 tolerance=9.99 within_tolerance=false | ok",
            at("10")
        ),
    ]);
}

#[test]
fn rule_164_an_exceeded_tolerance_is_out_of_tolerance_and_names_its_measurement() {
    run(&[
        "exceeded | tolerance=0 | schema@payload.tolerance",
        "exceeded | offset=0.05 | schema@payload.offset",
        "exceeded | offset=-0.05 | schema@payload.offset",
        "exceeded | offset=0.01 | schema@payload.offset",
        "exceeded | offset=-0.2 | ok",
        "exceeded | offset=0.050000001 | ok",
        "exceeded | offset=null | ok",
        "exceeded | ^causation_id=null | schema@causation_id",
        "exceeded | tolerance=0 ^causation_id=null | schema@payload.tolerance",
    ]);
}

/// A decimal string of `nanos` nanoseconds in seconds, normalized as §4.6 writes it, built from
/// integers alone.
fn seconds(nanos: i64) -> String {
    let sign = if nanos < 0 { "-" } else { "" };
    let magnitude = nanos.unsigned_abs();
    let (whole, part) = (magnitude / 1_000_000_000, magnitude % 1_000_000_000);
    let fraction = format!("{part:09}");
    let fraction = fraction.trim_end_matches('0');
    if part == 0 {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{fraction}")
    }
}

proptest! {
    /// The oracle sorts integer nanoseconds by magnitude, then by value, and takes index n / 2;
    /// it judges tolerance on integers. The journal compares decimal strings digit by digit.
    #[test]
    fn rules_162_and_163_agree_with_an_integer_oracle(
        offsets in prop::collection::vec(-200_000_000i64..200_000_000, 1..6),
        tolerance in 1i64..100_000_000,
        other in 0usize..6,
    ) {
        let servers: Vec<String> = (0..offsets.len()).map(|i| format!("s{i}")).collect();
        let pairs: Vec<(String, String)> = servers
            .iter()
            .zip(&offsets)
            .map(|(s, n)| (s.clone(), seconds(*n)))
            .collect();
        let borrowed: Vec<(&str, &str)> =
            pairs.iter().map(|(s, o)| (s.as_str(), o.as_str())).collect();
        let list = samples(&borrowed);
        let mut sorted = offsets.clone();
        sorted.sort_by_key(|n| (n.unsigned_abs(), *n));
        let expected = sorted[sorted.len() / 2];
        let within = expected.unsigned_abs() <= tolerance.unsigned_abs();
        let draft = |offset: i64, within: bool| {
            set(
                &base("measured"),
                &format!(
                    "samples={list} offset={} tolerance={} within_tolerance={within}",
                    seconds(offset),
                    seconds(tolerance)
                ),
            )
        };
        prop_assert_eq!(outcome(&draft(expected, within)), "ok");
        prop_assert_eq!(
            outcome(&draft(expected, !within)),
            "schema@payload.within_tolerance"
        );
        let wrong = offsets[other % offsets.len()];
        if wrong != expected {
            prop_assert_eq!(outcome(&draft(wrong, within)), "schema@payload.offset");
        }
    }
}
