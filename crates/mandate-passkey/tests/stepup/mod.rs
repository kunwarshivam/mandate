//! A step-up case every check passes, for the step-up tests: one principal in one workspace, a
//! challenge for one approval issued at [`ISSUED`], a passkey past its cool-off, and an assertion
//! the software authenticator signed over SHA-256 of the record's canonical form, computed here
//! from the expected JSON text rather than by the crate. Each test changes the fields its title
//! names.

#![allow(
    dead_code,
    reason = "each test binary uses a different subset of the fixtures"
)]

use mandate_canon::Digest;
use mandate_identity::{AssertionId, PrincipalId, StepUpActionKind, WorkspaceId};
use mandate_passkey::stepup::{
    Action, ChallengeRecord, ChallengeState, Consumed, EnrolledCredential, Environment,
    Presentation, Proof, StepUpRefusal, consume,
};
use mandate_time::UtcNanos;

use crate::common::{Alg, Authenticator, Ceremony, OwnedAssertion, rp, sha256};

pub const CHALLENGE_ID: &str = "01J9ZQ4B7Y8K3M5N6P7Q8R9S0T";
pub const ALICE: &str = "01J9ZQ4B7Y8K3M5N6P7Q8R9S0A";
pub const MALLORY: &str = "01J9ZQ4B7Y8K3M5N6P7Q8R9S0M";
pub const WORKSPACE: &str = "01J9ZQ4B7Y8K3M5N6P7Q8R9S0W";
pub const OTHER_WORKSPACE: &str = "01J9ZQ4B7Y8K3M5N6P7Q8R9S0X";

/// Reads ULID text (Crockford base32) into its value, written out here rather than taken from
/// the crate.
pub fn ulid(text: &str) -> u128 {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    text.bytes().fold(0, |n, c| {
        let digit = ALPHABET
            .iter()
            .position(|a| *a == c)
            .expect("a Crockford digit");
        (n << 5) | digit as u128
    })
}
pub const ISSUED: &str = "2026-10-08T15:00:00.123456789Z";
pub const EXPIRES: &str = "2026-10-08T15:05:00.123456789Z";
/// When the fixture passkey's enrolment cool-off ended, a day before the challenge.
pub const COOL_OFF_ENDS: &str = "2026-10-07T15:00:00.000000000Z";

pub fn at(text: &str) -> UtcNanos {
    UtcNanos::parse(text).expect("a fixture instant")
}

/// `ISSUED` moved by `secs` seconds and `nanos` nanoseconds.
pub fn issued_plus(secs: i64, nanos: i64) -> UtcNanos {
    let issued = at(ISSUED);
    let total = i128::from(issued.secs()) * 1_000_000_000
        + i128::from(issued.nanos())
        + i128::from(secs) * 1_000_000_000
        + i128::from(nanos);
    UtcNanos::from_parts(
        (total / 1_000_000_000) as i64,
        (total % 1_000_000_000) as u32,
    )
    .expect("in range")
}

pub fn alice() -> PrincipalId {
    PrincipalId(ulid(ALICE))
}

pub fn mallory() -> PrincipalId {
    PrincipalId(ulid(MALLORY))
}

pub fn workspace() -> WorkspaceId {
    WorkspaceId(ulid(WORKSPACE))
}

pub fn other_workspace() -> WorkspaceId {
    WorkspaceId(ulid(OTHER_WORKSPACE))
}

pub fn challenge_id() -> AssertionId {
    AssertionId(CHALLENGE_ID.to_owned())
}

pub fn approval() -> Action {
    Action {
        kind: StepUpActionKind::Approve,
        digest: Digest::of(b"the approval's content object"),
    }
}

pub fn record() -> ChallengeRecord {
    ChallengeRecord::issue(challenge_id(), workspace(), alice(), approval(), at(ISSUED))
        .expect("a record")
}

/// The canonical form journal spec §4 gives the fixture record, written out by hand.
pub fn expected_canonical() -> String {
    let digest: String = sha256(b"the approval's content object")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!(
        concat!(
            r#"{{"action_digest":"sha256:{}","action_kind":"approve","#,
            r#""challenge_id":"{}","expires_at":"{}","issued_at":"{}","#,
            r#""principal_id":"{}","workspace_id":"{}"}}"#
        ),
        digest, CHALLENGE_ID, EXPIRES, ISSUED, ALICE, WORKSPACE
    )
}

/// The bytes the authenticator signs over for the fixture record.
pub fn challenge_bytes() -> Vec<u8> {
    sha256(expected_canonical().as_bytes())
}

pub enum Kind {
    Passkey,
    CliConfirm,
}

pub enum State {
    Unknown,
    Issued,
    Used,
}

/// One presentation of a step-up and everything `consume` reads.
pub struct Case {
    pub authenticator: Authenticator,
    pub environment: Environment,
    pub state: State,
    pub record: ChallengeRecord,
    pub principal: PrincipalId,
    pub workspace: WorkspaceId,
    pub action: Action,
    pub kind: Kind,
    pub assertion: OwnedAssertion,
    pub enrolled: Option<EnrolledCredential>,
    pub now: UtcNanos,
}

impl Case {
    pub fn new() -> Self {
        let authenticator = Authenticator::new(Alg::Es256);
        let assertion = authenticator.assert(&Ceremony::get(&challenge_bytes(), 8));
        let enrolled = EnrolledCredential {
            principal_id: alice(),
            credential: authenticator.credential(7),
            cool_off_ends: at(COOL_OFF_ENDS),
        };
        Self {
            authenticator,
            environment: Environment::Live,
            state: State::Issued,
            record: record(),
            principal: alice(),
            workspace: workspace(),
            action: approval(),
            kind: Kind::Passkey,
            assertion,
            enrolled: Some(enrolled),
            now: issued_plus(30, 0),
        }
    }

    /// Signs a fresh assertion over `bytes` with `ceremony`'s other fields.
    pub fn sign(&mut self, bytes: &[u8], change: impl FnOnce(&mut Ceremony)) {
        let mut ceremony = Ceremony::get(bytes, 8);
        change(&mut ceremony);
        self.assertion = self.authenticator.assert(&ceremony);
    }

    pub fn run(&self) -> Result<Consumed, StepUpRefusal> {
        let state = match self.state {
            State::Unknown => ChallengeState::Unknown,
            State::Issued => ChallengeState::Issued(&self.record),
            State::Used => ChallengeState::Used(&self.record),
        };
        let proof = match self.kind {
            Kind::Passkey => Proof::Passkey {
                assertion: self.assertion.view(),
                credential: self.enrolled.as_ref(),
            },
            Kind::CliConfirm => Proof::CliConfirm,
        };
        let presentation = Presentation {
            workspace_id: self.workspace,
            principal_id: self.principal,
            action: self.action,
            proof,
        };
        consume(&rp(), self.environment, state, &presentation, self.now)
    }
}
