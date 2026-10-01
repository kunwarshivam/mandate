//! Check 7 against the workspace policy overlay (mandate spec §6.4 check 7, DEC-173 item 13; the
//! #321 review's major, backlog E8-3's follow-up): the requirement is the stricter of the bound
//! `approvers` and the overlay current at the effective time. Independence is required if either
//! requires it, the approver count is the larger of the two, and an author's earlier `counted`
//! grant stops counting once independence is required.
//!
//! The hand cases are `reference/mandate/ref.py`'s `approval_quorum` and `approval_admit`, and the
//! three scripts `fuzz_policy_quorum` pins; the property's oracle computes the order value and the
//! ceiling on its own scaled integers and keeps its own grant count (AGENTS.md, "Independent
//! oracles"). Every test but the live ones at the end is pending until the implementation PR and
//! fails on `quorum`'s `ApprovalError::Unimplemented` (DEC-77, DEC-110).

mod common;

use std::collections::BTreeSet;
use std::num::NonZeroU8;

use common::{AUTHOR, DEADLINE, OWNER, SECOND, answer, ctx, grant, request, skip, step_up, user};
use mandate_approval::{
    Admission, AdmissionContext, PolicyOverlay, Quorum, Refusal, Request, Response, RiskClock,
    Verdict, admit, quorum,
};
use mandate_num::{Price, Qty, Usd};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

fn check<S>(strategy: S, body: impl Fn(S::Value) -> Result<(), TestCaseError>)
where
    S: Strategy,
    S::Value: std::fmt::Debug,
{
    let config = ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    if let Err(failure) = proptest::test_runner::TestRunner::new(config).run(&strategy, body) {
        panic!("{failure}");
    }
}

fn fail<E: std::fmt::Debug>(what: &str) -> impl FnOnce(E) -> TestCaseError + '_ {
    move |e| TestCaseError::fail(format!("{what} answers, not {e:?}"))
}

fn overlay(independent: bool, ceiling: Option<&str>) -> PolicyOverlay {
    PolicyOverlay {
        independent_approval_required: independent,
        two_approver_above_usd: ceiling.map(|c| Usd::parse(c).unwrap()),
    }
}

fn count(n: u8) -> NonZeroU8 {
    NonZeroU8::new(n).unwrap()
}

/// The fixture's request, whose order value is 10 × 187.25 = 1872.5, with the bound requirement
/// and the grant set already folded.
fn pending_with(required: u8, independent: bool, grants: &[&str]) -> Request {
    let mut r = request();
    r.content.bound.approvers_required = count(required);
    r.content.bound.independent_required = independent;
    r.grants = grants.iter().map(|g| user(g)).collect();
    r
}

/// The fixture's context, with the author listed as an approver too, so only independence can
/// refuse the author's grant.
fn under(policy: PolicyOverlay) -> AdmissionContext {
    let mut c = ctx();
    c.approvers.insert(user(AUTHOR));
    c.policy = policy;
    c
}

/// A fresh, singly stepped-up grant from `who`.
fn grant_by(who: &str) -> Response {
    Response {
        responder: user(who),
        ..grant()
    }
}

fn judged(pending: &Request, who: &str, policy: PolicyOverlay) -> Admission {
    answer(
        "admit",
        admit(Some(pending), &grant_by(who), &under(policy)),
    )
}

fn required(r: &Request, policy: PolicyOverlay) -> Quorum {
    answer("quorum", quorum(&r.content.bound, &policy))
}

fn quorum_of(approvers: u8, independent: bool) -> Quorum {
    Quorum {
        approvers_required: count(approvers),
        independent_required: independent,
    }
}

/// Check 7, `approval_quorum`: independence is required if the request bound it or the overlay
/// requires it, and the count is unchanged by independence alone.
#[test]
fn independence_is_required_if_the_request_or_the_overlay_requires_it() {
    for (bound, by_policy, want) in [
        (false, false, false),
        (true, false, true),
        (false, true, true),
        (true, true, true),
    ] {
        for approvers in [1, 2] {
            assert_eq!(
                required(
                    &pending_with(approvers, bound, &[]),
                    overlay(by_policy, None)
                ),
                quorum_of(approvers, want),
                "bound {bound}, policy {by_policy}, {approvers} approver(s)"
            );
        }
    }
}

