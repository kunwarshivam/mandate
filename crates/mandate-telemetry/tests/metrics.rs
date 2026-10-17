//! Recording (E21-3): what is recorded is what a scrape shows, a record outside its metric's
//! declared labels or kind is refused and counted, and `off` records and serves nothing.

use std::time::Duration;

use mandate_telemetry::catalog::{BUILD, CATALOG, CYCLES, Metric, RECONCILIATIONS, REFUSED};
use mandate_telemetry::{
    Kind, Labels, Metrics, OpaqueId, ProcessType, Release, TelemetryError, lint, reason,
};

fn metrics() -> Metrics {
    Metrics::new(ProcessType::Executor, &Release::parse("0.4.1").unwrap()).unwrap()
}

fn outcome(process: ProcessType, code: mandate_telemetry::ReasonCode) -> Labels {
    Labels::new().process(process).reason(code)
}

fn sample(text: &str, prefix: &str) -> Option<String> {
    text.lines()
        .find(|line| line.starts_with(prefix))
        .map(str::to_owned)
}

#[test]
fn the_build_is_set_at_start() {
    let text = metrics().encode().unwrap();
    assert_eq!(
        sample(&text, BUILD.name()).as_deref(),
        Some(r#"mandate_process_build_info{process_type="executor",release="0.4.1"} 1"#),
        "{text}"
    );
}

#[test]
fn counts_add_up_per_label_set() {
    let m = metrics();
    for _ in 0..3 {
        m.count(&CYCLES, &outcome(ProcessType::Executor, reason::COMPLETED));
    }
    m.add(&CYCLES, &outcome(ProcessType::Executor, reason::FAILED), 4);
    m.count(
        &RECONCILIATIONS,
        &outcome(ProcessType::Executor, reason::COMPLETED),
    );
    let text = m.encode().unwrap();
    assert!(text.contains(
        r#"mandate_process_cycles_total{process_type="executor",reason_code="completed"} 3"#
    ));
    assert!(text.contains(
        r#"mandate_process_cycles_total{process_type="executor",reason_code="failed"} 4"#
    ));
    assert!(text.contains(
        r#"mandate_process_reconciliations_total{process_type="executor",reason_code="completed"} 1"#
    ));
    assert!(sample(&text, REFUSED.name()).is_none(), "{text}");
}

#[test]
fn a_record_without_its_declared_keys_is_refused_and_counted_never_exported() {
    let m = metrics();
    let id = OpaqueId::parse("ws_zqinstrument").unwrap();
    m.count(&CYCLES, &Labels::new().reason(reason::COMPLETED));
    m.count(
        &CYCLES,
        &outcome(ProcessType::Executor, reason::COMPLETED).workspace(&id),
    );
    m.count(&CYCLES, &Labels::new());
    let text = m.encode().unwrap();
    assert!(!text.contains("mandate_process_cycles_total{"), "{text}");
    assert!(!text.contains("zqinstrument"), "{text}");
    assert_eq!(
        sample(&text, REFUSED.name()).as_deref(),
        Some(r#"mandate_telemetry_refused_total{reason_code="labels_not_declared"} 3"#),
        "{text}"
    );
}

#[test]
fn a_record_offered_to_a_metric_of_another_kind_is_refused() {
    let m = metrics();
    let labels = Labels::new()
        .process(ProcessType::Executor)
        .release(&Release::parse("0.4.1").unwrap());
    m.count(&BUILD, &labels);
    m.duration(&BUILD, &labels, Duration::from_millis(3));
    m.state(
        &CYCLES,
        &outcome(ProcessType::Executor, reason::COMPLETED),
        true,
    );
    let text = m.encode().unwrap();
    assert_eq!(
        sample(&text, REFUSED.name()).as_deref(),
        Some(r#"mandate_telemetry_refused_total{reason_code="kind_mismatch"} 3"#),
        "{text}"
    );
    assert!(text.contains(r#"release="0.4.1"} 1"#), "{text}");
    assert!(!text.contains("mandate_process_cycles_total{"), "{text}");
}

#[test]
fn a_state_can_be_cleared() {
    let m = metrics();
    let labels = Labels::new()
        .process(ProcessType::Executor)
        .release(&Release::parse("0.4.1").unwrap());
    m.state(&BUILD, &labels, false);
    assert!(m.encode().unwrap().contains(r#"release="0.4.1"} 0"#));
}

#[test]
fn every_scrape_passes_the_exposition_lint() {
    let m = metrics();
    let counters: Vec<&Metric> = CATALOG
        .iter()
        .filter(|c| c.kind() == Kind::Counter)
        .collect();
    for metric in counters {
        m.count(metric, &outcome(ProcessType::Executor, reason::FAILED));
        m.count(metric, &Labels::new().reason(reason::FAILED));
    }
    let text = m.encode().unwrap();
    assert_eq!(lint::exposition(&text), Vec::new(), "{text}");
}

#[test]
fn off_records_nothing_and_serves_nothing() {
    let m = Metrics::off();
    m.count(&CYCLES, &outcome(ProcessType::Executor, reason::COMPLETED));
    assert_eq!(m.encode(), Err(TelemetryError::Unavailable));
    assert_eq!(format!("{m:?}"), "Metrics(off)");
    assert_eq!(format!("{:?}", metrics()), "Metrics");
}

#[test]
fn clones_share_one_registry() {
    let m = metrics();
    let twin = m.clone();
    twin.count(&CYCLES, &outcome(ProcessType::Executor, reason::COMPLETED));
    assert!(
        m.encode()
            .unwrap()
            .contains(r#"reason_code="completed"} 1"#)
    );
}

#[test]
fn label_values_take_only_their_own_shape() {
    for good in ["ws_a", "agent_a", "paper_a", "a", &"a".repeat(64)] {
        assert!(OpaqueId::parse(good).is_ok(), "{good}");
    }
    for bad in [
        "",
        "AAPL",
        "aapl.us",
        "1abc",
        "_a",
        "a-b",
        "a b",
        &"a".repeat(65),
        "été",
    ] {
        assert_eq!(
            OpaqueId::parse(bad),
            Err(TelemetryError::NotOpaque),
            "{bad}"
        );
    }
    for good in ["0.0.0", "0.4.1", "abc123f", "1.2.3-rc.1", "v1"] {
        assert!(Release::parse(good).is_ok(), "{good}");
    }
    for bad in ["", "12", "101.25", "-1", ".1.2", "V1", &"a".repeat(65)] {
        assert_eq!(Release::parse(bad), Err(TelemetryError::NotOpaque), "{bad}");
    }
    for process in ProcessType::ALL {
        assert_eq!(ProcessType::parse(process.key()), Some(process));
    }
    assert_eq!(ProcessType::parse("broker"), None);
}

#[test]
fn labels_hold_one_value_per_key_in_key_order() {
    let a = OpaqueId::parse("agent_a").unwrap();
    let b = OpaqueId::parse("agent_b").unwrap();
    let labels = Labels::new()
        .reason(reason::FAILED)
        .agent(&a)
        .process(ProcessType::Scheduler)
        .agent(&b);
    let pairs: Vec<(&str, &str)> = labels.pairs().collect();
    assert_eq!(
        pairs,
        vec![
            ("agent", "agent_b"),
            ("process_type", "scheduler"),
            ("reason_code", "failed")
        ]
    );
}
