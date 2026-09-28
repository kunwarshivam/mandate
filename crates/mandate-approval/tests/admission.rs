//! E8-3's admission and step-up clauses of the
//! [M7 brief](../../../docs/project/tasks/M7-escalation-v0.md) (checks 1 to 7, DEC-156 items 1, 7,
//! and 8, DEC-158 option (c), DEC-165 items 4 and 5), with E8-2's lateness: one named test per
//! clause, each naming the MC-E case or planted bug (PB-n) it stands for.
//!
//! Every test but the live one at the end is pending until the implementation PR and fails on
//! `ApprovalError::Unimplemented` (DEC-77, DEC-110).

mod common;

use std::collections::BTreeSet;
use std::num::NonZeroU8;

use common::{
    AUTHOR, DEADLINE, OWNER, SECOND, T0, answer, bound, ctx, grant, hash, price, request, skip,
    step_up, user,
};
use mandate_approval::{
    ActorKind, Admission, ApprovalRef, AssertionId, CommandAuthority, Environment, KillScope,
    KillSwitchAuthority, OwnerCommandKind, Refusal, RiskClock, StepUpRefusal, Verdict, admit,
    kill_switch, kill_switch_code, owner_command,
};
use mandate_num::Price;

fn admitted(
    pending: Option<&mandate_approval::Request>,
    response: &mandate_approval::Response,
) -> Admission {
    answer("admit", admit(pending, response, &ctx()))
}

/// The paired positive of every one-sided refusal below: the fixture each test varies is itself
/// admitted, so a stub that refuses one constant reason passes none of them.
fn the_unchanged_fixture_is_admitted() {
    assert_eq!(
        admitted(Some(&request()), &grant()),
        Admission::Admitted,
        "the unchanged fixture is admitted"
    );
}

/// MC-E01, MC-E03: a timely grant from a listed user with fresh evidence is admitted; the same
/// grant read after the deadline is not, so silence and lateness never admit.
#[test]
fn a_timely_grant_is_admitted_and_a_late_one_is_not() {
    assert_eq!(admitted(Some(&request()), &grant()), Admission::Admitted);
    let mut c = ctx();
    c.folded_clock = RiskClock(DEADLINE + 1);
    assert_eq!(
        answer("admit", admit(Some(&request()), &grant(), &c)),
        Admission::Refused(Refusal::Late)
    );
}

/// MC-E02, PX-7: a skip needs no step-up, and one skip ends the approval whatever the quorum.
#[test]
fn a_skip_needs_no_step_up_and_ignores_the_quorum() {
    let mut two = request();
    two.content.bound.approvers_required = NonZeroU8::new(2).unwrap();
    assert_eq!(admitted(Some(&request()), &skip()), Admission::Admitted);
    assert_eq!(admitted(Some(&two), &skip()), Admission::Admitted);
    assert_eq!(
        admitted(Some(&two), &grant()),
        Admission::Counted,
        "a grant still needs two"
    );
}

/// MC-E08, check 1: a response to an approval no longer pending is refused, and so is one that
/// names another approval (a response copied onto a different request).
#[test]
fn a_response_to_an_approval_not_pending_is_refused() {
    assert_eq!(
        admitted(None, &grant()),
        Admission::Refused(Refusal::NotPending)
    );
    assert_eq!(
        admitted(None, &skip()),
        Admission::Refused(Refusal::NotPending)
    );
    assert_eq!(
        admitted(Some(&request()), &skip()),
        Admission::Admitted,
        "the same skip, pending"
    );
    for response in [grant(), skip()] {
        let elsewhere = mandate_approval::Response {
            approval: ApprovalRef::of_requested_event("01J9ZQ4Y8N6K3V5T2R1M0P7XWZ").unwrap(),
            ..response
        };
        assert_eq!(
            admitted(Some(&request()), &elsewhere),
            Admission::Refused(Refusal::NotPending)
        );
    }
}

/// MC-E04, PB-3, check 2: a response at exactly the deadline is late; a second before is not.
#[test]
fn a_response_at_exactly_the_deadline_is_late() {
    let mut at = grant();
    at.submitted_at = RiskClock(DEADLINE);
    at.verdict = Verdict::Approve(Some(step_up("assertion-1", DEADLINE - 1)));
    assert_eq!(
        admitted(Some(&request()), &at),
        Admission::Refused(Refusal::Late)
    );
    at.submitted_at = RiskClock(DEADLINE - 1);
    let mut c = ctx();
    c.folded_clock = RiskClock(DEADLINE - 1);
    assert_eq!(
        answer("admit", admit(Some(&request()), &at, &c)),
        Admission::Admitted
    );
}

