//! The backtest and evaluation reads (§4.2, §4.7; DEC-1001 item 5, DEC-1002 items 6 and 7,
//! DEC-1143), folded from the control stream's rows alone (API-14).
//!
//! - A backtest is the one `BacktestRunRecorded` (journal spec §9.18) whose `payload.backtest_id`
//!   equals the id asked for, by equality. `result: completed` serves `state: completed` with its
//!   report; `refused` serves `state: refused` with its code. Nothing journals a running backtest
//!   yet (`POST /backtests` is not built), so `running` is never served: a backtest is absent until
//!   its record commits.
//! - An evaluation is the one `EvaluationRunRecorded` whose `payload.evaluation_id` equals the id.
//!   Whether it is served at all is the route's: only in an internal paper workspace (DEC-1143).
//! - A record that breaks §9.18's shape, or a second record with the same id, is
//!   [`Found::Unreadable`]: the journal holds no single answer, so the read claims none, neither
//!   the first, the last, nor an absence (DEC-1143 item 3).
//!
//! Every function here is pure: rows and bytes in, values out. The store is read by the route.

use mandate_canon::Value;
use mandate_journal::{ArtifactRef, StoredEvent};

/// The backtest's record (journal spec §9.18).
pub const BACKTEST_RECORDED: &str = "BacktestRunRecorded";

/// The evaluation's record (journal spec §9.18).
pub const EVALUATION_RECORDED: &str = "EvaluationRunRecorded";

/// The one environment an evaluation is served in (DEC-103).
pub const PAPER: &str = "paper";

/// What the control stream holds for one id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found<T> {
    /// No record names the id: the read's 404.
    Absent,
    /// Exactly one record names it, and it has §9.18's shape.
    One(T),
    /// A record names it but breaks §9.18's shape, or more than one does: the read's 503.
    Unreadable,
    /// The fold is not built yet.
    Unimplemented { story: &'static str },
}

/// How a recorded backtest ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// `result: completed`: the report artifact it names.
    Completed { report_ref: ArtifactRef },
    /// `result: refused`: the run's stable reason code.
    Refused { refusal: String },
}

/// One recorded backtest, as `read-models/backtest` states it before its report is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacktestView {
    pub backtest_id: String,
    pub mandate_version: String,
    pub outcome: Outcome,
}

impl BacktestView {
    /// The wire name of its `state`.
    pub fn state(&self) -> &'static str {
        match self.outcome {
            Outcome::Completed { .. } => "completed",
            Outcome::Refused { .. } => "refused",
        }
    }
}

/// Why the bytes the store holds for a report are not served.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unserved {
    /// Not a JSON object of the report's `kind` and `report_version`.
    NotAReport,
    /// A backtest report of another mandate version than its record names.
    OtherVersion,
    /// The check is not built yet.
    Unimplemented { story: &'static str },
}

/// One recorded evaluation, before its report is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationView {
    pub evaluation_id: String,
    pub report_ref: ArtifactRef,
}

/// The backtest the control stream records under `backtest_id`.
pub fn backtest(control: &[StoredEvent], backtest_id: &str) -> Found<BacktestView> {
    let _ = (control, backtest_id);
    Found::Unimplemented { story: "E4-5" }
}

/// The evaluation the control stream records under `evaluation_id`.
pub fn evaluation(control: &[StoredEvent], evaluation_id: &str) -> Found<EvaluationView> {
    let _ = (control, evaluation_id);
    Found::Unimplemented { story: "E17-8" }
}

/// The backtest report `bytes` hold, when it is a JSON object of `report_version` 2 for the version
/// `view` backtested.
///
/// # Errors
/// [`Unserved`]: anything else is never served.
pub fn backtest_report(bytes: &[u8], view: &BacktestView) -> Result<Value, Unserved> {
    let _ = (bytes, view);
    Err(Unserved::Unimplemented { story: "E4-5" })
}

/// The evaluation report `bytes` hold, when it is a JSON object of `report_version` 1.
///
/// # Errors
/// [`Unserved::NotAReport`] for anything else.
pub fn evaluation_report(bytes: &[u8]) -> Result<Value, Unserved> {
    let _ = bytes;
    Err(Unserved::Unimplemented { story: "E17-8" })
}

/// Whether the control stream shows a paper workspace: it has a row, and every row's environment
/// is [`PAPER`] (DEC-1143 item 2).
pub fn paper(control: &[StoredEvent]) -> bool {
    let _ = control;
    false
}
