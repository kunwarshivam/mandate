//! The command responses of workspace API spec §5. Each repeats the envelope's members through
//! `response!`, since a type that flattens another cannot refuse an unknown member (DEC-682 item
//! 23). A non-empty `as_of` and a non-empty, unique `dropped` list of pointers are pending rules.

use serde::{Deserialize, Serialize};

use crate::envelope::{ApiVersion, StepUpStatus, Watermark};
use crate::wire::{EventId, Ref, Timestamp, present};

/// A closed response: the envelope's members, then its own, with its rules pending.
macro_rules! response {
    ($(#[$doc:meta])* $name:ident { $($(#[$attr:meta])* $member:ident: $ty:ty,)* }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub api_version: ApiVersion,
            pub build: Ref,
            pub served_at: Timestamp,
            pub as_of: Vec<Watermark>,
            $($(#[$attr])* pub $member: $ty,)*
        }

        crate::wire::rules!(pending: $name);
    };
}

/// Every `202`'s phase (DEC-682 item 14): recorded, never applied or approved (API-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recorded {
    Recorded,
}

response! {
    /// The `202` of an owner command, a kill switch, or a revoke (§5.4). `step_up_status` is the
    /// kill switch's alone; `dropped` lists what an API-7 operation dropped (DEC-682 item 27). Each
    /// is absent, never `null`, when it has nothing to say.
    CommandAccepted {
        command_id: EventId,
        phase: Recorded,
        #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
        step_up_status: Option<StepUpStatus>,
        #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
        dropped: Option<Vec<String>>,
    }
}

response! {
    /// The `202` of an approval response (§5.2): recorded, never approved (API-12). `dropped` is
    /// never the root `""`, and is absent, never `null`, when nothing was dropped.
    ApprovalResponseAccepted {
        response_id: EventId,
        phase: Recorded,
        #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
        dropped: Option<Vec<String>>,
    }
}

/// The server's own classification of a confirmed change (§5.1, mandate spec §9.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    RiskIncreasing,
    RiskReducing,
    Neutral,
}

/// When a confirmed version takes effect: `now` unless it increases risk (§5.1, mandate spec
/// §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applies {
    Now,
    NextSafePoint,
}

response! {
    /// The `202` of a confirm (§5.1).
    ConfirmAccepted {
        command_id: EventId,
        phase: Recorded,
        classification: Classification,
        applies: Applies,
    }
}

/// Ending a delegation already gone (§5.3, DEC-682 item 29): nothing is committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlreadyEnded {
    AlreadyEnded,
}

response! {
    /// The `200` of ending a delegation that is already gone (§5.3).
    EndDelegationAlreadyEnded {
        outcome: AlreadyEnded,
    }
}