/// MC-E05, PB-2, EI-15: lateness is judged at the later of `submitted_at` and the folded clock.
#[test]
fn lateness_uses_the_later_of_submitted_and_the_folded_clock() {
    the_unchanged_fixture_is_admitted();
    let mut c = ctx();
    c.folded_clock = RiskClock(DEADLINE + 60);
    let early = grant();
    assert_eq!(
        answer("admit", admit(Some(&request()), &early, &c)),
        Admission::Refused(Refusal::Late)
    );
    let mut s = skip();
    s.submitted_at = RiskClock(T0);
    assert_eq!(
        answer("admit", admit(Some(&request()), &s, &c)),
        Admission::Refused(Refusal::Late)
    );
}

/// MC-E09 to MC-E12, PB-10, EI-10: only a user listed in `approvers` can answer.
#[test]
fn every_actor_but_a_listed_user_is_refused() {
    the_unchanged_fixture_is_admitted();
    for kind in [
        ActorKind::System,
        ActorKind::Agent,
        ActorKind::Broker,
        ActorKind::PlatformOperator,
    ] {
        for response in [grant(), skip()] {
            let r = mandate_approval::Response {
                actor_kind: kind,
                ..response
            };
            assert_eq!(
                admitted(Some(&request()), &r),
                Admission::Refused(Refusal::NotAnApprover),
                "{kind:?}"
            );
        }
    }
    let stranger = mandate_approval::Response {
        responder: user("user-stranger"),
        ..grant()
    };
    assert_eq!(
        admitted(Some(&request()), &stranger),
        Admission::Refused(Refusal::NotAnApprover)
    );
}

/// PB-20, EI-16, check 4: a request no channel delivered cannot be granted or skipped.
#[test]
fn an_undelivered_request_is_refused() {
    the_unchanged_fixture_is_admitted();
    let mut r = request();
    r.delivered = false;
    for response in [grant(), skip()] {
        assert_eq!(
            admitted(Some(&r), &response),
            Admission::Refused(Refusal::NotDelivered)
        );
    }
}

/// MC-E07, PB-15, EI-14, check 5: a response must repeat the request's content hash.
#[test]
fn a_wrong_content_hash_is_refused() {
    the_unchanged_fixture_is_admitted();
    for response in [grant(), skip()] {
        let r = mandate_approval::Response {
            content_hash: hash("other"),
            ..response
        };
        assert_eq!(
            admitted(Some(&request()), &r),
            Admission::Refused(Refusal::ContentMismatch)
        );
    }
}

/// MC-E13, check 6: a grant without step-up evidence is refused.
#[test]
fn a_grant_without_step_up_is_refused() {
    the_unchanged_fixture_is_admitted();
    let r = mandate_approval::Response {
        verdict: Verdict::Approve(None),
        ..grant()
    };
    assert_eq!(
        admitted(Some(&request()), &r),
        Admission::Refused(Refusal::StepUpMissing)
    );
}

/// MC-E14, PB-11: evidence is fresh for 300 s before the effective time, inclusive.
#[test]
fn step_up_is_judged_at_the_effective_time_and_300_s_is_fresh() {
    let mut c = ctx();
    c.folded_clock = RiskClock(T0 + 200);
    let with = |authenticated_at| mandate_approval::Response {
        verdict: Verdict::Approve(Some(step_up("assertion-1", authenticated_at))),
        submitted_at: RiskClock(T0 + 1),
        ..grant()
    };
    assert_eq!(
        answer("admit", admit(Some(&request()), &with(T0 - 100), &c)),
        Admission::Admitted
    );
    assert_eq!(
        answer("admit", admit(Some(&request()), &with(T0 - 101), &c)),
        Admission::Refused(Refusal::StepUpStale)
    );
}

/// MC-E15, PB-12: an assertion is used once per workspace.
#[test]
fn a_reused_assertion_is_refused() {
    the_unchanged_fixture_is_admitted();
    let mut c = ctx();
    c.used_assertions
        .insert(AssertionId("assertion-1".to_owned()));
    assert_eq!(
        answer("admit", admit(Some(&request()), &grant(), &c)),
        Admission::Refused(Refusal::StepUpReused)
    );
}

