//! A property over random session histories for the risk-reduction route 1 of identity spec §6.4
//! and the deprovision signal of §11.1 (backlog E9-1, slice A2, DEC-652 items 3 and 5, DEC-816
//! item 8): every admission and refresh matches an oracle that folds the operations itself. A
//! lapse ends the session as `expired` and every later call is refused the same way, and a clock
//! behind the last activity refuses only what adds no risk reduction. Times are whole seconds from
//! [`T0`].

use mandate_authn::{
    ACCESS_TOKEN_LIFETIME_S, EndReason, OrgKind, ProviderAnswer, RefreshSecret, Refreshed, Request,
    SessionLimits, SessionPolicy, SessionRecord, SessionRefusal, UtcNanos,
};
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

const T0: i64 = 1_800_000_000;
const HOUR: i64 = 3_600;

fn at(offset_s: i64) -> UtcNanos {
    UtcNanos::from_parts(T0 + offset_s, 0).unwrap()
}

fn secret(n: u8) -> RefreshSecret {
    RefreshSecret([n; 32])
}

fn open() -> SessionRecord {
    let limits = SessionLimits::resolve(OrgKind::Business, SessionPolicy::default()).unwrap();
    SessionRecord::open(limits, &secret(1), at(0)).unwrap()
}

/// One step of a session's life, with the seconds that pass before it; a negative wait is a clock
/// that went backwards.
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
    let wait = prop_oneof![3 => 0..60i64, 3 => 0..400i64, 3 => 0..5_000i64, 1 => -400..0i64];
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
/// bookkeeping (token indexes, not digests; times in seconds). Once `ended` is set it never
/// changes, and every later call is refused with it.
#[derive(Debug, Default)]
struct Oracle {
    ended: Option<EndReason>,
    outage: bool,
    last_activity: i64,
    access_until: i64,
    issued: usize,
}

const IDLE: i64 = HOUR;
const ABSOLUTE: i64 = 12 * HOUR;

impl Oracle {
    fn end(&mut self, reason: EndReason) -> SessionRefusal {
        self.ended = Some(reason);
        SessionRefusal::Ended { reason }
    }

    fn authorize(&mut self, request: Request, now: i64) -> Result<(), SessionRefusal> {
        if let Some(reason) = self.ended {
            return Err(SessionRefusal::Ended { reason });
        }
        let reducing = request != Request::Other;
        let judged_at = if now >= self.last_activity {
            now
        } else if reducing {
            self.last_activity
        } else {
            return Err(SessionRefusal::ClockBehind);
        };
        if judged_at >= ABSOLUTE {
            return Err(self.end(EndReason::Expired));
        }
        if self.outage {
            if !reducing {
                return Err(SessionRefusal::RiskReductionOnly);
            }
            self.last_activity = judged_at;
            return Ok(());
        }
        if judged_at - self.last_activity >= IDLE {
            return Err(self.end(EndReason::Expired));
        }
        if judged_at >= self.access_until {
            return Err(SessionRefusal::AccessExpired);
        }
        self.last_activity = judged_at;
        Ok(())
    }

    fn refresh(
        &mut self,
        back: usize,
        answer: ProviderAnswer,
        now: i64,
    ) -> Result<Refreshed, SessionRefusal> {
        if let Some(reason) = self.ended {
            return Err(SessionRefusal::Ended { reason });
        }
        if now < self.last_activity {
            return Err(SessionRefusal::ClockBehind);
        }
        let idle_lapsed = now - self.last_activity >= IDLE;
        if now >= ABSOLUTE || (!self.outage && idle_lapsed) {
            return Err(self.end(EndReason::Expired));
        }
        if back > self.issued {
            return Err(SessionRefusal::UnknownRefreshToken);
        }
        if back > 0 {
            return Err(self.end(EndReason::RefreshReuse));
        }
        match answer {
            ProviderAnswer::Granted if idle_lapsed => Err(self.end(EndReason::Expired)),
            ProviderAnswer::Granted => {
                self.issued += 1;
                self.outage = false;
                self.last_activity = now;
                self.access_until = now + ACCESS_TOKEN_LIFETIME_S;
                Ok(Refreshed::Rotated {
                    access_expires_at: at(self.access_until),
                })
            }
            ProviderAnswer::Status(code) if !(500..600).contains(&code) => {
                Err(self.end(EndReason::RefreshFailed))
            }
            ProviderAnswer::Unreachable | ProviderAnswer::Status(_) => {
                self.outage = true;
                Ok(Refreshed::Outage)
            }
            ProviderAnswer::Deprovision => Err(self.end(EndReason::Deprovisioned)),
        }
    }
}

fn follows_the_oracle(ops: Vec<Op>) -> Result<(), TestCaseError> {
    let mut s = open();
    let mut o = Oracle {
        access_until: ACCESS_TOKEN_LIFETIME_S,
        ..Oracle::default()
    };
    let mut now = 0;
    for op in ops {
        match op {
            Op::Authorize(request, wait) => {
                now += wait;
                let expected = o.authorize(request, now);
                let got = s.authorize(request, at(now));
                prop_assert_eq!(got, expected, "{:?} at {}", request, now);
            }
            Op::Refresh { back, answer, wait } => {
                now += wait;
                let presented = if back > o.issued {
                    200
                } else {
                    u8::try_from(o.issued - back + 1).unwrap()
                };
                let next = u8::try_from(o.issued + 2).unwrap();
                let expected = o.refresh(back, answer, now);
                let got = s.refresh(&secret(presented), answer, &secret(next), at(now));
                let what = format!("refresh {answer:?} back {back} at {now}");
                prop_assert_eq!(got, expected, "{}", what);
            }
            Op::End(reason, wait) => {
                now += wait;
                let expected = o.ended.is_none().then_some(reason);
                o.ended = o.ended.or(Some(reason));
                prop_assert_eq!(s.end(reason), expected);
            }
        }
        prop_assert_eq!(s.ended(), o.ended);
    }
    Ok(())
}

#[test]
#[ignore = "pending E9-1"]
fn every_admission_follows_the_spec_rules_over_any_history() {
    let config = ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    let ops = prop::collection::vec(op(), 1..60);
    if let Err(failure) = TestRunner::new(config).run(&ops, follows_the_oracle) {
        panic!("{failure}");
    }
}
