//! The command responses of workspace API spec §5. Each repeats the envelope's members through
//! `response!`, since a type that flattens another cannot refuse an unknown member (DEC-682 item
//! 23). `as_of` names at least one watermark, and `dropped` is a non-empty list of unique pointers.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::envelope::{ApiVersion, StepUpStatus, Watermark};
use crate::requests::{Shape, max_orders};
use crate::wire::{
    Check, Decimal, EventId, Id, Ref, Rules, Timestamp, is_pointer, is_stream_id, is_word, present,
};

/// A closed response: the envelope's members, then its own. Its [`Rules`] check `as_of` with
/// [`as_of`] and then its own members.
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

        crate::wire::rules!(checked: $name);
    };
}

/// Common `AsOf` under `at` (§6.1, API-14): at least one watermark, each valid.
fn as_of(watermarks: &[Watermark], at: &str, check: &mut Check) {
    check.rule(!watermarks.is_empty(), &format!("{at}/as_of"), "items");
    for (index, watermark) in watermarks.iter().enumerate() {
        watermark.rules(&format!("{at}/as_of/{index}"), check);
    }
}

/// A `dropped` list under `at` (DEC-682 item 27): absent, or at least one pointer and each once,
/// the root `""` only where `root` allows it.
fn dropped(list: Option<&[String]>, root: bool, at: &str, check: &mut Check) {
    let Some(list) = list else {
        return;
    };
    let at = format!("{at}/dropped");
    let unique: BTreeSet<&String> = list.iter().collect();
    check.rule(!list.is_empty() && unique.len() == list.len(), &at, "items");
    for (index, pointer) in list.iter().enumerate() {
        let holds = is_pointer(pointer, root);
        check.rule(holds, &format!("{at}/{index}"), "pattern");
    }
}

/// The root is dropped only from a pause or hold body that is not JSON (DEC-682 item 27).
impl Rules for CommandAccepted {
    fn rules(&self, at: &str, check: &mut Check) {
        as_of(&self.as_of, at, check);
        dropped(self.dropped.as_deref(), true, at, check);
    }
}

/// A Skip body that is not JSON is refused, never read as `{}`, so the root is never dropped.
impl Rules for ApprovalResponseAccepted {
    fn rules(&self, at: &str, check: &mut Check) {
        as_of(&self.as_of, at, check);
        dropped(self.dropped.as_deref(), false, at, check);
    }
}

impl Rules for ConfirmAccepted {
    fn rules(&self, at: &str, check: &mut Check) {
        as_of(&self.as_of, at, check);
    }
}

impl Rules for EndDelegationAlreadyEnded {
    fn rules(&self, at: &str, check: &mut Check) {
        as_of(&self.as_of, at, check);
    }
}

impl Rules for DelegationPreview {
    fn rules(&self, at: &str, check: &mut Check) {
        as_of(&self.as_of, at, check);
        let delegation = format!("{at}/delegation");
        max_orders(self.delegation.max_orders, &delegation, check);
    }
}

/// `recorded` has no steps and every other phase at least one.
impl Rules for CommandStatus {
    fn rules(&self, at: &str, check: &mut Check) {
        as_of(&self.as_of, at, check);
        let recorded = self.phase == CommandPhase::Recorded;
        let steps = self.steps.is_empty() == recorded;
        check.rule(steps, &format!("{at}/steps"), "items");
        for (index, step) in self.steps.iter().enumerate() {
            step.rules(&format!("{at}/steps/{index}"), check);
        }
    }
}

/// `stream_id` an account, agent, control, or clock stream; `seq` from 1; `event_type`
/// `^[A-Z][A-Za-z]+$`; `reason` a lowercase code.
impl Rules for Step {
    fn rules(&self, at: &str, check: &mut Check) {
        let stream = is_stream_id(&self.stream_id, &["ctl", "clock"]);
        check.rule(stream, &format!("{at}/stream_id"), "pattern");
        check.rule(self.seq >= 1, &format!("{at}/seq"), "range");
        let named = is_event_type(&self.event_type);
        check.rule(named, &format!("{at}/event_type"), "pattern");
        let reason = self.reason.as_deref().is_none_or(is_word);
        check.rule(reason, &format!("{at}/reason"), "pattern");
    }
}

/// `^[A-Z][A-Za-z]+$`: an uppercase letter, then at least one letter.
fn is_event_type(text: &str) -> bool {
    let [first, second, rest @ ..] = text.as_bytes() else {
        return false;
    };
    let letters = rest.iter().chain([second]).all(u8::is_ascii_alphabetic);
    first.is_ascii_uppercase() && letters
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

/// A preview's classification: a delegation always increases risk (§5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncreasesRisk {
    RiskIncreasing,
}

response! {
    /// The `200` of a delegation preview (§5.3, DEC-682 item 29). The delegation's `max_orders` is
    /// 1 to 1,000.
    DelegationPreview {
        preview_id: Id,
        mandate_version: Ref,
        unasked_usd_after: Decimal,
        classification: IncreasesRisk,
        step_up_digest: Ref,
        delegation: PreviewedDelegation,
    }
}

/// The delegation a preview's version adds, as the owner entered it. Mandate spec §6.5's other
/// members are planned with E8-15 (DEC-681 item 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewedDelegation {
    #[serde(deserialize_with = "Option::deserialize")]
    pub expires_at: Option<Timestamp>,
    pub shape: Shape,
    pub max_orders: u32,
    pub max_order_usd: Decimal,
    pub max_total_usd: Decimal,
    pub source_approval_id: EventId,
}

/// Where a command stands (§5.5): never `unknown`, since an ambiguous append has no event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandPhase {
    Recorded,
    Taken,
    Applied,
    Refused,
    Ended,
}

response! {
    /// `GET /commands/{event_id}` (§5.5, DEC-682 item 15). `recorded` has no steps and every other
    /// phase at least one.
    CommandStatus {
        command_id: EventId,
        phase: CommandPhase,
        steps: Vec<Step>,
    }
}

/// One event a command produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    #[serde(deserialize_with = "Option::deserialize")]
    pub reason: Option<String>,
    pub stream_id: String,
    pub seq: u64,
    pub event_type: String,
    pub recorded_at: Timestamp,
}
