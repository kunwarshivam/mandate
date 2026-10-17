# Metrics and the label lint (E21-3)

- **Spec:** [infrastructure design](../../../../docs/design/infrastructure.md) §8.1 (metrics
  through the OpenTelemetry API with a Prometheus pull exporter, opaque labels only), §2 OPS-11
  (telemetry is never an input; the decision cycle is unchanged with the exporter failing);
  ADR-0001 ES-18; DEC-73; DEC-980.
- **Code:** `crates/mandate-telemetry`. `src/catalog.rs` (the closed metric set: `Metric` with
  private fields, `CATALOG`), `src/label.rs` (the six label keys and their typed values:
  `OpaqueId`, `ProcessType`, `ReasonCode`, `Release`; `Labels`), `src/lint.rs` (`catalog`,
  `metric`, `exposition`, `own`), `src/metrics.rs` (`Metrics`: OpenTelemetry instruments over a
  Prometheus registry; refused records counted; `off`), `src/exporter.rs` (`serve`: `GET /metrics`
  on `std::net`, every scrape linted before it is sent, `503` otherwise). In `crates/mandate-agentd`,
  `src/telemetry.rs` (`Instrumented`: the host's calls counted by outcome after each returns) and
  the binary's `metrics <addr>` loopback exporter.
- **Tests:** `mandate-telemetry`: `tests/lint.rs` (the catalog passes; every forbidden word,
  suffix, prefix, duplicate and label-order defect is a finding; a full scrape passes; undeclared
  keys, scope labels, `target_info`, tickers, prices and numbers as values fail; a property that a
  reason value passes exactly when an independent shape oracle says it is opaque, and that no
  decimal number is an admitted value), `tests/metrics.rs` (counts per label set, refusals counted
  and never exported, kind mismatch, `off`), `tests/exporter.rs` (the wire bytes, `404`, `503`,
  a taken address, a stalled scraper never blocking a record). `mandate-agentd`:
  `tests/telemetry.rs` (a property that a supervisor run's journal, sends, host calls and stop are
  identical with no telemetry, telemetry off, the exporter held by stalled scrapers, and the
  exporter unable to bind; the scrape's counts by outcome), `tests/process.rs` (the binary serves
  its metrics on loopback).
- **Run:** `cargo nextest run -p mandate-telemetry -p mandate-agentd`.
