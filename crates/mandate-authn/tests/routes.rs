//! The risk-reduction routes 1 and 2 of identity spec §6.4 and the deprovision signal of §11.1
//! (backlog E9-1, slice A2, DEC-652 item 3): an unreachable provider, a failed refresh, a
//! deprovision, the reduction-only session, and a property over random histories whose oracle
//! folds the operations itself. Times are whole seconds from [`T0`].

use mandate_authn::{
    ACCESS_TOKEN_LIFETIME_S, EndReason, OrgKind, ProviderAnswer, REDUCTION_ONLY_LIFETIME_S, Reach,
    RefreshSecret, Refreshed, Request, SessionLimits, SessionPolicy, SessionRecord, SessionRefusal,
    SubjectStanding, UtcNanos,
};
use proptest::prelude::*;

const T0: i64 = 1_800_000_000;
const HOUR: i64 = 3_600;
const DAY: i64 = 24 * HOUR;

fn at(offset_s: i64) -> UtcNanos {
    UtcNanos::from_parts(T0 + offset_s, 0).unwrap()
}

fn secret(n: u8) -> RefreshSecret {
    RefreshSecret([n; 32])
}

fn business() -> SessionLimits {
    SessionLimits::resolve(OrgKind::Business, SessionPolicy::default()).unwrap()
}

fn open() -> SessionRecord {
    SessionRecord::open(business(), &secret(1), at(0)).unwrap()
}

fn ended(reason: EndReason) -> SessionRefusal {
    SessionRefusal::Ended { reason }
}

#[test]
#[ignore = "pending E9-1"]
fn an_unreachable_provider_leaves_pause_and_the_kill_switch_until_the_absolute_lifetime() {
    for answer in [
        ProviderAnswer::Unreachable,
        ProviderAnswer::Status(500),
        ProviderAnswer::Status(503),
        ProviderAnswer::Status(599),
    ] {
        let mut s = open();
        let outcome = s.refresh(&secret(1), answer, &secret(2), at(300));
        assert_eq!(outcome, Ok(Refreshed::Outage), "{answer:?}");
        assert_eq!(s.reach(), Reach::Outage);
        assert!(!s.can_present_step_up());
        assert_eq!(
            s.authorize(Request::Other, at(301)),
            Err(SessionRefusal::RiskReductionOnly)
        );
        assert_eq!(
            s.authorize(Request::Pause, at(5 * HOUR)),
            Ok(()),
            "no idle timeout in an outage"
        );
        assert_eq!(s.authorize(Request::KillSwitch, at(12 * HOUR - 1)), Ok(()));
        assert_eq!(
            s.authorize(Request::KillSwitch, at(12 * HOUR)),
            Err(SessionRefusal::AbsoluteExpired)
        );
    }
    let mut s = open();
    assert_eq!(
        s.refresh(&secret(1), ProviderAnswer::Unreachable, &secret(2), at(300)),
        Ok(Refreshed::Outage)
    );
    let restored = s.refresh(&secret(1), ProviderAnswer::Granted, &secret(3), at(900));
    assert_eq!(
        restored,
        Ok(Refreshed::Rotated {
            access_expires_at: at(1200)
        }),
        "the token stayed current"
    );
    assert_eq!(
        (s.reach(), s.authorize(Request::Other, at(901))),
        (Reach::Full, Ok(()))
    );
    let reused = s.refresh(&secret(1), ProviderAnswer::Granted, &secret(4), at(902));
    assert_eq!(reused, Err(ended(EndReason::RefreshReuse)));
}

#[test]
#[ignore = "pending E9-1"]
fn any_other_answer_ends_the_session_with_every_permission() {
    let cases = [400, 401, 403, 404, 408, 429, 200, 204, 302, 499, 600]
        .map(|code| (ProviderAnswer::Status(code), EndReason::RefreshFailed))
        .into_iter()
        .chain([(ProviderAnswer::Deprovision, EndReason::Deprovisioned)]);
    for (answer, reason) in cases {
        for outage_first in [false, true] {
            let mut s = open();
            if outage_first {
                let outage =
                    s.refresh(&secret(1), ProviderAnswer::Unreachable, &secret(2), at(300));
                assert_eq!(outage, Ok(Refreshed::Outage));
            }
            let outcome = s.refresh(&secret(1), answer, &secret(2), at(400));
            assert_eq!(outcome, Err(ended(reason)), "{answer:?}");
            assert_eq!(s.ended(), Some(reason), "{answer:?}");
            for request in [Request::Pause, Request::KillSwitch, Request::Other] {
                assert_eq!(
                    s.authorize(request, at(401)),
                    Err(ended(reason)),
                    "{answer:?}"
                );
            }
        }
    }
    let mut s = open();
    let logout = s.end(EndReason::Deprovisioned);
    assert_eq!(
        logout,
        Some(EndReason::Deprovisioned),
        "a back-channel logout"
    );
    assert_eq!(s.end(EndReason::Deprovisioned), None);
    assert_eq!(
        s.authorize(Request::KillSwitch, at(1)),
        Err(ended(EndReason::Deprovisioned))
    );
}

