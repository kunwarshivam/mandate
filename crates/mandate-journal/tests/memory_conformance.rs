//! The append conformance suite (`conformance/mod.rs`) on the in-memory journal. The Postgres
//! journal runs the same suite (`mandate-journal-pg`, E5-3).

mod common;
mod conformance;

use mandate_journal::MemoryJournal;

conformance::conformance_tests! {
    fresh = || Some(MemoryJournal::new())
}
