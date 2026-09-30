//! Property tests for E8-3's invariants (and E8-2's lateness) in the
//! [M7 brief](../../../docs/project/tasks/M7-escalation-v0.md) that the pure core holds by itself,
//! each against an oracle that shares no code with the crate (AGENTS.md, "Independent oracles"):
//!
//! - **The check table** (EI-10, EI-11, EI-14, EI-15, EI-16): the brief's checks 1 to 7 and 8 to
//!   12, written out again here in order, from the brief's tables alone.
//! - **The clock accumulator** (EI-2, EI-15): its own maximum of every tick, dropped and
//!   out-of-order ticks included, and "timely" decided from that alone.
//! - **The principal generator** (EI-10): every `actor.kind` of journal §3, inside and outside
//!   `approvers`.
//! - **The assertion ledger** (EI-11): its own set of assertion ids already used.
//! - **Scaled-integer drift** (EI-5): `|m_now − m_req| × 10 000 ≤ band_bp × m_req` on `i128` at
//!   nine places, parsed from the prices' text, never through `mandate-num`.
//! - **The field comparer** (EI-4): the act's order against the text the generator drew.
//! - **The kill-switch probe** (EI-8, EI-11): every evidence shape, never refused.
//!
//! The runtime-level invariants (EI-1, EI-3, EI-6, EI-7, EI-12, and EI-8's same-step handoff) need
//! the fold and belong to the runtime's tests PR (DEC-165 item 1). Every property is pending until
//! the implementation PR and fails on `ApprovalError::Unimplemented` (DEC-77, DEC-110).

mod common;

use std::collections::BTreeSet;
use std::num::NonZeroU8;
use std::ops::Range;

use common::{AUTHOR, DEADLINE, OWNER, SECOND, T0, ctx, grant, hash, request, step_up, user};
use mandate_approval::{
    ActorKind, Admission, AssertionId, AssetClass, Classification, CommandAuthority, DryRun,
    Environment, KillSwitchAuthority, ModeNow, OwnerCommandKind, ReferenceMark, Refusal, Request,
    Response, Revalidation, RiskClock, SkipReason, StepUp, StepUpRefusal, Verdict, admit,
    kill_switch, owner_command, revalidate,
};
use mandate_num::{Price, Qty};
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

const WINDOW: i64 = 300;

/// An evidence age offset: authenticated exactly `WINDOW` seconds before the basis in one draw
/// of five, so the inclusive 300 s edge is drawn on every seed, and otherwise from `range`.
fn at_the_window_edge_or(range: Range<i64>) -> impl Strategy<Value = i64> {
    prop_oneof![1 => Just(-WINDOW), 4 => range]
}

/// The brief's step-up rule, written out: evidence counts if present, authenticated no more than
/// 300 s before `basis` and not after it, unused, and `CliConfirm` on a paper stream. The first
/// failure in the order of the brief's check-6 codes names the refusal.
fn oracle_step_up(
    evidence: Option<&StepUp>,
    basis: i64,
    live: bool,
    used: &BTreeSet<AssertionId>,
) -> Option<StepUpRefusal> {
    let e = match evidence {
        None => return Some(StepUpRefusal::Missing),
        Some(e) => e,
    };
    let age = basis - e.authenticated_at.0;
    if !(0..=WINDOW).contains(&age) {
        Some(StepUpRefusal::Stale)
    } else if used.contains(&e.assertion) {
        Some(StepUpRefusal::Reused)
    } else if live {
        Some(StepUpRefusal::Method)
    } else {
        None
    }
}

#[derive(Debug, Clone)]
struct AdmitCase {
    pending: bool,
    folded: i64,
    submitted: i64,
    actor: ActorKind,
    responder: &'static str,
    delivered: bool,
    same_hash: bool,
    approve: bool,
    evidence: Option<(i64, u8)>,
    live: bool,
    used: Vec<u8>,
    required: u8,
    grants: Vec<&'static str>,
    independent: bool,
}

