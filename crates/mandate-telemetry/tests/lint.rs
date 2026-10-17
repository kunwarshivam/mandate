//! The label lint (E21-3, design §8.1): the catalog passes it, a seeded bad declaration fails it,
//! and a scrape holding anything outside the allowed set fails it.

use mandate_telemetry::catalog::{self, CATALOG};
use mandate_telemetry::lint::{self, FORBIDDEN_WORDS, Finding};
use mandate_telemetry::{Kind, LabelKey, Labels, Metrics, ProcessType, Release, reason};
use proptest::prelude::*;

fn metrics() -> Metrics {
    Metrics::new(ProcessType::AgentProcess, &Release::parse("0.0.0").unwrap()).unwrap()
}

/// A scrape after every catalog metric has been recorded once.
fn full_scrape() -> String {
    let m = metrics();
    let labels = Labels::new()
        .process(ProcessType::AgentProcess)
        .reason(reason::COMPLETED);
    for metric in &CATALOG {
        match metric.kind() {
            Kind::Counter if metric.labels() == [LabelKey::Reason] => {
                m.count(metric, &Labels::new().reason(reason::COMPLETED));
            }
            Kind::Counter => m.count(metric, &labels),
            Kind::State => {}
        }
    }
    m.encode().unwrap()
}

#[test]
fn the_crates_own_catalog_and_reason_codes_pass() {
    assert_eq!(lint::catalog(&CATALOG), Vec::new());
    assert_eq!(lint::reasons(&reason::ALL), Vec::new());
    assert!(!CATALOG.is_empty());
}

#[test]
fn every_catalog_metric_is_found_by_its_name() {
    for metric in &CATALOG {
        assert_eq!(catalog::find(metric.name()), Some(metric));
    }
    assert_eq!(catalog::find("mandate_unknown_total"), None);
}

#[test]
fn a_name_outside_the_prefix_or_charset_is_a_finding() {
    for name in [
        "cycles_total",
        "mandate_",
        "mandate_Cycles_total",
        "mandate_a-b_total",
    ] {
        let findings = lint::metric(name, "Counts.", Kind::Counter, &[]);
        assert!(
            findings.contains(&Finding::Name {
                metric: name.to_owned()
            }),
            "{name}: {findings:?}"
        );
    }
}

#[test]
fn every_forbidden_word_in_a_name_or_its_help_is_a_finding() {
    for word in FORBIDDEN_WORDS {
        let name = format!("mandate_order_{word}_total");
        let findings = lint::metric(&name, "Counts.", Kind::Counter, &[]);
        assert_eq!(findings, vec![Finding::Forbidden { metric: name, word }]);
        let help = format!("Counts each {} seen.", word.to_ascii_uppercase());
        let findings = lint::metric("mandate_orders_total", &help, Kind::Counter, &[]);
        assert_eq!(
            findings,
            vec![Finding::Forbidden {
                metric: "mandate_orders_total".to_owned(),
                word
            }]
        );
    }
}

#[test]
fn a_suffix_that_does_not_match_the_kind_is_a_finding() {
    let cases = [
        ("mandate_cycles", Kind::Counter),
        ("mandate_ready_total", Kind::State),
    ];
    for (name, kind) in cases {
        assert_eq!(
            lint::metric(name, "Counts.", kind, &[]),
            vec![Finding::Suffix {
                metric: name.to_owned()
            }],
            "{name}"
        );
    }
    for (name, kind) in [
        ("mandate_cycles_total", Kind::Counter),
        ("mandate_ready", Kind::State),
    ] {
        assert_eq!(
            lint::metric(name, "Counts.", kind, &[]),
            Vec::new(),
            "{name}"
        );
    }
}

#[test]
fn label_keys_declared_twice_or_out_of_order_are_a_finding() {
    for labels in [
        [LabelKey::Reason, LabelKey::Reason],
        [LabelKey::Reason, LabelKey::Process],
    ] {
        assert_eq!(
            lint::metric("mandate_cycles_total", "Counts.", Kind::Counter, &labels),
            vec![Finding::Labels {
                metric: "mandate_cycles_total".to_owned()
            }]
        );
    }
}

#[test]
fn a_name_used_twice_in_a_catalog_is_a_finding() {
    let twice = [catalog::CYCLES, catalog::SETTLES, catalog::CYCLES];
    assert_eq!(
        lint::catalog(&twice),
        vec![Finding::Duplicate {
            metric: catalog::CYCLES.name().to_owned()
        }]
    );
}

#[test]
fn a_full_scrape_passes_the_exposition_lint() {
    let text = full_scrape();
    assert!(text.contains("mandate_process_cycles_total{"), "{text}");
    for metric in &CATALOG {
        if metric.kind() == Kind::Counter {
            let help = format!("# HELP {} {}\n", metric.name(), metric.help());
            assert!(text.contains(&help), "{help} in {text}");
        }
    }
    assert_eq!(lint::exposition(&text), Vec::new());
}

fn cycles_line(labels: &str) -> String {
    format!("mandate_process_cycles_total{{{labels}}} 1\n")
}