/// Check 7, `approval_quorum`: a ceiling below the order value asks for two; one at it, or above
/// it by the smallest unit, does not, since only an order value that exceeds it counts.
#[test]
fn a_ceiling_below_the_order_value_asks_for_two_and_one_at_it_does_not() {
    let one = pending_with(1, false, &[]);
    for (ceiling, want) in [
        ("1872.499999999", 2),
        ("100", 2),
        ("0", 2),
        ("1872.5", 1),
        ("1872.500000001", 1),
        ("100000", 1),
    ] {
        assert_eq!(
            required(&one, overlay(false, Some(ceiling))),
            quorum_of(want, false),
            "ceiling {ceiling} against an order of 1872.5"
        );
    }
}

/// DEC-173 item 13: an overlay that turns maker-checker off or raises the ceiling leaves the
/// bound requirement in force; the larger count and either independence win.
#[test]
fn a_loosened_overlay_leaves_the_bound_requirement_in_force() {
    let strict = pending_with(2, true, &[]);
    for policy in [
        overlay(false, Some("100000")),
        overlay(false, Some("1872.5")),
        overlay(true, Some("100000")),
    ] {
        assert_eq!(required(&strict, policy), quorum_of(2, true), "{policy:?}");
    }
}

/// Check 7, DEC-173 item 13: an admin who turns maker-checker on while an approval is pending
/// binds it, so the author can no longer grant it; another approver still can.
#[test]
fn maker_checker_turned_on_while_pending_refuses_the_author() {
    let open = pending_with(1, false, &[]);
    assert_eq!(
        judged(&open, AUTHOR, PolicyOverlay::NONE),
        Admission::Admitted,
        "with no policy the author of a mandate that binds no independence may grant"
    );
    let maker_checker = overlay(true, None);
    assert_eq!(
        judged(&open, AUTHOR, maker_checker),
        Admission::Refused(Refusal::NotIndependent)
    );
    assert_eq!(judged(&open, OWNER, maker_checker), Admission::Admitted);
}

/// `fuzz_policy_quorum`'s first pinned script: two approvers bound, the author's grant counted,
/// then maker-checker turned on. The author's grant stops counting, so the next approver's grant
/// is counted, not admitted; a third, independent approver admits.
#[test]
fn the_authors_earlier_counted_grant_stops_counting_once_independence_is_required() {
    assert_eq!(
        judged(&pending_with(2, false, &[]), AUTHOR, PolicyOverlay::NONE),
        Admission::Counted,
        "the author's grant counted before the policy changed"
    );
    let maker_checker = overlay(true, None);
    assert_eq!(
        judged(&pending_with(2, false, &[AUTHOR]), OWNER, maker_checker),
        Admission::Counted
    );
    assert_eq!(
        judged(
            &pending_with(2, false, &[AUTHOR, OWNER]),
            SECOND,
            maker_checker
        ),
        Admission::Admitted
    );
}

/// `fuzz_policy_quorum`'s second pinned script: a ceiling lowered below the order value while the
/// approval is pending raises its count to two, so one grant is counted and a second admits; a
/// ceiling at the order value leaves one grant enough.
#[test]
fn a_lowered_ceiling_while_pending_makes_one_grant_counted() {
    let lowered = overlay(false, Some("1872.49"));
    assert_eq!(
        judged(&pending_with(1, false, &[]), OWNER, lowered),
        Admission::Counted
    );
    assert_eq!(
        judged(&pending_with(1, false, &[SECOND]), OWNER, lowered),
        Admission::Admitted
    );
    assert_eq!(
        judged(
            &pending_with(1, false, &[]),
            OWNER,
            overlay(false, Some("1872.5"))
        ),
        Admission::Admitted
    );
}