fn admit_case() -> impl Strategy<Value = AdmitCase> {
    let who = prop::sample::select(vec![OWNER, SECOND, AUTHOR, "user-stranger"]);
    let actor = prop::sample::select(vec![
        ActorKind::User,
        ActorKind::User,
        ActorKind::User,
        ActorKind::System,
        ActorKind::Agent,
        ActorKind::Broker,
        ActorKind::PlatformOperator,
    ]);
    (
        (
            prop::bool::weighted(0.9),
            DEADLINE - 400..DEADLINE + 20,
            DEADLINE - 400..DEADLINE + 20,
        ),
        (
            actor,
            who,
            prop::bool::weighted(0.9),
            prop::bool::weighted(0.9),
        ),
        (
            prop::bool::weighted(0.8),
            prop::option::weighted(0.9, (at_the_window_edge_or(-350..20), 0u8..3)),
            prop::bool::weighted(0.1),
            prop::collection::vec(0u8..6, 0..2),
        ),
        (
            1u8..=2,
            prop::collection::vec(prop::sample::select(vec![OWNER, SECOND]), 0..2),
            prop::bool::weighted(0.3),
        ),
    )
        .prop_map(
            |(
                (pending, folded, submitted),
                (actor, responder, delivered, same_hash),
                (approve, evidence, live, used),
                (required, grants, independent),
            )| AdmitCase {
                pending,
                folded,
                submitted,
                actor,
                responder,
                delivered,
                same_hash,
                approve,
                evidence,
                live,
                used,
                required,
                grants,
                independent,
            },
        )
}

fn assertion(n: u8) -> AssertionId {
    AssertionId(format!("assertion-{n}"))
}

impl AdmitCase {
    fn effective(&self) -> i64 {
        if self.submitted > self.folded {
            self.submitted
        } else {
            self.folded
        }
    }

    fn evidence(&self) -> Option<StepUp> {
        self.evidence
            .map(|(age, n)| step_up(&format!("assertion-{n}"), self.effective() + age))
    }

    fn build(&self) -> (Request, Response, mandate_approval::AdmissionContext) {
        let mut r = request();
        r.delivered = self.delivered;
        r.content.bound.approvers_required = NonZeroU8::new(self.required).unwrap();
        r.content.bound.independent_required = self.independent;
        r.grants = self.grants.iter().map(|g| user(g)).collect();
        let mut c = ctx();
        c.folded_clock = RiskClock(self.folded);
        c.environment = if self.live {
            Environment::Live
        } else {
            Environment::Paper
        };
        c.used_assertions = self.used.iter().map(|n| assertion(*n)).collect();
        c.author = user(AUTHOR);
        c.approvers.insert(user(AUTHOR));
        let resp = Response {
            actor_kind: self.actor,
            responder: user(self.responder),
            verdict: if self.approve {
                Verdict::Approve(self.evidence())
            } else {
                Verdict::Skip
            },
            content_hash: if self.same_hash {
                hash("request")
            } else {
                hash("other")
            },
            submitted_at: RiskClock(self.submitted),
            ..grant()
        };
        (r, resp, c)
    }

    /// Checks 1 to 7 of the brief, in its order.
    fn expected(&self) -> Admission {
        let listed = [OWNER, SECOND, AUTHOR].contains(&self.responder);
        let refusal = if !self.pending {
            Some(Refusal::NotPending)
        } else if self.effective() >= DEADLINE {
            Some(Refusal::Late)
        } else if self.actor != ActorKind::User || !listed {
            Some(Refusal::NotAnApprover)
        } else if !self.delivered {
            Some(Refusal::NotDelivered)
        } else if !self.same_hash {
            Some(Refusal::ContentMismatch)
        } else {
            None
        };
        if let Some(r) = refusal {
            return Admission::Refused(r);
        }
        if !self.approve {
            return Admission::Admitted;
        }
        let used: BTreeSet<AssertionId> = self.used.iter().map(|n| assertion(*n)).collect();
        if let Some(r) =
            oracle_step_up(self.evidence().as_ref(), self.effective(), self.live, &used)
        {
            return Admission::Refused(match r {
                StepUpRefusal::Missing => Refusal::StepUpMissing,
                StepUpRefusal::Stale => Refusal::StepUpStale,
                StepUpRefusal::Reused => Refusal::StepUpReused,
                StepUpRefusal::Method => Refusal::StepUpMethod,
            });
        }
        if self.grants.contains(&self.responder) {
            return Admission::Refused(Refusal::DuplicateApprover);
        }
        if self.independent && self.responder == AUTHOR {
            return Admission::Refused(Refusal::NotIndependent);
        }
        let mut distinct: BTreeSet<&str> = self.grants.iter().copied().collect();
        distinct.insert(self.responder);
        if distinct.len() >= usize::from(self.required) {
            Admission::Admitted
        } else {
            Admission::Counted
        }
    }
}

