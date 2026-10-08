//! The step-up refusal codes (identity spec §7.2 step 6; mandate spec §6.1; DEC-662), live from
//! the tests PR on so the mutation gate has a test to judge the module with (DEC-139), and the
//! smallest pending check of the challenge record, which `tests/consume.rs` covers in full.

use mandate_canon::Digest;
use mandate_identity::{AssertionId, PrincipalId, StepUpActionKind, WorkspaceId};
use mandate_passkey::stepup::{Action, ChallengeRecord, Missing, StepUpRefusal};
use mandate_time::UtcNanos;

#[test]
#[ignore = "pending E9-4"]
fn a_record_issued_at_the_epoch_has_a_canonical_form() {
    let action = Action {
        kind: StepUpActionKind::Approve,
        digest: Digest::of(b"an approval's content object"),
    };
    let record = ChallengeRecord::issue(
        AssertionId("01J9ZQ4B7Y8K3M5N6P7Q8R9S0T".to_owned()),
        WorkspaceId(1),
        PrincipalId(2),
        action,
        UtcNanos::EPOCH,
    )
    .expect("a record");
    let canonical = record.canonical().expect("ULID text for every identifier");
    assert!(canonical.starts_with(br#"{"action_digest":"sha256:"#));
    assert!(canonical.ends_with(br#""workspace_id":"00000000000000000000000001"}"#));
}

#[test]
fn every_step_up_refusal_has_its_api_code() {
    let codes = [
        (
            StepUpRefusal::Missing(Missing::UnknownChallenge),
            "step_up_missing",
        ),
        (
            StepUpRefusal::Missing(Missing::CoolingOff),
            "step_up_missing",
        ),
        (StepUpRefusal::Stale, "step_up_stale"),
        (StepUpRefusal::Reused, "step_up_reused"),
        (StepUpRefusal::Method, "step_up_method"),
        (StepUpRefusal::Mismatch, "step_up_mismatch"),
    ];
    for (refusal, code) in codes {
        assert_eq!(refusal.code(), code);
    }
}
