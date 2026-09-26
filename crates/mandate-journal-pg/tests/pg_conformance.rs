//! The append conformance suite on the Postgres journal: the same tests, from the same file, that
//! `MemoryJournal` passes (`crates/mandate-journal/tests/memory_conformance.rs`).

#[path = "../../mandate-journal/tests/common/mod.rs"]
mod common;
#[path = "../../mandate-journal/tests/conformance/mod.rs"]
mod conformance;
mod support;

conformance::conformance_tests! {
    fresh = support::PgBackend::fresh
}