/// EI-10, EI-11, EI-14, EI-15, EI-16, DEC-156 item 2: admission is the brief's check table.
#[test]
fn admission_matches_the_check_table() {
    check(admit_case(), |case| {
        let (r, resp, c) = case.build();
        let got = admit(case.pending.then_some(&r), &resp, &c).map_err(fail("admit"))?;
        prop_assert_eq!(got, case.expected());
        Ok(())
    });
}

/// EI-2, EI-15: across dropped, duplicated, and out-of-order ticks, nothing at or after the
/// deadline by the accumulator's own clock is admitted, and a valid response before it is.
#[test]
fn silence_and_lateness_never_admit() {
    let steps = prop::collection::vec((prop::bool::ANY, -60i64..120), 1..20);
    check(steps, |steps| {
        let mut clock = T0;
        for (is_tick, offset) in steps {
            if is_tick {
                clock = clock.max(clock + offset);
                continue;
            }
            let submitted = clock - 30 + offset;
            let effective = submitted.max(clock);
            let mut c = ctx();
            c.folded_clock = RiskClock(clock);
            let resp = Response {
                submitted_at: RiskClock(submitted),
                verdict: Verdict::Approve(Some(step_up("assertion-1", effective))),
                ..grant()
            };
            let got = admit(Some(&request()), &resp, &c).map_err(fail("admit"))?;
            if effective >= DEADLINE {
                prop_assert_eq!(got, Admission::Refused(Refusal::Late));
            } else {
                prop_assert_eq!(got, Admission::Admitted);
            }
        }
        Ok(())
    });
}

/// EI-10: across every actor kind and responder, only a listed user is admitted.
#[test]
fn only_a_listed_user_is_admitted() {
    let kinds = prop::sample::select(vec![
        ActorKind::System,
        ActorKind::Agent,
        ActorKind::User,
        ActorKind::Broker,
        ActorKind::PlatformOperator,
    ]);
    let who = prop::sample::select(vec![OWNER, SECOND, "user-stranger", "agent-1", "operator"]);
    check((kinds, who, prop::bool::ANY), |(kind, who, approve)| {
        let resp = Response {
            actor_kind: kind,
            responder: user(who),
            verdict: if approve {
                grant().verdict
            } else {
                Verdict::Skip
            },
            ..grant()
        };
        let got = admit(Some(&request()), &resp, &ctx()).map_err(fail("admit"))?;
        let human = kind == ActorKind::User && [OWNER, SECOND].contains(&who);
        prop_assert_eq!(got == Admission::Admitted, human);
        Ok(())
    });
}

/// EI-11: an assertion is admitted once per workspace, and only inside its window.
#[test]
fn an_assertion_is_admitted_once_and_only_inside_its_window() {
    let grants = prop::collection::vec((0u8..4, -320i64..10), 1..16);
    check(grants, |grants| {
        let mut seen: BTreeSet<AssertionId> = BTreeSet::new();
        for (n, age) in grants {
            let effective = T0 + 10;
            let mut c = ctx();
            c.used_assertions = seen.clone();
            let resp = Response {
                verdict: Verdict::Approve(Some(step_up(
                    &format!("assertion-{n}"),
                    effective + age,
                ))),
                ..grant()
            };
            let got = admit(Some(&request()), &resp, &c).map_err(fail("admit"))?;
            let expected = if !(-WINDOW..=0).contains(&age) {
                Admission::Refused(Refusal::StepUpStale)
            } else if seen.contains(&assertion(n)) {
                Admission::Refused(Refusal::StepUpReused)
            } else {
                seen.insert(assertion(n));
                Admission::Admitted
            };
            prop_assert_eq!(got, expected);
        }
        Ok(())
    });
}

/// A decimal's text as an integer at nine places, parsed by hand.
fn scaled(text: &str) -> i128 {
    let (int, frac) = text.split_once('.').unwrap_or((text, ""));
    let frac = format!("{frac:0<9}");
    int.parse::<i128>().unwrap() * 1_000_000_000 + frac.parse::<i128>().unwrap()
}

/// An integer at nine places as canonical decimal text.
fn unscaled(n: i128) -> String {
    let frac = format!("{:09}", n % 1_000_000_000);
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        format!("{}", n / 1_000_000_000)
    } else {
        format!("{}.{frac}", n / 1_000_000_000)
    }
}

