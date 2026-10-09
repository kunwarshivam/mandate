//! The command responses of workspace API spec §5. Each repeats the envelope's members through
//! `response!`, since a type that flattens another cannot refuse an unknown member (DEC-682 item
//! 23). A non-empty `as_of` and a non-empty, unique `dropped` list of pointers are pending rules.

use serde::{Deserialize, Serialize};

use crate::envelope::{ApiVersion, StepUpStatus, Watermark};
use crate::wire::{EventId, Ref, Timestamp};

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
    /// kill switch's alone; `dropped` lists what an API-7 operation dropped (DEC-682 item 27).
    CommandAccepted {
        command_id: EventId,
        phase: Recorded,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step_up_status: Option<StepUpStatus>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dropped: Option<Vec<String>>,
    }
}
