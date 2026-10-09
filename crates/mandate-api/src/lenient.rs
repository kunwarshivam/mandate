//! The API-7 operations' lenient decoder (workspace API spec §5, DEC-682 item 27, DEC-689 item 2,
//! DEC-886): only an operation's hard members are strict, and every other member that is unknown or
//! fails to parse is dropped, never applied, and listed by its JSON pointer for the `202`'s
//! `dropped`.

use serde::Serialize;

use crate::envelope::{Record, StepUpEvidence};
use crate::requests::{
    ApprovalResponseRequest, EndDelegationRequest, HoldRequest, KillSwitchRequest,
    OwnerExitRequest, PauseRequest,
};
use crate::wire::{Ref, Refused};

/// What [`decode_lenient`] kept of a body, and the pointer of each member it dropped: once each, in
/// the order the members appear in the body (DEC-886 item 11). A member left out is never listed
/// (item 12), and a body read as `{}` lists `""`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept<T> {
    pub value: T,
    pub dropped: Vec<String>,
}

/// An API-7 operation's body as the server keeps it. The hard members are strict: a body whose hard
/// member is missing, unparsable, or named twice is refused (DEC-886 item 6).
pub trait Api7: Serialize + sealed::Sealed {
    /// The body's hard members by name: the kill switch's `scope`, an owner exit's `instrument`,
    /// and a Skip's `verdict` and `content_hash` (DEC-682 item 27). The path ids are not in a body.
    const HARD: &'static [&'static str];
    /// Whether a body that is not a JSON object is read as `{}` with `dropped: [""]`: pause and
    /// hold only (DEC-682 item 27, DEC-886 item 5).
    const READS_NON_OBJECT: bool;
}

mod sealed {
    pub trait Sealed {}
}

/// An approval response as the server keeps it: an `approved` judged strictly, or a Skip judged
/// leniently (§5.2, DEC-682 item 27).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum ApprovalAnswer {
    Approved(ApprovalResponseRequest),
    Skipped(SkipResponse),
}

/// The members a Skip keeps. It never names a delegation: a non-null one is dropped (DEC-886
/// item 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkipResponse {
    pub verdict: SkipVerdict,
    pub content_hash: Ref,
    pub record: Option<Record>,
    pub step_up: Option<StepUpEvidence>,
}

/// A Skip's verdict, `skipped`, the one this shape can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipVerdict {
    Skipped,
}

/// `$shape`'s [`Api7`] answers.
macro_rules! api7 {
    ($($shape:ty: [$($hard:literal),*], $reads:literal;)+) => {
        $(impl sealed::Sealed for $shape {}

        impl Api7 for $shape {
            const HARD: &'static [&'static str] = &[$($hard),*];
            const READS_NON_OBJECT: bool = $reads;
        })+
    };
}

api7! {
    PauseRequest: [], true;
    HoldRequest: [], true;
    EndDelegationRequest: [], false;
    KillSwitchRequest: ["scope"], false;
    OwnerExitRequest: ["instrument"], false;
    ApprovalAnswer: ["verdict", "content_hash"], false;
}

/// The body of an API-7 operation as `T` keeps it, with what it dropped (DEC-682 item 27,
/// DEC-886).
///
/// # Errors
/// [`Refused::Invalid`] for a missing, unparsable, or duplicated hard member, for a body that is
/// not a JSON object on an operation other than pause and hold, and for an `approved` that fails
/// [`crate::wire::decode`].
pub fn decode_lenient<T: Api7>(body: &[u8]) -> Result<Kept<T>, Refused> {
    let _ = body;
    Err(Refused::Unimplemented)
}
