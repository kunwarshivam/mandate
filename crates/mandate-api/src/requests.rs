//! The request bodies of workspace API spec §4.2, §4.5, §4.6, and §5.1 to §5.4: closed, so a
//! member the schema does not name, such as `requested_by`, is refused (DEC-682 item 2, API-6). A
//! member the schema requires but lets be `null` decodes through `Option::deserialize`, so leaving
//! it out is refused while `null` is not (DEC-682 item 9). The API-7 shapes are strict here; A2's
//! lenient decoder drops what DEC-682 item 27 lets them drop (DEC-689 item 2).

use serde::{Deserialize, Serialize};

use crate::envelope::{Record, StepUpAction, StepUpAssertion, StepUpEvidence, StepUpMethod};
use crate::responses::Classification;
use crate::wire::{Asset, Check, Decimal, EventId, Id, Ref, Refused, Rules, Timestamp, is_pointer};

/// `POST /agents/{agent_id}/pause` (§5.4): `{}` is valid (DEC-682 item 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct PauseRequest {
    #[serde(default)]
    pub record: Option<Record>,
}

/// `POST /agents/{agent_id}/hold` (§4.2, DEC-191): `{}` is valid (DEC-682 item 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct HoldRequest {
    #[serde(default)]
    pub record: Option<Record>,
}

/// `POST /agents/{agent_id}/delegations/{delegation_id}/end` (§5.3): `{}` is valid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct EndDelegationRequest {
    #[serde(default)]
    pub record: Option<Record>,
}

/// Lifting a hold a client set (§4.2, DEC-191). A `null` step-up is forwarded, and the runtime
/// refuses it (DEC-682 item 9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct LiftHoldRequest {
    #[serde(deserialize_with = "Option::deserialize")]
    pub record: Option<Record>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_up: Option<StepUpEvidence>,
}

/// `POST /agents/{agent_id}/resume` (§5.4). A `null` step-up is forwarded, and the runtime refuses
/// it (`OwnerCommandRefused`). `step_up_assertion` is optional, an absent one read as `null`, and
/// is carried only beside `passkey` evidence ([DEC-2128] item 1, item 6's own slice).
///
/// [DEC-2128]: ../../../docs/project/decisions/DEC-2128.md
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct ResumeRequest {
    #[serde(deserialize_with = "Option::deserialize")]
    pub record: Option<Record>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_up: Option<StepUpEvidence>,
    #[serde(default)]
    pub step_up_assertion: Option<StepUpAssertion>,
}

/// `POST /step-up/challenges` (§3.6, DEC-2103 item 1): the one action the challenge binds. The
/// workspace and principal are the session's, so the body names neither.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct StepUpChallengeRequest {
    pub action: StepUpAction,
}

/// `POST /agents/{agent_id}/acknowledgments` (§3.11 item 12, DEC-2144): the hold acknowledged and
/// its subject, declared after the nullable members so serde names a missing one of them first.
/// `request` and `step_up_assertion` are optional, an absent one read as `null`; an assertion is
/// carried only beside `passkey` evidence (DEC-2128 item 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct AcknowledgeRequest {
    #[serde(deserialize_with = "Option::deserialize")]
    pub record: Option<Record>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_up: Option<StepUpEvidence>,
    #[serde(default)]
    pub request: Option<EventId>,
    #[serde(default)]
    pub step_up_assertion: Option<StepUpAssertion>,
    pub subject: AcknowledgeSubject,
    pub acknowledged: EventId,
}

/// `POST /agents/{agent_id}/acknowledgment-requests` (§3.11 item 12, DEC-2144): an acknowledgment's
/// members but `request`, for a workspace where another user must acknowledge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct AcknowledgmentRequestRequest {
    #[serde(deserialize_with = "Option::deserialize")]
    pub record: Option<Record>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_up: Option<StepUpEvidence>,
    #[serde(default)]
    pub step_up_assertion: Option<StepUpAssertion>,
    pub subject: AcknowledgeSubject,
    pub acknowledged: EventId,
}

