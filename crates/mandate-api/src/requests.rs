//! The plain request bodies of workspace API spec §4.2, §4.6, §5.3, and §5.4: closed, so a member
//! the schema does not name, such as `requested_by`, is refused (DEC-682 item 2, API-6). A member
//! the schema requires but lets be `null` decodes through `Option::deserialize`, so leaving it out
//! is refused while `null` is not (DEC-682 item 9).

use serde::{Deserialize, Serialize};

use crate::envelope::{Record, StepUpEvidence};
use crate::wire::{Asset, Decimal, EventId};

/// `POST /agents/{agent_id}/pause` (§5.4): `{}` is valid (DEC-682 item 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PauseRequest {
    #[serde(default)]
    pub record: Option<Record>,
}

/// `POST /agents/{agent_id}/hold` (§4.2, DEC-191): `{}` is valid (DEC-682 item 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldRequest {
    #[serde(default)]
    pub record: Option<Record>,
}

/// `POST /agents/{agent_id}/delegations/{delegation_id}/end` (§5.3): `{}` is valid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndDelegationRequest {
    #[serde(default)]
    pub record: Option<Record>,
}

/// Lifting a hold a client set (§4.2, DEC-191). A `null` step-up is forwarded, and the runtime
/// refuses it (DEC-682 item 9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiftHoldRequest {
    #[serde(deserialize_with = "Option::deserialize")]
    pub record: Option<Record>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_up: Option<StepUpEvidence>,
}

/// `POST /agents/{agent_id}/resume` (§5.4). A `null` step-up is forwarded, and the runtime refuses
/// it (`OwnerCommandRefused`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeRequest {
    #[serde(deserialize_with = "Option::deserialize")]
    pub record: Option<Record>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_up: Option<StepUpEvidence>,
}

/// `POST /agents/{agent_id}/acknowledgments` (§4.2): the event acknowledged, declared after the
/// nullable members so serde names a missing one of them first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcknowledgeRequest {
    #[serde(deserialize_with = "Option::deserialize")]
    pub record: Option<Record>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_up: Option<StepUpEvidence>,
    pub acknowledged: EventId,
}

/// `POST /agents/{agent_id}/requests` (§4.6, DEC-682 item 16). Who asked comes from the channel,
/// never the body (API-6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerRequest {
    pub instrument: Asset,
    pub side: Side,
    #[serde(default)]
    pub size: Option<Decimal>,
}

/// Which way an owner request trades.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

crate::wire::rules!(none: PauseRequest, HoldRequest, EndDelegationRequest, LiftHoldRequest);
crate::wire::rules!(none: ResumeRequest, AcknowledgeRequest, OwnerRequest);