#[derive(Debug, Clone)]
struct RevalidateCase {
    crypto: bool,
    m_req: Option<i128>,
    m_now: Option<i128>,
    qty: String,
    same_version: bool,
    mode: ModeNow,
    restricted: bool,
    in_universe: bool,
    classification: Classification,
    dry_run: DryRun,
}

fn revalidate_case() -> impl Strategy<Value = RevalidateCase> {
    let m_req = 1i128..2_000_000;
    let nudge = prop::sample::select(vec![-1i128, 0, 1]);
    let modes = prop::sample::select(vec![
        ModeNow::Normal,
        ModeNow::Normal,
        ModeNow::ExitsOnly,
        ModeNow::Paused,
        ModeNow::Stopped,
    ]);
    let classification = prop::sample::select(vec![
        Classification::Auto,
        Classification::Ask {
            decided_by: "rule:large_order".to_owned(),
        },
        Classification::Ask {
            decided_by: "rule:large_order".to_owned(),
        },
        Classification::Ask {
            decided_by: "default".to_owned(),
        },
        Classification::Deny,
    ]);
    let dry = prop::sample::select(vec![
        DryRun::Allow,
        DryRun::Allow,
        DryRun::Deny {
            reason: "gross_limit".to_owned(),
        },
    ]);
    (
        (
            prop::bool::ANY,
            m_req,
            prop::bool::ANY,
            nudge,
            -3i128..=3,
            prop::bool::weighted(0.05),
            prop::bool::weighted(0.05),
            1u32..100_000,
        ),
        (
            prop::bool::weighted(0.9),
            modes,
            prop::bool::weighted(0.1),
            prop::bool::weighted(0.9),
            classification,
            dry,
        ),
    )
        .prop_map(
            |(
                (crypto, req, up, nudge, spread, no_req, no_now, qty),
                (same_version, mode, restricted, in_universe, classification, dry_run),
            )| {
                let m_req = req * 10_000 * 1_000;
                let band: i128 = if crypto { 200 } else { 100 };
                let edge = band * m_req / 10_000;
                let delta = (edge * (4 + spread) / 4 + nudge).max(0);
                let m_now = if up {
                    m_req + delta
                } else {
                    (m_req - delta).max(1)
                };
                RevalidateCase {
                    crypto,
                    m_req: (!no_req).then_some(m_req),
                    m_now: (!no_now).then_some(m_now),
                    qty: qty.to_string(),
                    same_version,
                    mode,
                    restricted,
                    in_universe,
                    classification,
                    dry_run,
                }
            },
        )
}

impl RevalidateCase {
    fn expected(&self) -> Option<SkipReason> {
        let band: i128 = if self.crypto { 200 } else { 100 };
        let inside = match (self.m_req, self.m_now) {
            (Some(r), Some(n)) => (n - r).abs() * 10_000 <= band * r,
            _ => false,
        };
        if !self.same_version {
            Some(SkipReason::VersionChanged)
        } else if self.mode != ModeNow::Normal {
            Some(SkipReason::Mode)
        } else if self.restricted || !self.in_universe {
            Some(SkipReason::InstrumentRestricted)
        } else if self.classification == Classification::Deny {
            Some(SkipReason::ReclassifiedDeny)
        } else if matches!(&self.classification, Classification::Ask { decided_by } if decided_by != "rule:large_order")
        {
            Some(SkipReason::ReclassifiedOtherTrigger)
        } else if let DryRun::Deny { reason } = &self.dry_run {
            Some(SkipReason::Gate {
                reason: reason.clone(),
            })
        } else if !inside {
            Some(SkipReason::Drift)
        } else {
            None
        }
    }
}