/// MC-E16, PB-19: `CliConfirm` never satisfies a live stream.
#[test]
fn cli_confirm_is_refused_for_a_live_stream() {
    the_unchanged_fixture_is_admitted();
    let mut c = ctx();
    c.environment = Environment::Live;
    assert_eq!(
        answer("admit", admit(Some(&request()), &grant(), &c)),
        Admission::Refused(Refusal::StepUpMethod)
    );
}

/// PB-22, check 7: a first grant of two is counted; a second distinct approver admits.
#[test]
fn a_first_grant_of_two_is_counted_and_a_second_approver_admits() {
    let mut r = request();
    r.content.bound.approvers_required = NonZeroU8::new(2).unwrap();
    assert_eq!(admitted(Some(&r), &grant()), Admission::Counted);
    r.grants.insert(user(OWNER));
    let second = mandate_approval::Response {
        responder: user(SECOND),
        verdict: Verdict::Approve(Some(step_up("assertion-2", T0 + 5))),
        ..grant()
    };
    assert_eq!(admitted(Some(&r), &second), Admission::Admitted);
}

/// PB-18: the same approver counts once.
#[test]
fn same_approver_counts_once() {
    the_unchanged_fixture_is_admitted();
    let mut r = request();
    r.content.bound.approvers_required = NonZeroU8::new(2).unwrap();
    r.grants.insert(user(OWNER));
    let again = mandate_approval::Response {
        verdict: Verdict::Approve(Some(step_up("assertion-2", T0 + 5))),
        ..grant()
    };
    assert_eq!(
        admitted(Some(&r), &again),
        Admission::Refused(Refusal::DuplicateApprover)
    );
}

/// Mandate spec §6.4: with independent approval, the mandate's author cannot grant.
#[test]
fn the_author_cannot_grant_an_independent_approval() {
    let mut r = request();
    r.content.bound.independent_required = true;
    let mut c = ctx();
    c.approvers.insert(user(AUTHOR));
    let by_author = mandate_approval::Response {
        responder: user(AUTHOR),
        ..grant()
    };
    assert_eq!(
        answer("admit", admit(Some(&r), &by_author, &c)),
        Admission::Refused(Refusal::NotIndependent)
    );
    assert_eq!(
        answer("admit", admit(Some(&r), &grant(), &c)),
        Admission::Admitted
    );
}

/// DEC-156 item 2: the first failing check in the brief's order names the refusal.
#[test]
fn the_first_failing_admission_check_names_the_refusal() {
    let late_and_wrong = mandate_approval::Response {
        submitted_at: RiskClock(DEADLINE),
        content_hash: hash("other"),
        verdict: Verdict::Approve(None),
        ..grant()
    };
    assert_eq!(
        admitted(Some(&request()), &late_and_wrong),
        Admission::Refused(Refusal::Late)
    );
    let stranger_without_evidence = mandate_approval::Response {
        responder: user("user-stranger"),
        verdict: Verdict::Approve(None),
        ..grant()
    };
    assert_eq!(
        admitted(Some(&request()), &stranger_without_evidence),
        Admission::Refused(Refusal::NotAnApprover)
    );
}

fn command(
    kind: OwnerCommandKind,
    evidence: Option<&mandate_approval::StepUp>,
    committed: i64,
    processed: i64,
) -> CommandAuthority {
    answer(
        "owner_command",
        owner_command(
            kind,
            evidence,
            RiskClock(committed),
            RiskClock(processed),
            Environment::Paper,
            &BTreeSet::new(),
        ),
    )
}

/// PX-4, PB-14: pause needs no step-up, however late it is read; resume, at the same moment,
/// does.
#[test]
fn pause_needs_no_step_up_and_resume_does() {
    assert_eq!(
        command(OwnerCommandKind::Pause, None, T0, T0 + 3600),
        CommandAuthority::Apply
    );
    assert_eq!(
        command(OwnerCommandKind::Resume, None, T0, T0 + 3600),
        CommandAuthority::Refused(StepUpRefusal::Missing)
    );
}

