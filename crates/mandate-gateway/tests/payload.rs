//! `CallRecord::agent_payload` refuses what the agent stream would refuse at append (journal spec
//! §9.19 rules 148 and 151, and the compiler's record on the control stream).

mod common;

use common::*;
use mandate_gateway::guard::GuardRule;
use mandate_gateway::{
    CallRecord, CandidateKind, Outcome, PayloadError, Purpose, Verdict, VerdictResult,
};

fn ok_record() -> CallRecord {
    let (mut gateway, model) = gateway();
    gateway.transport_mut().script(vec![ok_reply(10)]);
    gateway.call(&request(&model, 1)).unwrap()
}

fn v(kind: CandidateKind, result: VerdictResult) -> Verdict {
    Verdict { kind, result }
}

#[test]
fn a_well_formed_record_has_a_payload() {
    let record = ok_record();
    let verdicts = [
        v(CandidateKind::Thesis, VerdictResult::Judged),
        v(CandidateKind::Thesis, VerdictResult::UnknownEvidence),
        v(CandidateKind::Invalidation, VerdictResult::UnknownThesis),
    ];
    assert!(record.agent_payload(&verdicts).is_ok());
}

#[test]
fn the_compilers_record_is_not_an_agent_stream_payload() {
    let mut record = ok_record();
    record.purpose = Purpose::Compiler;
    assert_eq!(
        record.agent_payload(&[]),
        Err(PayloadError::CompilerOnAgentStream)
    );
}

#[test]
fn rule_148_members_must_agree_with_the_outcome() {
    let base = ok_record();
    let mut no_prompt = base.clone();
    no_prompt.prompt_ref = None;
    let mut no_digest = base.clone();
    no_digest.request_digest = None;
    let mut ruled = base.clone();
    ruled.guard_rule = Some(GuardRule::Credential);
    let mut kept = base.clone();
    kept.outcome = Outcome::InputRejected;
    kept.guard_rule = Some(GuardRule::Credential);
    kept.request_digest = None;
    let mut digest_kept = base.clone();
    digest_kept.outcome = Outcome::InputRejected;
    digest_kept.guard_rule = Some(GuardRule::Credential);
    digest_kept.prompt_ref = None;
    let mut unruled = base.clone();
    unruled.outcome = Outcome::InputRejected;
    unruled.prompt_ref = None;
    unruled.request_digest = None;
    for record in [no_prompt, no_digest, ruled, kept, digest_kept, unruled] {
        assert_eq!(record.agent_payload(&[]), Err(PayloadError::GuardMembers));
    }
}

#[test]
fn rule_151_verdicts_only_for_a_research_ok_theses_first_each_fitting_its_kind() {
    let judged = [v(CandidateKind::Thesis, VerdictResult::Judged)];
    let mut signal = ok_record();
    signal.purpose = Purpose::LlmSignal;
    assert_eq!(signal.agent_payload(&judged), Err(PayloadError::Verdicts));
    assert!(signal.agent_payload(&[]).is_ok());
    let mut failed = ok_record();
    failed.outcome = Outcome::SchemaInvalid;
    failed.output = None;
    assert_eq!(failed.agent_payload(&judged), Err(PayloadError::Verdicts));
    let record = ok_record();
    let out_of_order = [
        v(CandidateKind::Invalidation, VerdictResult::Judged),
        v(CandidateKind::Thesis, VerdictResult::Judged),
    ];
    assert_eq!(
        record.agent_payload(&out_of_order),
        Err(PayloadError::Verdicts)
    );
    for bad in [
        v(CandidateKind::Thesis, VerdictResult::UnknownThesis),
        v(
            CandidateKind::Invalidation,
            VerdictResult::UnknownInstrument,
        ),
    ] {
        assert_eq!(
            record.agent_payload(&[bad]),
            Err(PayloadError::Verdicts),
            "{bad:?}"
        );
    }
}

#[test]
fn each_result_fits_exactly_the_kinds_rule_151_allows() {
    use CandidateKind::{Invalidation, Thesis};
    let table = [
        (VerdictResult::Judged, true, true),
        (VerdictResult::ForbiddenLanguage, true, true),
        (VerdictResult::UnknownThesis, false, true),
        (VerdictResult::UnknownInstrument, true, false),
        (VerdictResult::UnknownEvidence, true, false),
        (VerdictResult::DuplicateInstrument, true, false),
        (VerdictResult::RevisionsNotEnabled, true, false),
    ];
    for (result, thesis, invalidation) in table {
        assert_eq!(result.fits(Thesis), thesis, "{result:?}");
        assert_eq!(result.fits(Invalidation), invalidation, "{result:?}");
    }
}

/// An evaluation is the platform's call, with no agent (DEC-2216), so its record is no agent
/// stream's payload either.
#[test]
fn an_evaluations_record_is_not_an_agent_stream_payload() {
    let mut record = ok_record();
    record.purpose = Purpose::Evaluation;
    assert_eq!(
        record.agent_payload(&[]),
        Err(PayloadError::NoAgentOnAgentStream)
    );
}
