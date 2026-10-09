//! Action-bound step-up (identity spec §7.2, §7.3; backlog E9-4; DEC-662): the challenge record,
//! its WebAuthn challenge, and `consume`'s checks, each refusal against a case every check passes.

mod common;
mod stepup;

use mandate_canon::Digest;
use mandate_identity::{StepUpActionKind, StepUpEvidence, StepUpMethod, WorkspaceId};
use mandate_passkey::stepup::{
    Action, Consumed, Environment, Missing, StepUpRefusal, Used, reverify,
};
use mandate_passkey::{Challenge, Refusal};

use common::{UV, rp};
use stepup::{Case, EXPIRES, Kind, State, at, challenge_bytes, issued_plus, record};

fn passkey_consumed(now: mandate_time::UtcNanos) -> Consumed {
    Consumed {
        evidence: StepUpEvidence {
            assertion_id: stepup::challenge_id(),
            authenticated_at: now,
            method: StepUpMethod::Passkey,
        },
        used: Used {
            challenge_id: stepup::challenge_id(),
        },
        sign_count: Some(8),
    }
}

#[test]
#[ignore = "pending E9-4"]
fn a_challenge_expires_three_hundred_seconds_after_it_is_issued() {
    assert_eq!(record().expires_at, at(EXPIRES));
}

#[test]
#[ignore = "pending E9-4"]
fn the_record_hashes_its_journal_canonical_form() {
    let record = record();
    assert_eq!(
        record.canonical().map(String::from_utf8),
        Ok(Ok(stepup::expected_canonical()))
    );
    assert_eq!(
        record.webauthn_challenge(),
        Ok(Challenge::new(&challenge_bytes()).expect("32 bytes"))
    );
}

#[test]
#[ignore = "pending E9-4"]
fn a_passkey_step_up_returns_the_evidence_and_the_used_marker_together() {
    for environment in [Environment::Live, Environment::Paper] {
        let mut case = Case::new();
        case.environment = environment;
        assert_eq!(
            case.run(),
            Ok(passkey_consumed(case.now)),
            "{environment:?}"
        );
    }
}

#[test]
#[ignore = "pending E9-4"]
fn cli_confirm_counts_in_paper_only() {
    let mut case = Case::new();
    case.kind = Kind::CliConfirm;
    case.environment = Environment::Paper;
    let expected = Consumed {
        evidence: StepUpEvidence {
            method: StepUpMethod::CliConfirm,
            ..passkey_consumed(case.now).evidence
        },
        sign_count: None,
        ..passkey_consumed(case.now)
    };
    assert_eq!(case.run(), Ok(expected));
    case.environment = Environment::Live;
    assert_eq!(case.run(), Err(StepUpRefusal::Method));
}

#[test]
#[ignore = "pending E9-4"]
fn an_unknown_challenge_is_missing_and_a_used_one_is_reused() {
    let mut case = Case::new();
    case.state = State::Unknown;
    assert_eq!(
        case.run(),
        Err(StepUpRefusal::Missing(Missing::UnknownChallenge))
    );
    case.state = State::Used;
    assert_eq!(case.run(), Err(StepUpRefusal::Reused));
}

#[test]
#[ignore = "pending E9-4"]
fn a_challenge_counts_from_its_issue_until_just_before_its_expiry() {
    let mut case = Case::new();
    for (now, accepted) in [
        (issued_plus(0, -1), false),
        (issued_plus(0, 0), true),
        (issued_plus(299, 999_999_999), true),
        (issued_plus(300, 0), false),
        (issued_plus(3_600, 0), false),
    ] {
        case.now = now;
        let result = case.run();
        if accepted {
            assert_eq!(result, Ok(passkey_consumed(now)), "{now}");
        } else {
            assert_eq!(result, Err(StepUpRefusal::Stale), "{now}");
        }
    }
}

#[test]
#[ignore = "pending E9-4"]
fn a_challenge_bound_to_another_principal_or_workspace_is_a_mismatch() {
    let mut case = Case::new();
    assert_eq!(
        case.run(),
        Ok(passkey_consumed(case.now)),
        "the unchanged case"
    );
    case.principal = stepup::mallory();
    assert_eq!(case.run(), Err(StepUpRefusal::Mismatch));
    let mut case = Case::new();
    case.workspace = WorkspaceId(stepup::ulid(stepup::OTHER_WORKSPACE));
    assert_eq!(case.run(), Err(StepUpRefusal::Mismatch));
}

#[test]
#[ignore = "pending E9-4"]
fn another_principals_credential_or_none_counts_as_missing() {
    let mut case = Case::new();
    assert_eq!(
        case.run(),
        Ok(passkey_consumed(case.now)),
        "the unchanged case"
    );
    case.enrolled = case
        .enrolled
        .map(|e| mandate_passkey::stepup::EnrolledCredential {
            principal_id: stepup::mallory(),
            ..e
        });
    assert_eq!(
        case.run(),
        Err(StepUpRefusal::Missing(Missing::NotTheirCredential))
    );
    case.enrolled = None;
    assert_eq!(
        case.run(),
        Err(StepUpRefusal::Missing(Missing::NotTheirCredential))
    );
}