#[test]
fn a_scrape_with_anything_outside_the_allowed_set_fails() {
    let undeclared = |key: &str| Finding::Undeclared {
        metric: "mandate_process_cycles_total".to_owned(),
        key: key.to_owned(),
    };
    let value = |key: &'static str| Finding::Value {
        metric: "mandate_process_cycles_total".to_owned(),
        key,
    };
    let unknown = |metric: &str| Finding::Unknown {
        metric: metric.to_owned(),
    };
    let cases: Vec<(String, Vec<Finding>)> = vec![
        (
            cycles_line(r#"process_type="agent_process",symbol="aapl""#),
            vec![undeclared("symbol")],
        ),
        (
            cycles_line(r#"process_type="agent_process",otel_scope_name="mandate""#),
            vec![undeclared("otel_scope_name")],
        ),
        (
            cycles_line(r#"process_type="agent_process",workspace="ws_a""#),
            vec![undeclared("workspace")],
        ),
        (
            cycles_line(r#"process_type="aapl""#),
            vec![value("process_type")],
        ),
        (
            cycles_line(r#"reason_code="AAPL""#),
            vec![value("reason_code")],
        ),
        (
            cycles_line(r#"reason_code="101.25""#),
            vec![value("reason_code")],
        ),
        (
            cycles_line(r#"reason_code="12""#),
            vec![value("reason_code")],
        ),
        (
            cycles_line(r#"reason_code="a\"b""#),
            vec![value("reason_code")],
        ),
        (
            cycles_line(r#"reason_code="a\nb""#),
            vec![value("reason_code")],
        ),
        (
            cycles_line(r#"reason_code="a\\b""#),
            vec![value("reason_code")],
        ),
        (
            cycles_line(r#"reason_code="a\tb""#),
            vec![Finding::Malformed { line: 1 }],
        ),
        (
            cycles_line(r#"process_type="executor"reason_code="failed""#),
            vec![Finding::Malformed { line: 1 }],
        ),
        (
            cycles_line(r#"Reason_code="failed""#),
            vec![Finding::Malformed { line: 1 }],
        ),
        (
            cycles_line(r#"="failed""#),
            vec![Finding::Malformed { line: 1 }],
        ),
        (
            "target_info{service_name=\"x\"} 1\n".to_owned(),
            vec![unknown("target_info")],
        ),
        (
            "# HELP mandate_order_price_total x\n".to_owned(),
            vec![unknown("mandate_order_price_total")],
        ),
        (
            "# HELP mandate_process_cycles_total Prices seen.\n".to_owned(),
            vec![Finding::Header {
                metric: "mandate_process_cycles_total".to_owned(),
            }],
        ),
        (
            "# TYPE mandate_process_cycles_total gauge\n".to_owned(),
            vec![Finding::Header {
                metric: "mandate_process_cycles_total".to_owned(),
            }],
        ),
        (
            "mandate_process_cycles_total_bucket{le=\"1\"} 1\n".to_owned(),
            vec![unknown("mandate_process_cycles_total_bucket")],
        ),
        ("# EOF\n".to_owned(), vec![Finding::Malformed { line: 1 }]),
        (
            "\nmandate_process_cycles_total{reason_code=\"x\" 1\n".to_owned(),
            vec![Finding::Malformed { line: 2 }],
        ),
        (
            cycles_line(r#"reason_code="completed""#).replace(" 1", " one"),
            vec![Finding::Malformed { line: 1 }],
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(lint::exposition(&text), expected, "{text}");
    }
}

#[test]
fn a_sample_with_no_labels_or_only_declared_ones_passes() {
    for text in [
        "mandate_telemetry_refused_total 3\n".to_owned(),
        cycles_line(r#"process_type="executor",reason_code="failed""#),
        "# TYPE mandate_process_cycles_total counter\n".to_owned(),
    ] {
        assert_eq!(lint::exposition(&text), Vec::new(), "{text}");
    }
}

/// The label-value shape, written here from design §8.1 and DEC-710 item 4 rather than from the
/// crate: a lowercase letter, then lowercase letters, digits and underscores, 64 bytes at most.
fn opaque(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

proptest! {
    #[test]
    fn a_reason_value_passes_exactly_when_it_is_opaque(
        value in prop_oneof!["[ -~]{0,70}", "[a-z_\\\\\"\n\r\té€0-9]{0,70}"]
    ) {
        let escaped = value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        let text = cycles_line(&format!(r#"reason_code="{escaped}""#));
        prop_assert_eq!(lint::exposition(&text).is_empty(), opaque(&value), "{}", text);
    }

    #[test]
    fn no_decimal_number_is_ever_an_admitted_value(int in 0u32..100_000, frac in 0u32..100) {
        for value in [int.to_string(), format!("{int}.{frac:02}"), format!("-{int}")] {
            for key in [LabelKey::Workspace, LabelKey::Agent, LabelKey::Account, LabelKey::Reason,
                        LabelKey::Process, LabelKey::Release] {
                let release_version = key == LabelKey::Release && value.matches('.').count() >= 2;
                prop_assert!(release_version || !key.admits(&value), "{:?} {}", key, value);
            }
        }
    }
}
