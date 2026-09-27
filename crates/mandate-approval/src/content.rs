//! What a request binds and shows (E8-1): the canonical content object, its hash, and the
//! confirmation code `approvals show` prints for it (DEC-155 item 3, EI-14).

use std::num::NonZeroU8;

use mandate_canon::{DecStr, Digest, Value};
use mandate_num::{Price, Qty, Signed};

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
/// [`ApprovalError::Unimplemented`] until E8-1.
pub fn content_object(content: &RequestContent) -> Result<Value, ApprovalError> {
    let _ = content;
    Err(ApprovalError::Unimplemented { story: "E8-1" })
}

/// # Errors
/// [`ApprovalError::Unimplemented`] until E8-1.
pub fn content_hash(content: &RequestContent) -> Result<ContentHash, ApprovalError> {
    let _ = content;
    Err(ApprovalError::Unimplemented { story: "E8-1" })
}

/// # Errors
/// [`ApprovalError::Unimplemented`] until E8-1.
pub fn confirmation_code(hash: &ContentHash) -> Result<ConfirmationCode, ApprovalError> {
    let _ = hash;
    Err(ApprovalError::Unimplemented { story: "E8-1" })
}