/// What an acknowledgment answers (journal spec §9.28 version 2): a latched limit, an account
/// restriction, or a reconciliation hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcknowledgeSubject {
    Limit,
    Restriction,
    Reconciliation,
}

/// `POST /agents/{agent_id}/requests` (§4.6, DEC-682 item 16). Who asked comes from the channel,
/// never the body (API-6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
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
crate::wire::rules!(none: OwnerRequest, StepUpChallengeRequest);

/// `POST /mandate-versions/{mandate_version}/confirm` (§5.1): each confirmed path is a pointer.
/// `step_up_assertion` is optional, an absent one read as `null`, and is carried only beside
/// `passkey` evidence ([DEC-2128] item 1).
///
/// [DEC-2128]: ../../../docs/project/decisions/DEC-2128.md
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct ConfirmRequest {
    #[serde(deserialize_with = "Option::deserialize")]
    pub agent_id: Option<Id>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub base_version: Option<Ref>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_up: Option<StepUpEvidence>,
    #[serde(default)]
    pub step_up_assertion: Option<StepUpAssertion>,
    pub confirmed_paths: Vec<String>,
    pub warnings_acknowledged: Vec<Id>,
    pub classification_shown: Classification,
    pub screen_digest: Ref,
    pub record: Record,
}

/// `POST /approvals/{approval_id}/responses` (§5.2). A `skipped` names no delegation (DEC-682
/// item 11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct ApprovalResponseRequest {
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_up: Option<StepUpEvidence>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub delegation: Option<DelegationChosen>,
    pub verdict: Verdict,
    pub content_hash: Ref,
    pub record: Record,
}

/// The owner's answer to an approval (§5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approved,
    Skipped,
}

/// The previewed delegation an `approved` adds (§5.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct DelegationChosen {
    pub preview_id: Id,
    pub mandate_version: Ref,
}

/// `POST /approvals/{approval_id}/delegation-previews` (§5.3, DEC-682 item 13). `until_close`
/// takes the three nullable members `null` and a timed shape takes all three; `max_orders` is 1 to
/// 1,000.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct DelegationPreviewRequest {
    #[serde(deserialize_with = "Option::deserialize")]
    pub max_order_usd: Option<Decimal>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub max_total_usd: Option<Decimal>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub expires_at: Option<Timestamp>,
    pub shape: Shape,
    pub max_orders: u32,
}

/// A delegation's shape (§5.3, mandate spec §6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    UntilClose,
    InstrumentForTime,
    KindForTime,
}

/// `POST /agents/{agent_id}/stop` (§4.2, DEC-682 item 10): a warning exactly with `release`, and
/// a `step_up_assertion` only beside `passkey` evidence, as on a resume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct StopRequest {
    #[serde(deserialize_with = "Option::deserialize")]
    pub warning_shown: Option<Ref>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub record: Option<Record>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_up: Option<StepUpEvidence>,
    #[serde(default)]
    pub step_up_assertion: Option<StepUpAssertion>,
    pub release: bool,
}

/// `POST /kill-switch` (§5.4, DEC-682 item 7): only `scope` is required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct KillSwitchRequest {
    pub scope: Scope,
    #[serde(default)]
    pub environment_shown: Option<Environment>,
    #[serde(default)]
    pub owner_exit: Option<Vec<BidConfirmation>>,
    #[serde(default)]
    pub record: Option<Record>,
    #[serde(default)]
    pub step_up: Option<StepUpEvidence>,
}

/// What a kill switch stops, tagged on `kind`: a workspace scope's `id` is `null` and no other's
/// is (DEC-682 item 7), so serde holds the schema's `if`/`then`. No org scope (DEC-436 item 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    remote = "Self",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Scope {
    Agent {
        id: Id,
    },
    Connection {
        id: Id,
    },
    Workspace {
        #[serde(deserialize_with = "crate::wire::null")]
        id: (),
    },
}

/// The environment the owner saw when they pressed the switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    Paper,
    Live,
}