#[test]
#[ignore = "pending E9-1"]
fn only_a_deprovision_signal_closes_the_local_passkey_route() {
    let signed_in = SubjectStanding {
        last_sign_in: Some(at(-DAY)),
        last_deprovision: None,
    };
    for (answer, opens) in [
        (ProviderAnswer::Status(429), true),
        (ProviderAnswer::Status(408), true),
        (ProviderAnswer::Status(400), true),
        (ProviderAnswer::Deprovision, false),
    ] {
        let mut s = open();
        assert!(
            s.refresh(&secret(1), answer, &secret(2), at(300)).is_err(),
            "{answer:?}"
        );
        let standing = signed_in.noting(s.ended(), at(300));
        let route_two = SessionRecord::open_reduction_only(standing, at(301));
        assert_eq!(route_two.is_ok(), opens, "{answer:?}");
        if !opens {
            assert_eq!(route_two, Err(SessionRefusal::DeprovisionSeen));
        }
    }
    for reason in [
        EndReason::SignOut,
        EndReason::Deactivated,
        EndReason::RefreshReuse,
        EndReason::Admin,
    ] {
        assert_eq!(
            signed_in.noting(Some(reason), at(5)),
            signed_in,
            "{reason:?}"
        );
    }
    assert_eq!(signed_in.noting(None, at(5)), signed_in);
    let logged_out = signed_in.noting(Some(EndReason::Deprovisioned), at(5));
    assert_eq!(logged_out.last_deprovision, Some(at(5)));
    assert_eq!(logged_out.last_sign_in, signed_in.last_sign_in);
}

#[test]
#[ignore = "pending E9-1"]
fn a_reduction_only_session_reaches_pause_and_the_kill_switch_for_fifteen_minutes() {
    assert_eq!(REDUCTION_ONLY_LIFETIME_S, 900);
    let standing = SubjectStanding {
        last_sign_in: Some(at(-DAY)),
        last_deprovision: Some(at(-2 * DAY)),
    };
    let mut s = SessionRecord::open_reduction_only(standing, at(0)).unwrap();
    assert_eq!(s.reach(), Reach::ReductionOnly);
    assert!(!s.can_present_step_up(), "§7.3: no step-up through it");
    assert_eq!(
        s.authorize(Request::Other, at(1)),
        Err(SessionRefusal::RiskReductionOnly)
    );
    assert_eq!(s.authorize(Request::Pause, at(899)), Ok(()));
    assert_eq!(s.authorize(Request::KillSwitch, at(899)), Ok(()));
    assert_eq!(
        s.authorize(Request::KillSwitch, at(900)),
        Err(SessionRefusal::AbsoluteExpired)
    );
    let refresh = s.refresh(&secret(1), ProviderAnswer::Granted, &secret(2), at(10));
    assert_eq!(refresh, Err(SessionRefusal::NotRefreshable));
    assert_eq!(s.reach(), Reach::ReductionOnly, "nothing widens it");
    let never_refused = SubjectStanding {
        last_sign_in: None,
        last_deprovision: None,
    };
    assert!(SessionRecord::open_reduction_only(never_refused, at(0)).is_ok());
    for (sign_in, refusal) in [
        (Some(-DAY), Some(-1)),
        (Some(-DAY), Some(-DAY)),
        (None, Some(-DAY)),
    ] {
        let standing = SubjectStanding {
            last_sign_in: sign_in.map(at),
            last_deprovision: refusal.map(at),
        };
        let refused = SessionRecord::open_reduction_only(standing, at(0));
        assert_eq!(
            refused,
            Err(SessionRefusal::DeprovisionSeen),
            "{sign_in:?} {refusal:?}"
        );
    }
}

/// One step of a session's life, with the seconds that pass before it.
#[derive(Debug, Clone)]
enum Op {
    Authorize(Request, i64),
    /// Present the token issued `back` rotations ago (0 is the current one; past the first, an
    /// unknown token), answered with `answer`.
    Refresh {
        back: usize,
        answer: ProviderAnswer,
        wait: i64,
    },
    End(EndReason, i64),
}

