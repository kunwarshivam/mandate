//! What a request binds and shows (E8-1): the canonical content object, its hash, and the
//! confirmation code `approvals show` prints for it (DEC-155 item 3, EI-14).

use std::num::NonZeroU8;

use mandate_canon::{DecStr, Digest, Int, Key, Object, Value, to_canonical};
use mandate_num::{Price, Qty, Signed};
use mandate_time::UtcNanos;

use crate::{ApprovalError, RiskClock};

/// Why an order is asked. There is no exit variant: an exit is never asked (`AGENTS.md` rule 2,
/// mandate spec §6.2), so a request for one is unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AskablePurpose {
    Open,
    Increase,
}

/// The asset class the drift band is read for (DEC-156 item 3). `mandate-domain` has the same
/// two classes at layer 1, which this crate cannot depend on (DEC-165 item 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AssetClass {
    UsEquity,
    Crypto,
}

/// The last folded `MarkUpdated` at the request, and its account-stream `seq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceMark {
    pub price: Price,
    pub seq: u64,
}

/// The order a grant approves, exactly as the builder sized it (EI-4). The side is always `buy`,
/// because an ASK is only ever an `open` or an `increase`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundAction {
    pub instrument: String,
    pub asset_class: AssetClass,
    pub qty: Qty,
    pub limit: Price,
    pub purpose: AskablePurpose,
    pub mandate_version: String,
    /// `DecidedBy::label()` of the rule that asked: `rule:<id>`, `default`, or `admission_ceiling`.
    pub decided_by: String,
    /// Bound, because it is what the owner approved (brief, checks 10 and 11).
    pub combined_score: Signed,
    /// `None` when no mark was folded at the request, which re-validation skips as `drift`.
    pub reference_mark: Option<ReferenceMark>,
    pub approvers_required: NonZeroU8,
    pub independent_required: bool,
}

/// Who wrote a piece of evidence (mandate spec §6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceAuthor {
    /// "Output of software you selected".
    OwnerSelected,
    /// The research agent's thesis: platform-authored (DEC-97).
    Platform,
}

/// A reference to one model output or thesis: never its text, which stays an artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRef {
    pub event_id: String,
    pub artifact: Option<Digest>,
    pub author: EvidenceAuthor,
}

/// The §6.3 figures a request shows as its risk impact: facts about this order against the owner's
/// own limits, never an estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskField {
    OrderUsd,
    PositionUsdAfter,
    GrossUsdAfter,
    BoughtTodayUsd,
    Drawdown,
    DailyPnlFraction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskFigure {
    pub field: RiskField,
    pub value: DecStr,
    /// The mandate's own cap the figure is measured against, where it has one.
    pub cap: Option<DecStr>,
}

/// Everything `ApprovalRequested` carries for the owner to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContent {
    pub bound: BoundAction,
    pub evidence: Vec<EvidenceRef>,
    pub risk_impact: Vec<RiskFigure>,
    pub deadline: RiskClock,
}

/// SHA-256 of the canonical content object (journal spec §4). A response must repeat it (EI-14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContentHash(pub Digest);

/// The code `approvals show` prints for one content hash and `approve --code` must repeat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmationCode(pub String);