/// The displayed bid an owner confirmed to sell an equity outside the regular session (§5.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct BidConfirmation {
    pub asset_id: Asset,
    pub bid: Decimal,
    pub bid_size: Decimal,
    pub quoted_at: Timestamp,
    pub floor: Decimal,
}

/// `POST /agents/{agent_id}/exits` (§5.4, DEC-682 item 12): only `instrument` is required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct OwnerExitRequest {
    pub instrument: Asset,
    #[serde(default)]
    pub bid: Option<Decimal>,
    #[serde(default)]
    pub bid_size: Option<Decimal>,
    #[serde(default)]
    pub quoted_at: Option<Timestamp>,
    #[serde(default)]
    pub floor: Option<Decimal>,
    #[serde(default)]
    pub record: Option<Record>,
    #[serde(default)]
    pub step_up: Option<StepUpEvidence>,
}

/// `POST /connections/{connection_id}/revoke` (§4.5, §5.6, DEC-682 item 18): `compromised` absent
/// is `false`, the ordinary revoke (DEC-681 item 8), and `null` is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct RevokeRequest {
    #[serde(default)]
    pub compromised: bool,
    #[serde(default)]
    pub record: Option<Record>,
    #[serde(default)]
    pub step_up: Option<StepUpEvidence>,
}

/// The broker an OAuth connect names: Alpaca alone (§4.5, DEC-2623 item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OauthBroker {
    Alpaca,
}

/// The token-breadth disclosure an owner confirmed, by document and version, never by its text
/// (DEC-2621 items 4 and 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct Disclosure {
    pub document: Id,
    pub version: String,
}

/// `POST /connections/oauth/start` (§4.5, DEC-2623 item 1). A `null` step-up is decoded, and the
/// route refuses it with nothing appended; `environment` `live` is decoded, and the route refuses
/// it as `live_unavailable` (DEC-883). `step_up_assertion` is optional, an absent one read as
/// `null`, and is carried only beside `passkey` evidence ([DEC-2126] item 1).
///
/// [DEC-2126]: ../../../docs/project/decisions/DEC-2126.md
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct OauthStartRequest {
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_up: Option<StepUpEvidence>,
    #[serde(default)]
    pub step_up_assertion: Option<StepUpAssertion>,
    pub broker: OauthBroker,
    pub environment: Environment,
    pub disclosure: Disclosure,
}

impl Rules for ConfirmRequest {
    fn rules(&self, at: &str, check: &mut Check) {
        for (index, path) in self.confirmed_paths.iter().enumerate() {
            let at = format!("{at}/confirmed_paths/{index}");
            check.rule(is_pointer(path, false), &at, "pattern");
        }
        assertion_rules(
            self.step_up.as_ref(),
            self.step_up_assertion.as_ref(),
            at,
            check,
        );
    }
}

/// [`assertion_rules`] for a body the server decodes by hand (the deploy body, until its schema and
/// its type land together, DEC-2101 item 6), so the deploy refuses an assertion exactly as a
/// confirmation does (DEC-2128 item 1).
///
/// # Errors
/// [`Refused::Invalid`] with the violations found: one at `/step_up_assertion` for an assertion
/// beside evidence that is not `passkey`, and any of the assertion's own members.
pub fn assertion_checked(
    evidence: Option<&StepUpEvidence>,
    assertion: Option<&StepUpAssertion>,
) -> Result<(), Refused> {
    let mut check = Check::default();
    assertion_rules(evidence, assertion, "", &mut check);
    check.done()
}

/// DEC-2126 item 1, as DEC-2128 item 1 applies it: an assertion is carried only beside `passkey`
/// evidence, else one violation at `/step_up_assertion`, and its own members are checked.
fn assertion_rules(
    evidence: Option<&StepUpEvidence>,
    assertion: Option<&StepUpAssertion>,
    at: &str,
    check: &mut Check,
) {
    if let Some(assertion) = assertion {
        let rests_on_a_passkey =
            evidence.is_some_and(|evidence| evidence.method == StepUpMethod::Passkey);
        let at = format!("{at}/step_up_assertion");
        check.rule(rests_on_a_passkey, &at, "condition");
        assertion.rules(&at, check);
    }
}