fn op() -> impl Strategy<Value = Op> {
    let wait = prop_oneof![0..60i64, 0..400i64, 0..5_000i64];
    let request = prop_oneof![
        Just(Request::Pause),
        Just(Request::KillSwitch),
        Just(Request::Other)
    ];
    let answer = prop_oneof![
        4 => Just(ProviderAnswer::Granted),
        1 => Just(ProviderAnswer::Unreachable),
        1 => (395u16..605).prop_map(ProviderAnswer::Status),
        1 => Just(ProviderAnswer::Deprovision),
    ];
    let reason = prop_oneof![
        Just(EndReason::SignOut),
        Just(EndReason::Deactivated),
        Just(EndReason::Admin)
    ];
    prop_oneof![
        6 => (request, wait.clone()).prop_map(|(r, w)| Op::Authorize(r, w)),
        4 => (prop_oneof![4 => Just(0usize), 1 => 1usize..4], answer, wait.clone())
            .prop_map(|(back, answer, wait)| Op::Refresh { back, answer, wait }),
        1 => (reason, wait).prop_map(|(r, w)| Op::End(r, w)),
    ]
}

/// The oracle: what the spec's rules admit, folded from the operations so far with its own
/// bookkeeping (token indexes, not digests; times in seconds).
#[derive(Debug, Default)]
struct Oracle {
    ended: Option<EndReason>,
    outage: bool,
    last_activity: i64,
    access_until: i64,
    issued: usize,
}

impl Oracle {
    fn authorize(
        &mut self,
        request: Request,
        now: i64,
        idle: i64,
        absolute: i64,
    ) -> Result<(), SessionRefusal> {
        if let Some(reason) = self.ended {
            return Err(SessionRefusal::Ended { reason });
        }
        if now >= absolute {
            return Err(SessionRefusal::AbsoluteExpired);
        }
        let reducing = request != Request::Other;
        if self.outage {
            return if reducing {
                Ok(())
            } else {
                Err(SessionRefusal::RiskReductionOnly)
            };
        }
        if now - self.last_activity >= idle {
            return Err(SessionRefusal::IdleExpired);
        }
        if now >= self.access_until {
            return Err(SessionRefusal::AccessExpired);
        }
        self.last_activity = now;
        Ok(())
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    #[ignore = "pending E9-1"]
    fn every_admission_follows_the_spec_rules_over_any_history(ops in prop::collection::vec(op(), 1..60)) {
        let (idle, absolute) = (HOUR, 12 * HOUR);
        let mut s = open();
        let mut o = Oracle { access_until: ACCESS_TOKEN_LIFETIME_S, ..Oracle::default() };
        let mut now = 0;
        for op in ops {
            match op {
                Op::Authorize(request, wait) => {
                    now += wait;
                    let expected = o.authorize(request, now, idle, absolute);
                    prop_assert_eq!(s.authorize(request, at(now)), expected, "{:?} at {}", request, now);
                }
                Op::Refresh { back, answer, wait } => {
                    now += wait;
                    let presented = if back > o.issued { 200 } else { (o.issued - back + 1) as u8 };
                    let next = (o.issued + 2) as u8;
                    let outcome = s.refresh(&secret(presented), answer, &secret(next), at(now));
                    let expected = if let Some(reason) = o.ended {
                        Err(SessionRefusal::Ended { reason })
                    } else if now >= absolute {
                        Err(SessionRefusal::AbsoluteExpired)
                    } else if !o.outage && now - o.last_activity >= idle {
                        Err(SessionRefusal::IdleExpired)
                    } else if back > o.issued {
                        Err(SessionRefusal::UnknownRefreshToken)
                    } else if back > 0 {
                        o.ended = Some(EndReason::RefreshReuse);
                        Err(SessionRefusal::Ended { reason: EndReason::RefreshReuse })
                    } else {
                        match answer {
                            ProviderAnswer::Granted => {
                                o.issued += 1;
                                o.outage = false;
                                o.last_activity = now;
                                o.access_until = now + ACCESS_TOKEN_LIFETIME_S;
                                Ok(Refreshed::Rotated { access_expires_at: at(o.access_until) })
                            }
                            ProviderAnswer::Unreachable => {
                                o.outage = true;
                                Ok(Refreshed::Outage)
                            }
                            ProviderAnswer::Status(code) if (500..600).contains(&code) => {
                                o.outage = true;
                                Ok(Refreshed::Outage)
                            }
                            ProviderAnswer::Status(_) => {
                                o.ended = Some(EndReason::RefreshFailed);
                                Err(SessionRefusal::Ended { reason: EndReason::RefreshFailed })
                            }
                            ProviderAnswer::Deprovision => {
                                o.ended = Some(EndReason::Deprovisioned);
                                Err(SessionRefusal::Ended { reason: EndReason::Deprovisioned })
                            }
                        }
                    };
                    prop_assert_eq!(outcome, expected, "refresh {:?} back {} at {}", answer, back, now);
                }
                Op::End(reason, wait) => {
                    now += wait;
                    let expected = if o.ended.is_some() { None } else { Some(reason) };
                    o.ended = o.ended.or(Some(reason));
                    prop_assert_eq!(s.end(reason), expected);
                }
            }
            prop_assert_eq!(s.ended(), o.ended);
        }
    }
}