/// DEC-156 item 8, EI-11: resume, Stop, and acknowledge are judged when the runtime reads them.
#[test]
fn resume_stop_and_acknowledge_are_judged_when_processed() {
    let evidence = step_up("assertion-1", T0);
    for kind in [
        OwnerCommandKind::Resume,
        OwnerCommandKind::Stop,
        OwnerCommandKind::Acknowledge,
    ] {
        assert_eq!(
            command(kind, Some(&evidence), T0 + 10, T0 + 300),
            CommandAuthority::Apply
        );
        assert_eq!(
            command(kind, Some(&evidence), T0 + 10, T0 + 301),
            CommandAuthority::Refused(StepUpRefusal::Stale),
            "{kind:?}"
        );
        assert_eq!(
            command(kind, None, T0 + 10, T0 + 10),
            CommandAuthority::Refused(StepUpRefusal::Missing)
        );
    }
}

/// DEC-156 item 8: an owner exit is judged when the owner committed it, so a late read applies it
/// and evidence already stale at commit is refused.
#[test]
fn an_owner_exit_is_judged_when_the_owner_committed_it() {
    let evidence = step_up("assertion-1", T0);
    assert_eq!(
        command(
            OwnerCommandKind::OwnerExit,
            Some(&evidence),
            T0 + 300,
            T0 + 86_400
        ),
        CommandAuthority::Apply
    );
    assert_eq!(
        command(
            OwnerCommandKind::OwnerExit,
            Some(&evidence),
            T0 + 301,
            T0 + 301
        ),
        CommandAuthority::Refused(StepUpRefusal::Stale)
    );
}

/// PB-14b, EI-8, EI-11, DEC-158 option (c): a kill switch is never refused. Without valid evidence
/// it still stops and flattens as an automated flatten does; with it, the owner-exit privileges
/// apply too, however late it is read.
#[test]
fn a_kill_switch_without_step_up_still_stops_and_flattens() {
    let used: BTreeSet<AssertionId> = [AssertionId("assertion-used".to_owned())].into();
    let judge = |evidence: Option<mandate_approval::StepUp>, env| {
        answer(
            "kill_switch",
            kill_switch(evidence.as_ref(), RiskClock(T0 + 300), env, &used),
        )
    };
    let cases = [
        (None, Environment::Paper, StepUpRefusal::Missing),
        (
            Some(step_up("assertion-1", T0 - 1)),
            Environment::Paper,
            StepUpRefusal::Stale,
        ),
        (
            Some(step_up("assertion-used", T0 + 1)),
            Environment::Paper,
            StepUpRefusal::Reused,
        ),
        (
            Some(step_up("assertion-1", T0 + 1)),
            Environment::Live,
            StepUpRefusal::Method,
        ),
    ];
    for (evidence, env, why) in cases {
        assert_eq!(
            judge(evidence, env),
            KillSwitchAuthority::AutomatedFlatten(why)
        );
    }
    assert_eq!(
        judge(Some(step_up("assertion-1", T0)), Environment::Paper),
        KillSwitchAuthority::OwnerExitPrivileges
    );
}

/// DEC-155 item 4, rule 13: the kill switch's code is computed locally from the scope typed and
/// the control stream's head, so it is the same on every host for the same command and confirms
/// neither another scope nor a command made after another one landed.
#[test]
fn the_kill_switch_code_is_bound_to_its_scope_and_the_control_head() {
    let code = |scope: &KillScope, head| answer("kill_switch_code", kill_switch_code(scope, head));
    let agent = KillScope::Agent("agent-1".to_owned());
    let base = code(&agent, 41);
    assert_eq!(
        base,
        code(&agent, 41),
        "the same command gives the same code"
    );
    assert!(!base.0.is_empty());
    let others = [
        code(&agent, 42),
        code(&KillScope::Agent("agent-2".to_owned()), 41),
        code(&KillScope::Connection("agent-1".to_owned()), 41),
        code(&KillScope::Workspace, 41),
    ];
    for (i, other) in others.iter().enumerate() {
        assert_ne!(*other, base, "variant {i} confirms the same code");
    }
}

/// Live: the fixture's grant is the one every check above varies, and it binds the fixture's
/// order, so a pending test's failure is about the check its title names.
#[test]
fn the_fixtures_describe_one_consistent_request() {
    let r = request();
    let g = grant();
    assert_eq!(g.approval, r.id);
    assert_eq!(g.content_hash, r.content_hash);
    assert!(g.submitted_at < r.content.deadline);
    assert!(ctx().approvers.contains(&user(OWNER)));
    assert_eq!(r.content.bound, bound());
    assert_eq!(
        r.content
            .bound
            .qty
            .notional(r.content.bound.limit)
            .unwrap()
            .to_string(),
        "1872.5"
    );
    let _: Price = price("1");
}
