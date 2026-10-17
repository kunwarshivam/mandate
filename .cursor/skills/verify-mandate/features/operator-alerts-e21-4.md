# Operator alerts from journal events and signals (E21-4)

- **Spec:** [infrastructure design](../../../../docs/design/infrastructure.md) §8.2 (the alert
  table: source, severity, runbook), §2 OPS-10 (operator alerts carry opaque IDs and generic text
  only); [notifications spec](../../../../docs/specs/notifications.md) NT-1, NT-4; DEC-1070.
- **Code:** `mandate-alerts` (`crates/mandate-alerts`). `src/kind.rs` (`AlertKind`, one per §8.2 row, with its reason
  code, `Severity`, runbook and fixed text), `src/event.rs` (`from_event`: the journal-sourced
  rows from a `StoredEvent`'s payload), `src/signal.rs` (`Signal`, `from_signal`: the rows sourced
  from metrics and other services, with §8.2's thresholds), `src/payload.rs` (`Alert`, minted from
  `mandate-notify`'s `SecureRandom`; `payload`, `rendered`, the `Sink` trait, `CaptureSink`,
  `send`).
- **Tests:** `tests/alerts.rs`:
  - the §8.2 table, typed independently, matches every kind's severity and runbook;
  - every kind fires from its fixture, with and without canary members;
  - records and signals that are not alerts raise nothing, and thresholds hold at their bounds;
  - the kill switch is SEV-1 only when automated at workspace scope;
  - an unreadable body, or one missing its discriminating member, is an error;
  - the payload is the five members and nothing else;
  - ids are fresh and none is raised without entropy;
  - a payload capture of every alert finds no `mandate-notify` canary and no event id;
  - a property that whatever else a record holds never reaches the payload.
- **Run:** `cargo nextest run -p mandate-alerts`.
