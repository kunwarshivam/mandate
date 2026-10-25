//! The envelope's shapes (workspace API spec §3.2, §3.3, §6.1; DEC-682): wire types apart from
//! `mandate-identity`'s and the journal's, which the server converts between (DEC-689).

use serde::{Deserialize, Serialize};

use crate::wire::{Check, EventId, Id, Ref, Rules, Timestamp, is_stream_id};

/// The API's version, `v1` (§3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiVersion {
    #[serde(rename = "v1")]
    V1,
}

/// The last event of one stream a response reflects (§6.1, `common.schema.json#/$defs/Watermark`).
/// `stream_id` follows journal spec §2, and `seq` is 1 to 2^53 - 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct Watermark {
    pub stream_id: String,
    pub seq: u64,
    pub hash: Ref,
    pub recorded_at: Timestamp,
}

/// Who an event names (§3.3 item 4): a client is never a user, and names the user it acts for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    remote = "Self",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Actor {
    User { id: Id },
    Client { id: Id, on_behalf_of: Id },
}

/// The rendered record screen and the build that rendered it (§5, mandate spec §10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct Record {
    pub artifact: Ref,
    pub ui_build: Ref,
}

/// Journal spec §9.2's step-up evidence: the challenge's ULID, when the user verified, and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct StepUpEvidence {
    pub assertion_id: EventId,
    pub authenticated_at: Timestamp,
    pub method: StepUpMethod,
}

/// What a passkey's authenticator returned for the challenge a request's `step_up` names, sent
/// beside it ([DEC-2108] item 7, `envelope.schema.json#/$defs/StepUpAssertion`): the passkey's id
/// and the authenticator's three byte strings, each unpadded base64url. The server judges it and
/// stores its canonical bytes as the raw-assertion artifact (journal spec §9.32); it is never part
/// of the journaled evidence.
///
/// [DEC-2108]: ../../../docs/project/decisions/DEC-2108.md
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct StepUpAssertion {
    pub passkey_id: String,
    pub authenticator_data: String,
    pub client_data_json: String,
    pub signature: String,
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

/// The one action a step-up challenge binds (§3.6, API-17): its kind and the SHA-256 of the
/// action's canonical object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct StepUpAction {
    pub kind: StepUpActionKind,
    pub digest: Ref,
}

/// §3.6's step-up action kinds, one per action identity spec ID-4 lists that this API serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepUpActionKind {
    ConfirmVersion,
    Deploy,
    Approve,
    Connection,
    Resume,
    Stop,
    Acknowledge,
    OwnerExit,
    KillSwitchPrivilege,
    Disclosure,
    PolicyLoosen,
    MemberInvite,
    RoleGrant,
    ClientConnect,
    CredentialEnrol,
    NotificationAddress,
    BreakGlassApprove,
    LiftHold,
}

impl Rules for Watermark {
    fn rules(&self, at: &str, check: &mut Check) {
        let stream = is_stream_id(&self.stream_id, &["ctl", "clock", "ntf"]);
        check.rule(stream, &format!("{at}/stream_id"), "pattern");
        let seq = (1..=mandate_canon::MAX_INT).contains(&self.seq);
        check.rule(seq, &format!("{at}/seq"), "range");
    }
}

impl Rules for StepUpAssertion {
    fn rules(&self, at: &str, check: &mut Check) {
        check.rule(
            is_passkey_id(&self.passkey_id),
            &format!("{at}/passkey_id"),
            "pattern",
        );
        for (name, text) in [
            ("authenticator_data", &self.authenticator_data),
            ("client_data_json", &self.client_data_json),
            ("signature", &self.signature),
        ] {
            check.rule(is_base64url(text), &format!("{at}/{name}"), "pattern");
        }
    }
}

/// `common.schema.json#/$defs/PasskeyId`: unpadded base64url of 16 to 1023 bytes, so 22 to 1364
/// characters and never a length whose remainder by 4 is 1.
fn is_passkey_id(text: &str) -> bool {
    (22..=1364).contains(&text.len()) && text.len() % 4 != 1 && is_base64url_alphabet(text)
}

/// `common.schema.json#/$defs/Base64url`: 1 to 8192 characters of the base64url alphabet.
fn is_base64url(text: &str) -> bool {
    (1..=8192).contains(&text.len()) && is_base64url_alphabet(text)
}

fn is_base64url_alphabet(text: &str) -> bool {
    text.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

crate::wire::rules!(checked: Watermark, StepUpAssertion);
crate::wire::rules!(none: ApiVersion, Actor, Record, StepUpEvidence, StepUpStatus, StepUpAction);

crate::wire::object_only!(
    Watermark,
    Actor,
    Record,
    StepUpEvidence,
    StepUpAssertion,
    StepUpAction
);
