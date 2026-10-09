//! §11's control-stream checks (journal spec v0.29, E12-3): `anchor_self_mismatch` (§9.14 rule
//! 113, DEC-783 item 6) and `break_glass_cause_mismatch` (§9.13 rule 108, DEC-774 item 2). `append`
//! cannot see the `seq` it assigns or what an earlier batch holds, so neither is an append rule;
//! verification runs each over the stored rows of one control-stream range, in `seq` order, after
//! [`crate::verify_events`] passed them, and each reports its first failing event by `seq` (§9.13
//! rule 111).

use std::collections::BTreeMap;

use mandate_canon::{Digest, Value, parse};

use crate::{StoredEvent, TrustedStart};

/// §11's control-stream checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlStreamCheck {
    /// An `AnchorComputed`'s leaf for its own control stream is missing, or does not name the
    /// event just before the anchor by `seq` and by `hash`.
    AnchorSelfMismatch,
    /// A `platform_operator`'s `RecordsAccessed` does not name, as its `causation_id`, an earlier
    /// `PlatformOperatorAction` on the same control stream.
    BreakGlassCauseMismatch,
}

impl ControlStreamCheck {
    /// The check's code as §11 writes it.
    pub fn code(self) -> &'static str {
        match self {
            Self::AnchorSelfMismatch => "anchor_self_mismatch",
            Self::BreakGlassCauseMismatch => "break_glass_cause_mismatch",
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
    for pair in rows.windows(2) {
        let [before, row] = pair else { continue };
        if row.event_type != "AnchorComputed" {
            continue;
        }
        let body = parse(&row.body).ok();
        let leaves = body
            .as_ref()
            .and_then(|b| b.get("payload"))
            .and_then(|p| p.get("leaves"))
            .and_then(Value::as_array)
            .unwrap_or_default();
        let own = leaves.iter().find(|leaf| {
            leaf.get("stream_id").and_then(Value::as_str) == Some(row.stream_id.as_str())
        });
        let names_before = own.is_some_and(|leaf| {
            leaf.get("seq").and_then(Value::as_int) == Some(before.seq)
                && leaf
                    .get("hash")
                    .and_then(Value::as_str)
                    .and_then(Digest::from_hex)
                    == Some(before.hash)
        });
        if !names_before {
            return Err(ControlVerifyError::Mismatch(ControlStreamFailure {
                seq: row.seq,
                check: ControlStreamCheck::AnchorSelfMismatch,
            }));
        }
    }
    Ok(())
}

/// §11's `break_glass_cause_mismatch` over `rows` of one control-stream range entered at `start`:
/// every `RecordsAccessed` whose actor is a `platform_operator` names, as its `causation_id`, a
/// `PlatformOperatorAction` of the same `stream_id` at a lower `seq`, and the first read that does
/// not is reported at its own `seq`. A cause that names a later event or one of another type fails;
/// a cause named by no row fails only when `start.from_seq` is 1, since in a later range it may lie
/// before the trusted start, which the full chain judges. Reads by any other actor are not judged.
pub fn verify_break_glass_causes(
    rows: &[StoredEvent],
    start: TrustedStart,
) -> Result<(), ControlVerifyError> {
    let named: BTreeMap<&str, &StoredEvent> =
        rows.iter().map(|r| (r.event_id.as_str(), r)).collect();
    for row in rows {
        if row.event_type != "RecordsAccessed" {
            continue;
        }
        let Ok(body) = parse(&row.body) else {
            return Err(break_glass_mismatch(row));
        };
        let kind = body
            .get("actor")
            .and_then(|a| a.get("kind"))
            .and_then(Value::as_str);
        if kind != Some("platform_operator") {
            continue;
        }
        let cause = body
            .get("causation_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let opened = match named.get(cause) {
            None => start.from_seq != 1,
            Some(action) => {
                action.event_type == "PlatformOperatorAction"
                    && action.seq < row.seq
                    && action.stream_id == row.stream_id
            }
        };
        if !opened {
            return Err(break_glass_mismatch(row));
        }
    }
    Ok(())
}

/// The failure an operator read reports, at its own `seq`; a read whose body does not parse is
/// reported too, since no actor or cause can be shown for it.
fn break_glass_mismatch(row: &StoredEvent) -> ControlVerifyError {
    ControlVerifyError::Mismatch(ControlStreamFailure {
        seq: row.seq,
        check: ControlStreamCheck::BreakGlassCauseMismatch,
    })
}