impl Rules for ApprovalResponseRequest {
    fn rules(&self, at: &str, check: &mut Check) {
        let skipped = self.verdict == Verdict::Skipped;
        let holds = !(skipped && self.delegation.is_some());
        check.rule(holds, &format!("{at}/delegation"), "condition");
    }
}

impl Rules for DelegationPreviewRequest {
    fn rules(&self, at: &str, check: &mut Check) {
        let timed = self.shape != Shape::UntilClose;
        let members = [
            ("max_order_usd", self.max_order_usd.is_some()),
            ("max_total_usd", self.max_total_usd.is_some()),
            ("expires_at", self.expires_at.is_some()),
        ];
        for (name, present) in members {
            check.rule(present == timed, &format!("{at}/{name}"), "condition");
        }
        max_orders(self.max_orders, at, check);
    }
}

/// A delegation's `max_orders` under `at`, 1 to 1,000 (§5.3, DEC-682 item 13).
pub(crate) fn max_orders(count: u32, at: &str, check: &mut Check) {
    let holds = (1..=1000).contains(&count);
    check.rule(holds, &format!("{at}/max_orders"), "range");
}

impl Rules for StopRequest {
    fn rules(&self, at: &str, check: &mut Check) {
        let holds = self.warning_shown.is_some() == self.release;
        check.rule(holds, &format!("{at}/warning_shown"), "condition");
        assertion_rules(
            self.step_up.as_ref(),
            self.step_up_assertion.as_ref(),
            at,
            check,
        );
    }
}

impl Rules for ResumeRequest {
    fn rules(&self, at: &str, check: &mut Check) {
        assertion_rules(
            self.step_up.as_ref(),
            self.step_up_assertion.as_ref(),
            at,
            check,
        );
    }
}

crate::wire::rules!(checked: ConfirmRequest, ApprovalResponseRequest, DelegationPreviewRequest);
impl Rules for OauthStartRequest {
    fn rules(&self, at: &str, check: &mut Check) {
        let version = self.disclosure.version.as_str();
        let holds = (1..=9).contains(&version.len())
            && version.bytes().all(|b| b.is_ascii_digit())
            && !version.starts_with('0');
        check.rule(holds, &format!("{at}/disclosure/version"), "pattern");
        assertion_rules(
            self.step_up.as_ref(),
            self.step_up_assertion.as_ref(),
            at,
            check,
        );
    }
}

impl Rules for AcknowledgeRequest {
    fn rules(&self, at: &str, check: &mut Check) {
        assertion_rules(
            self.step_up.as_ref(),
            self.step_up_assertion.as_ref(),
            at,
            check,
        );
    }
}

impl Rules for AcknowledgmentRequestRequest {
    fn rules(&self, at: &str, check: &mut Check) {
        assertion_rules(
            self.step_up.as_ref(),
            self.step_up_assertion.as_ref(),
            at,
            check,
        );
    }
}

crate::wire::rules!(checked: StopRequest, ResumeRequest, OauthStartRequest);
crate::wire::rules!(checked: AcknowledgeRequest, AcknowledgmentRequestRequest);
crate::wire::rules!(none: KillSwitchRequest, OwnerExitRequest, RevokeRequest);

crate::wire::object_only!(
    PauseRequest,
    HoldRequest,
    EndDelegationRequest,
    LiftHoldRequest,
    ResumeRequest,
    AcknowledgeRequest,
    AcknowledgmentRequestRequest,
    OwnerRequest,
    ConfirmRequest,
    ApprovalResponseRequest,
    DelegationChosen,
    DelegationPreviewRequest,
    StopRequest,
    KillSwitchRequest,
    Scope,
    BidConfirmation,
    OwnerExitRequest,
    RevokeRequest,
    StepUpChallengeRequest,
    Disclosure,
    OauthStartRequest
);