/// `fuzz_policy_quorum`'s third pinned script: two independent approvers bound, then a policy
/// with maker-checker off and a ceiling far above the order. The author is still refused and one
/// grant is still only counted.
#[test]
fn a_loosened_policy_while_pending_leaves_the_bound_quorum() {
    let strict = pending_with(2, true, &[]);
    for policy in [PolicyOverlay::NONE, overlay(false, Some("100000"))] {
        assert_eq!(
            judged(&strict, AUTHOR, policy),
            Admission::Refused(Refusal::NotIndependent),
            "{policy:?}"
        );
        assert_eq!(
            judged(&strict, OWNER, policy),
            Admission::Counted,
            "{policy:?}"
        );
    }
}

/// A decimal's text as an integer at nine places, parsed by hand.
fn nano(text: &str) -> i128 {
    let (int, frac) = text.split_once('.').unwrap_or((text, ""));
    let frac = format!("{frac:0<9}");
    int.parse::<i128>().unwrap() * 1_000_000_000 + frac.parse::<i128>().unwrap()
}

/// A non-negative integer at nine places as canonical decimal text.
fn text_of(n: i128) -> String {
    let frac = format!("{:09}", n % 1_000_000_000);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        format!("{}", n / 1_000_000_000)
    } else {
        format!("{}.{frac}", n / 1_000_000_000)
    }
}

#[derive(Debug, Clone)]
struct QuorumCase {
    qty: &'static str,
    limit: &'static str,
    bound_approvers: u8,
    bound_independent: bool,
    policy_independent: bool,
    ceiling: Option<String>,
    grants: BTreeSet<&'static str>,
    responder: &'static str,
}

/// Orders whose value has at most four places, so the ceiling can sit one nano-dollar either side
/// of it; a ceiling at, just around, or far from the order value, or none; and grant sets an
/// admission could have folded. An author's grant is only ever counted while independence was
/// not required, and the bound requirement never changes, so a request that binds independence
/// never holds one.
fn quorum_case() -> impl Strategy<Value = QuorumCase> {
    let who = || prop::sample::select(vec![OWNER, SECOND, AUTHOR]);
    let offset = prop_oneof![
        Just(-1i128),
        Just(0i128),
        Just(1i128),
        -5_000_000_000_000i128..5_000_000_000_000,
    ];
    (
        prop::sample::select(vec!["10", "0.25", "3", "12", "1"]),
        prop::sample::select(vec!["187.25", "99.5", "43000", "0.01"]),
        (
            1u8..=2,
            prop::bool::weighted(0.4),
            prop::bool::weighted(0.5),
        ),
        prop::option::weighted(0.7, offset),
        prop::collection::btree_set(who(), 0..3),
        who(),
    )
        .prop_map(
            |(
                qty,
                limit,
                (bound_approvers, bound_independent, policy_independent),
                offset,
                mut grants,
                responder,
            )| {
                let order = nano(qty) * nano(limit) / 1_000_000_000;
                if bound_independent {
                    grants.remove(AUTHOR);
                }
                QuorumCase {
                    qty,
                    limit,
                    bound_approvers,
                    bound_independent,
                    policy_independent,
                    ceiling: offset.map(|o| text_of((order + o).max(0))),
                    grants,
                    responder,
                }
            },
        )
}

impl QuorumCase {
    fn build(&self) -> (Request, AdmissionContext) {
        let mut r = pending_with(self.bound_approvers, self.bound_independent, &[]);
        r.content.bound.qty = Qty::parse(self.qty).unwrap();
        r.content.bound.limit = Price::parse(self.limit).unwrap();
        r.grants = self.grants.iter().map(|g| user(g)).collect();
        let policy = overlay(self.policy_independent, self.ceiling.as_deref());
        (r, under(policy))
    }

    /// Check 7 from the spec's sentence alone: the order value at 18 places against the ceiling at
    /// 9, the larger count, either independence, and the grants that count after this one.
    fn expected(&self) -> (Quorum, Admission) {
        let order = nano(self.qty) * nano(self.limit);
        let over = self
            .ceiling
            .as_deref()
            .is_some_and(|c| order > nano(c) * 1_000_000_000);
        let need = self.bound_approvers.max(if over { 2 } else { 1 });
        let independent = self.bound_independent || self.policy_independent;
        let admission = if self.grants.contains(self.responder) {
            Admission::Refused(Refusal::DuplicateApprover)
        } else if independent && self.responder == AUTHOR {
            Admission::Refused(Refusal::NotIndependent)
        } else {
            let mut counting = self.grants.clone();
            counting.insert(self.responder);
            if independent {
                counting.remove(AUTHOR);
            }
            if counting.len() >= usize::from(need) {
                Admission::Admitted
            } else {
                Admission::Counted
            }
        };
        (quorum_of(need, independent), admission)
    }
}

