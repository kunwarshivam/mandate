//! §11's control-stream checks (journal spec v0.29, E12-3), beginning with `anchor_self_mismatch`
//! (§9.14 rule 113, DEC-783 item 6). `append` cannot see the `seq` it assigns, so the check is not
//! an append rule; verification runs it over the stored rows of one control-stream range, in `seq`
//! order, after [`crate::verify_events`] passed them, and reports its first failing event by `seq`
//! (§9.13 rule 111).

use crate::StoredEvent;

/// §11's control-stream checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlStreamCheck {
    /// An `AnchorComputed`'s leaf for its own control stream is missing, or does not name the
    /// event just before the anchor by `seq` and by `hash`.
    AnchorSelfMismatch,
}

impl ControlStreamCheck {
    /// The check's code as §11 writes it.
    pub fn code(self) -> &'static str {
        match self {
            Self::AnchorSelfMismatch => "anchor_self_mismatch",
        }
    }
}

/// The first failing event, by its `seq`, and the check it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlStreamFailure {
    pub seq: u64,
    pub check: ControlStreamCheck,
}

/// Why a control-stream check did not pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlVerifyError {
    Mismatch(ControlStreamFailure),
    /// The check is not built yet (DEC-77).
    Unimplemented {
        story: &'static str,
    },
}

/// §11's `anchor_self_mismatch` over `rows` of one control-stream range: each `AnchorComputed` has
/// a leaf for its own `stream_id` whose `seq` is the `seq` of the row before it and whose `hash` is
/// that row's `hash`, and the first anchor that has not is reported at its own `seq`. An anchor
/// that is the range's first row names an event before the trusted start, which the range does not
/// check (§11); the full chain checks it.
pub fn verify_anchor_self(rows: &[StoredEvent]) -> Result<(), ControlVerifyError> {
    let _ = rows;
    Err(ControlVerifyError::Unimplemented { story: "E12-3" })
}