#[test]
#[ignore = "pending E9-4"]
fn a_passkey_in_its_enrolment_cool_off_cannot_step_up() {
    let mut case = Case::new();
    let now = case.now;
    let enrolled = case.enrolled.clone().expect("enrolled");
    case.enrolled = Some(mandate_passkey::stepup::EnrolledCredential {
        cool_off_ends: issued_plus(30, 1),
        ..enrolled.clone()
    });
    assert_eq!(case.run(), Err(StepUpRefusal::Missing(Missing::CoolingOff)));
    case.enrolled = Some(mandate_passkey::stepup::EnrolledCredential {
        cool_off_ends: now,
        ..enrolled
    });
    assert_eq!(case.run(), Ok(passkey_consumed(now)));
}

#[test]
#[ignore = "pending E9-4"]
fn an_assertion_that_does_not_verify_counts_as_missing_with_its_reason() {
    let mut case = Case::new();
    case.sign(&common::sha256(b"another record"), |_| {});
    let refused = |r| Err(StepUpRefusal::Missing(Missing::Passkey(r)));
    assert_eq!(case.run(), refused(Refusal::ChallengeMismatch));
    case.sign(&challenge_bytes(), |c| c.flags &= !UV);
    assert_eq!(case.run(), refused(Refusal::UserNotVerified));
    case.sign(&challenge_bytes(), |c| c.counter = 7);
    assert_eq!(case.run(), refused(Refusal::CounterNotRising));
}

#[test]
#[ignore = "pending E9-4"]
fn a_step_up_for_another_action_is_a_mismatch() {
    let mut case = Case::new();
    assert_eq!(
        case.run(),
        Ok(passkey_consumed(case.now)),
        "the unchanged case"
    );
    case.action = Action {
        digest: Digest::of(b"another approval's content object"),
        ..case.action
    };
    assert_eq!(case.run(), Err(StepUpRefusal::Mismatch));
    let mut case = Case::new();
    case.action = Action {
        kind: StepUpActionKind::Deploy,
        ..case.action
    };
    assert_eq!(case.run(), Err(StepUpRefusal::Mismatch));
}

#[test]
#[ignore = "pending E9-4"]
fn two_failures_refuse_with_the_first_in_dec_662_order() {
    let mut case = Case::new();
    case.state = State::Used;
    case.now = issued_plus(301, 0);
    assert_eq!(
        case.run(),
        Err(StepUpRefusal::Reused),
        "reuse before staleness"
    );

    let mut case = Case::new();
    case.kind = Kind::CliConfirm;
    case.state = State::Unknown;
    assert_eq!(
        case.run(),
        Err(StepUpRefusal::Method),
        "method before existence"
    );

    let mut case = Case::new();
    case.now = issued_plus(301, 0);
    case.principal = stepup::mallory();
    assert_eq!(
        case.run(),
        Err(StepUpRefusal::Stale),
        "staleness before the principal"
    );

    let mut case = Case::new();
    case.principal = stepup::mallory();
    case.enrolled = None;
    assert_eq!(
        case.run(),
        Err(StepUpRefusal::Mismatch),
        "the principal before the credential"
    );

    let mut case = Case::new();
    case.enrolled = case
        .enrolled
        .map(|e| mandate_passkey::stepup::EnrolledCredential {
            cool_off_ends: issued_plus(60, 0),
            ..e
        });
    case.sign(&common::sha256(b"another record"), |_| {});
    assert_eq!(
        case.run(),
        Err(StepUpRefusal::Missing(Missing::CoolingOff)),
        "the cool-off before the signature"
    );

    let mut case = Case::new();
    case.sign(&common::sha256(b"another record"), |_| {});
    case.action = Action {
        kind: StepUpActionKind::Deploy,
        ..case.action
    };
    let refused = Err(StepUpRefusal::Missing(Missing::Passkey(
        Refusal::ChallengeMismatch,
    )));
    assert_eq!(case.run(), refused, "the signature before the action");
}

#[test]
#[ignore = "pending E9-4"]
fn a_stored_assertion_reverifies_against_the_public_key_after_its_counter_moved_on() {
    let case = Case::new();
    let consumed = case.run().expect("the step-up counts");
    let stored = mandate_passkey::Credential {
        sign_count: consumed.sign_count.expect("a passkey counter"),
        ..case.enrolled.clone().expect("enrolled").credential
    };
    let raw = case.assertion.view();
    assert!(reverify(&rp(), &case.record, &stored, &raw).is_ok());
    let mut other = record();
    other.principal_id = stepup::mallory();
    assert_eq!(
        reverify(&rp(), &other, &stored, &raw),
        Err(StepUpRefusal::Missing(Missing::Passkey(
            Refusal::ChallengeMismatch
        )))
    );
}
