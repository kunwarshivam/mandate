//! The step-up refusal codes (identity spec §7.2 step 6; mandate spec §6.1; DEC-662), live from
//! the tests PR on so the mutation gate has a test to judge the module with (DEC-139), and the
//! smallest pending check of the challenge record: its whole canonical form, written out by hand.

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
    let expected = concat!(
        r#"{"action_digest":"sha256:"#,
        "51d4aa49ce08cc98986749509d978f4310c38472511da03d6419d9b945071307",
        r#"","action_kind":"approve","challenge_id":"01J9ZQ4B7Y8K3M5N6P7Q8R9S0T","#,
        r#""expires_at":"1970-01-01T00:05:00.000000000Z","#,
        r#""issued_at":"1970-01-01T00:00:00.000000000Z","#,
        r#""principal_id":"00000000000000000000000002","#,
        r#""workspace_id":"00000000000000000000000001"}"#,
    );
    assert_eq!(
        record.canonical().map(String::from_utf8),
        Ok(Ok(expected.to_owned())),
        "the digest is SHA-256 of the content object, worked out apart from the crate",
    );
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
