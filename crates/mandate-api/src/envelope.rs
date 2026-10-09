//! The envelope's shapes (workspace API spec §3.2, §3.3, §6.1; DEC-682): wire types apart from
//! `mandate-identity`'s and the journal's, which the server converts between (DEC-689).

use serde::{Deserialize, Serialize};

use crate::wire::{EventId, Id, Ref, Timestamp};

/// The API's version, `v1` (§3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiVersion {
    #[serde(rename = "v1")]
    V1,
}

/// The last event of one stream a response reflects (§6.1, `common.schema.json#/$defs/Watermark`).
/// `stream_id`'s grammar and `seq`'s bounds, 1 to 2^53 - 1, are pending rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Watermark {
    pub stream_id: String,
    pub seq: u64,
    pub hash: Ref,
    pub recorded_at: Timestamp,
}

/// Who an event names (§3.3 item 4): a client is never a user, and names the user it acts for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Actor {
    User { id: Id },
    Client { id: Id, on_behalf_of: Id },
}

/// The rendered record screen and the build that rendered it (§5, mandate spec §10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub artifact: Ref,
    pub ui_build: Ref,
}

/// Journal spec §9.2's step-up evidence: the challenge's ULID, when the user verified, and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepUpEvidence {
    pub assertion_id: EventId,
    pub authenticated_at: Timestamp,
    pub method: StepUpMethod,
}

/// How the user verified (identity spec §7.3): `live` takes `passkey` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepUpMethod {
    Passkey,
    CliConfirm,
}

/// The kill switch's advisory report of the step-up it carried (DEC-682 item 14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepUpStatus {
    Bound,
    Missing,
    Unbound,
}

crate::wire::rules!(pending: Watermark);
crate::wire::rules!(none: ApiVersion, Actor, Record, StepUpEvidence, StepUpStatus);