/// EI-5 and EI-4: re-validation acts only when every check passes, with drift judged on integers,
/// and the act is the bound order field for field.
#[test]
fn revalidation_acts_only_when_every_check_passes_and_never_widens() {
    check(revalidate_case(), |case| {
        let mut r = request();
        r.content.bound.asset_class = if case.crypto {
            AssetClass::Crypto
        } else {
            AssetClass::UsEquity
        };
        r.content.bound.qty = Qty::parse(&case.qty).unwrap();
        r.content.bound.reference_mark = case.m_req.map(|m| ReferenceMark {
            price: Price::parse(&unscaled(m)).unwrap(),
            seq: 7,
        });
        let now = mandate_approval::Current {
            mandate_version: if case.same_version { "v3" } else { "v4" }.to_owned(),
            mode: case.mode,
            instrument_restricted: case.restricted,
            in_working_universe: case.in_universe,
            classification: case.classification.clone(),
            dry_run: case.dry_run.clone(),
            mark_now: case.m_now.map(|m| Price::parse(&unscaled(m)).unwrap()),
        };
        match (
            revalidate(&r, &now).map_err(fail("revalidate"))?,
            case.expected(),
        ) {
            (Revalidation::Skip(got), Some(want)) => prop_assert_eq!(got, want),
            (Revalidation::Act(order), None) => {
                let o = order.order();
                prop_assert_eq!(o.instrument.as_str(), "asset-equity-1");
                prop_assert_eq!(o.qty.to_string(), case.qty.clone());
                prop_assert_eq!(scaled(&o.limit.to_string()), scaled("187.25"));
                prop_assert_eq!(o.mandate_version.as_str(), "v3");
                prop_assert_eq!(o.purpose, mandate_approval::AskablePurpose::Open);
            }
            (got, want) => prop_assert!(false, "got {got:?}, the table says {want:?}"),
        }
        Ok(())
    });
}

fn evidence_shape() -> impl Strategy<Value = (Option<StepUp>, bool, Vec<u8>)> {
    let evidence = prop::option::weighted(0.8, (at_the_window_edge_or(-400..60), 0u8..3))
        .prop_map(|e| e.map(|(age, n)| step_up(&format!("assertion-{n}"), T0 + age)));
    (
        evidence,
        prop::bool::weighted(0.2),
        prop::collection::vec(0u8..3, 0..2),
    )
}

/// EI-8, EI-11, PB-14b, DEC-158 option (c): whatever its evidence, a kill switch stops and
/// flattens; only valid evidence at commit adds the owner-exit privileges.
#[test]
fn a_kill_switch_is_never_refused_whatever_its_evidence() {
    check(evidence_shape(), |(evidence, live, used)| {
        let used: BTreeSet<AssertionId> = used.into_iter().map(assertion).collect();
        let env = if live {
            Environment::Live
        } else {
            Environment::Paper
        };
        let got = kill_switch(evidence.as_ref(), RiskClock(T0), env, &used)
            .map_err(fail("kill_switch"))?;
        let expected = match oracle_step_up(evidence.as_ref(), T0, live, &used) {
            None => KillSwitchAuthority::OwnerExitPrivileges,
            Some(why) => KillSwitchAuthority::AutomatedFlatten(why),
        };
        prop_assert_eq!(got, expected);
        Ok(())
    });
}

/// PX-4, DEC-156 item 8, EI-11: pause always applies; resume, Stop, and acknowledge are judged
/// when processed; an owner exit when committed.
#[test]
fn owner_commands_match_the_freshness_table() {
    let kinds = prop::sample::select(vec![
        OwnerCommandKind::Pause,
        OwnerCommandKind::Resume,
        OwnerCommandKind::Stop,
        OwnerCommandKind::Acknowledge,
        OwnerCommandKind::OwnerExit,
    ]);
    check(
        (kinds, evidence_shape(), 0i64..600),
        |(kind, (evidence, live, used), lag)| {
            let used: BTreeSet<AssertionId> = used.into_iter().map(assertion).collect();
            let env = if live {
                Environment::Live
            } else {
                Environment::Paper
            };
            let got = owner_command(
                kind,
                evidence.as_ref(),
                RiskClock(T0),
                RiskClock(T0 + lag),
                env,
                &used,
            )
            .map_err(fail("owner_command"))?;
            let basis = if kind == OwnerCommandKind::OwnerExit {
                T0
            } else {
                T0 + lag
            };
            let expected = match kind {
                OwnerCommandKind::Pause => CommandAuthority::Apply,
                _ => oracle_step_up(evidence.as_ref(), basis, live, &used)
                    .map_or(CommandAuthority::Apply, CommandAuthority::Refused),
            };
            prop_assert_eq!(got, expected);
            Ok(())
        },
    );
}

/// Live: the drift oracle's own arithmetic, shown against hand-computed values, so a property
/// failure is the crate's, not the oracle's.
#[test]
fn the_scaled_integer_oracle_parses_and_prints_exactly() {
    assert_eq!(scaled("187.25"), 187_250_000_000);
    assert_eq!(unscaled(101_000_000_001), "101.000000001");
    assert_eq!(unscaled(100_000_000_000), "100");
}