/// The canonical content object: exactly the keys DEC-165 item 3 lists, and nothing else.
///
/// # Errors
/// [`ApprovalError::Unrepresentable`] for a deadline outside `UtcNanos`'s range, an order value
/// that overflows, or a `seq` beyond the canonical integer bound.
pub fn content_object(content: &RequestContent) -> Result<Value, ApprovalError> {
    let bound = &content.bound;
    let order_usd = bound
        .qty
        .notional(bound.limit)
        .map_err(|_| unrepresentable("order value"))?;
    let action = object([
        ("instrument", text(&bound.instrument)),
        ("asset_class", text(asset_class_name(bound.asset_class))),
        ("side", text("buy")),
        ("qty", text(&bound.qty.to_string())),
        ("limit", text(&bound.limit.to_string())),
        ("order_usd", text(&order_usd.to_string())),
        ("purpose", text(purpose_name(bound.purpose))),
    ])?;
    let trigger = object([
        ("mandate_version", text(&bound.mandate_version)),
        ("decided_by", text(&bound.decided_by)),
    ])?;
    let outputs = content
        .evidence
        .iter()
        .map(|e| {
            object([
                ("event_id", text(&e.event_id)),
                (
                    "artifact",
                    e.artifact.map_or(Value::Null, |d| text(&d.to_string())),
                ),
                ("label", text(author_label(e.author))),
            ])
        })
        .collect::<Result<Vec<_>, _>>()?;
    let evidence = object([
        (
            "combined_score",
            object([
                ("label", text(SCORE_LABEL)),
                ("value", text(&bound.combined_score.to_string())),
            ])?,
        ),
        ("outputs", Value::Array(outputs)),
    ])?;
    let risk_impact = content
        .risk_impact
        .iter()
        .map(|f| {
            object([
                ("field", text(risk_field_name(f.field))),
                ("value", text(f.value.as_str())),
                (
                    "cap",
                    f.cap.as_ref().map_or(Value::Null, |c| text(c.as_str())),
                ),
            ])
        })
        .collect::<Result<Vec<_>, _>>()?;
    let reference_mark = match bound.reference_mark {
        None => Value::Null,
        Some(mark) => object([
            ("price", text(&mark.price.to_string())),
            ("seq", int(mark.seq)?),
        ])?,
    };
    let deadline =
        UtcNanos::from_parts(content.deadline.0, 0).map_err(|_| unrepresentable("deadline"))?;
    object([
        ("action", action),
        ("trigger", trigger),
        ("evidence", evidence),
        ("risk_impact", Value::Array(risk_impact)),
        ("reference_mark", reference_mark),
        ("deadline", text(&deadline.to_string())),
        ("default", text(DEFAULT_SENTENCE)),
        ("choices", Value::Array(vec![text("approve"), text("skip")])),
        (
            "approvers",
            object([
                ("required", int(u64::from(bound.approvers_required.get()))?),
                ("independent", Value::Bool(bound.independent_required)),
            ])?,
        ),
    ])
}

/// Mandate spec §6.4's label for the combined score.
const SCORE_LABEL: &str = "combined model score, not a probability of profit";
/// Mandate spec §6.4's statement of the default.
const DEFAULT_SENTENCE: &str = "If you do nothing, this action is skipped";

fn unrepresentable(what: &'static str) -> ApprovalError {
    ApprovalError::Unrepresentable { what }
}

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

fn int(n: u64) -> Result<Value, ApprovalError> {
    Int::new(n)
        .map(Value::Int)
        .ok_or_else(|| unrepresentable("integer"))
}

fn object<const N: usize>(members: [(&str, Value); N]) -> Result<Value, ApprovalError> {
    members
        .into_iter()
        .map(|(k, v)| Key::new(k).map(|k| (k, v)))
        .collect::<Result<Object, _>>()
        .map(Value::Object)
        .map_err(|_| unrepresentable("key"))
}

fn asset_class_name(class: AssetClass) -> &'static str {
    match class {
        AssetClass::UsEquity => "us_equity",
        AssetClass::Crypto => "crypto",
    }
}

fn purpose_name(purpose: AskablePurpose) -> &'static str {
    match purpose {
        AskablePurpose::Open => "open",
        AskablePurpose::Increase => "increase",
    }
}

fn author_label(author: EvidenceAuthor) -> &'static str {
    match author {
        EvidenceAuthor::OwnerSelected => "Output of software you selected",
        EvidenceAuthor::Platform => "platform-authored",
    }
}

fn risk_field_name(field: RiskField) -> &'static str {
    match field {
        RiskField::OrderUsd => "order_usd",
        RiskField::PositionUsdAfter => "position_usd_after",
        RiskField::GrossUsdAfter => "gross_usd_after",
        RiskField::BoughtTodayUsd => "bought_today_usd",
        RiskField::Drawdown => "drawdown",
        RiskField::DailyPnlFraction => "daily_pnl_fraction",
    }
}

/// # Errors
/// As [`content_object`].
pub fn content_hash(content: &RequestContent) -> Result<ContentHash, ApprovalError> {
    content_object(content).map(|object| ContentHash(Digest::of(&to_canonical(&object))))
}

/// # Errors
/// Never in practice: a SHA-256 hex digest always has eight leading digits.
pub fn confirmation_code(hash: &ContentHash) -> Result<ConfirmationCode, ApprovalError> {
    let hex = hash.0.to_string();
    hex.get(..CODE_LEN)
        .map(|code| ConfirmationCode(code.to_owned()))
        .ok_or_else(|| unrepresentable("confirmation code"))
}

/// How many leading hex digits of the content hash the owner re-types.
const CODE_LEN: usize = 8;
