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

use mandate_api::wire::Id;
use mandate_canon::Value;
use mandate_journal::{ArtifactRef, StoredEvent};

use crate::reads::envelope;

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
}

/// One recorded evaluation, before its report is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationView {
    pub evaluation_id: String,
    pub report_ref: ArtifactRef,
}

/// The backtest the control stream records under `backtest_id`.
pub fn backtest(control: &[StoredEvent], backtest_id: &str) -> Found<BacktestView> {
    the_one(
        control,
        BACKTEST_RECORDED,
        "backtest_id",
        backtest_id,
        |payload| {
            let mandate_version = text(payload, "mandate_version")
                .filter(|version| ArtifactRef::parse(version).is_some())?;
            if text(payload, "marker") != Some("backtest")
                || payload.get("report_version").and_then(Value::as_int) != Some(2)
            {
                return None;
            }
            let outcome = match (
                text(payload, "result"),
                payload.get("report"),
                payload.get("refusal"),
            ) {
                (Some("completed"), Some(Value::Str(report)), Some(Value::Null)) => {
                    Outcome::Completed {
                        report_ref: ArtifactRef::parse(report)?,
                    }
                }
                (Some("refused"), Some(Value::Null), Some(Value::Str(code))) if is_code(code) => {
                    Outcome::Refused {
                        refusal: code.clone(),
                    }
                }
                _ => return None,
            };
            Some(BacktestView {
                backtest_id: backtest_id.to_owned(),
                mandate_version: mandate_version.to_owned(),
                outcome,
            })
        },
    )
}

/// The evaluation the control stream records under `evaluation_id`.
pub fn evaluation(control: &[StoredEvent], evaluation_id: &str) -> Found<EvaluationView> {
    the_one(
        control,
        EVALUATION_RECORDED,
        "evaluation_id",
        evaluation_id,
        |payload| {
            if payload.get("report_version").and_then(Value::as_int) != Some(1) {
                return None;
            }
            Some(EvaluationView {
                evaluation_id: evaluation_id.to_owned(),
                report_ref: text(payload, "report").and_then(ArtifactRef::parse)?,
            })
        },
    )
}

/// The backtest report `bytes` hold, when it is a JSON object of `report_version` 2 for the version
/// `view` backtested.
///
/// # Errors
/// [`Unserved`]: anything else is never served.
pub fn backtest_report(bytes: &[u8], view: &BacktestView) -> Result<Value, Unserved> {
    let report = report(bytes, "backtest_report", 2)?;
    if text(&report, "mandate_version") == Some(view.mandate_version.as_str()) {
        Ok(report)
    } else {
        Err(Unserved::OtherVersion)
    }
}

/// The evaluation report `bytes` hold, when it is a JSON object of `report_version` 1.
///
/// # Errors
/// [`Unserved::NotAReport`] for anything else.
pub fn evaluation_report(bytes: &[u8]) -> Result<Value, Unserved> {
    report(bytes, "forward_paper_evaluation", 1)
}

/// Whether the control stream shows a paper workspace: it has a row, and every row's environment
/// is [`PAPER`] (DEC-1143 item 2).
pub fn paper(control: &[StoredEvent]) -> bool {
    !control.is_empty() && control.iter().all(|row| row.environment == PAPER)
}

/// The one schema-version-1 `event_type` row whose payload's `member` equals `id`, read by `read`.
/// An id outside the id grammar names nothing. A row whose body does not parse is skipped, as the
/// other reads skip it.
fn the_one<T>(
    control: &[StoredEvent],
    event_type: &str,
    member: &str,
    id: &str,
    read: impl Fn(&Value) -> Option<T>,
) -> Found<T> {
    if Id::try_from(id.to_owned()).is_err() {
        return Found::Absent;
    }
    let mut found = Found::Absent;
    for row in control.iter().filter(|row| row.event_type == event_type) {
        let Some(payload) = envelope(row).and_then(|event| event.get("payload").cloned()) else {
            continue;
        };
        if text(&payload, member) != Some(id) {
            continue;
        }
        let view = (row.schema_version == 1).then(|| read(&payload)).flatten();
        found = match (found, view) {
            (Found::Absent, Some(view)) => Found::One(view),
            _ => return Found::Unreadable,
        };
    }
    found
}

/// The JSON object `bytes` hold when its `kind` and `report_version` are these.
fn report(bytes: &[u8], kind: &str, version: u64) -> Result<Value, Unserved> {
    let report = mandate_canon::parse(bytes).map_err(|_| Unserved::NotAReport)?;
    let shaped = report.as_object().is_some()
        && text(&report, "kind") == Some(kind)
        && report.get("report_version").and_then(Value::as_int) == Some(version);
    shaped.then_some(report).ok_or(Unserved::NotAReport)
}

/// The text member `name` of `value`.
fn text<'v>(value: &'v Value, name: &str) -> Option<&'v str> {
    value.get(name).and_then(Value::as_str)
}

/// ADR-0001 ES-09's stable code: `[a-z][a-z0-9_]{0,63}`.
fn is_code(code: &str) -> bool {
    let mut bytes = code.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && code.len() <= 64
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