/// MI-24, DEC-173 item 13: check 7 judges every grant against the stricter of the bound
/// requirement and the overlay, and `quorum` names that requirement.
#[test]
fn admission_judges_check_7_against_the_stricter_of_the_bound_and_the_overlay() {
    check(quorum_case(), |case| {
        let (r, c) = case.build();
        let (want_quorum, want) = case.expected();
        let got_quorum = quorum(&r.content.bound, &c.policy).map_err(fail("quorum"))?;
        prop_assert_eq!(got_quorum, want_quorum);
        let got = admit(Some(&r), &grant_by(case.responder), &c).map_err(fail("admit"))?;
        prop_assert_eq!(got, want);
        Ok(())
    });
}

/// Live, PX-7: a skip runs checks 1 to 5 only, so no overlay gives it a quorum to meet.
#[test]
fn a_skip_needs_no_quorum_whatever_the_overlay() {
    let strict = pending_with(2, true, &[]);
    for policy in [
        PolicyOverlay::NONE,
        overlay(true, None),
        overlay(false, Some("0")),
        overlay(true, Some("1872.49")),
    ] {
        for who in [OWNER, AUTHOR] {
            let s = Response {
                responder: user(who),
                ..skip()
            };
            assert_eq!(
                answer("admit", admit(Some(&strict), &s, &under(policy))),
                Admission::Admitted,
                "{who} under {policy:?}"
            );
        }
    }
}

/// Live, DEC-156 item 2: checks 1 to 6 decide before check 7 reads the overlay.
#[test]
fn checks_1_to_6_decide_before_the_overlay() {
    let maker_checker = under(overlay(true, Some("0")));
    let late = Response {
        submitted_at: RiskClock(DEADLINE),
        verdict: Verdict::Approve(Some(step_up("assertion-1", DEADLINE - 1))),
        ..grant_by(AUTHOR)
    };
    let bare = Response {
        verdict: Verdict::Approve(None),
        ..grant_by(AUTHOR)
    };
    let stranger = grant_by("user-stranger");
    for (response, why) in [
        (late, Refusal::Late),
        (bare, Refusal::StepUpMissing),
        (stranger, Refusal::NotAnApprover),
    ] {
        assert_eq!(
            answer("admit", admit(Some(&request()), &response, &maker_checker)),
            Admission::Refused(why)
        );
    }
}

/// Live: the fixture's order value is 1872.5, which the ceilings above straddle, and the oracle's
/// own arithmetic gives the hand values, so a pending failure is the crate's, not the oracle's.
#[test]
fn the_ceilings_straddle_the_fixture_order_and_the_oracle_computes_exactly() {
    let bound = request().content.bound;
    assert_eq!(
        bound.qty.notional(bound.limit).unwrap().to_string(),
        "1872.5"
    );
    assert_eq!(nano("187.25") * nano("10"), 1_872_500_000_000_000_000_000);
    assert_eq!(nano("1872.49"), 1_872_490_000_000);
    assert_eq!(text_of(1_872_500_000_001), "1872.500000001");
    assert_eq!(text_of(1_872_500_000_000), "1872.5");
    assert_eq!(text_of(0), "0");
    let at_the_ceiling = QuorumCase {
        qty: "10",
        limit: "187.25",
        bound_approvers: 1,
        bound_independent: false,
        policy_independent: false,
        ceiling: Some("1872.5".to_owned()),
        grants: BTreeSet::new(),
        responder: OWNER,
    };
    assert_eq!(
        at_the_ceiling.expected(),
        (quorum_of(1, false), Admission::Admitted)
    );
    let below = QuorumCase {
        ceiling: Some("1872.499999999".to_owned()),
        grants: [AUTHOR].into(),
        policy_independent: true,
        ..at_the_ceiling
    };
    assert_eq!(below.expected(), (quorum_of(2, true), Admission::Counted));
}
