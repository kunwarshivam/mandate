//! A property over random session histories for the risk-reduction route 1 of identity spec §6.4
//! and the deprovision signal of §11.1 (backlog E9-1, slice A2, DEC-652 items 3, 5 and 9, DEC-658,
//! DEC-816 item 8): every admission, `admit`, and refresh matches an oracle that folds the
//! operations itself. A lapse ends the session as `expired` and every later call is refused the
//! same way, and a clock behind the last activity refuses only what adds no risk reduction. Waits
//! aim at the §6.2 deadlines to the second (the access token, the idle hour, the 12 h lifetime),
//! and a record that ended is replaced by a fresh one after one more refused step, so histories
//! keep testing live sessions. Times are whole seconds from [`T0`].

use mandate_authn::{
    ACCESS_TOKEN_LIFETIME_S, EndReason, OrgKind, ProviderAnswer, RefreshSecret, Refreshed, Request,
    SessionLimits, SessionPolicy, SessionRecord, SessionRefusal, UtcNanos,
};
use mandate_identity::{Session, SessionKind, SessionRef};
use mandate_identity_seal::Seal;
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

const T0: i64 = 1_800_000_000;
const HOUR: i64 = 3_600;
const REFERENCE: SessionRef = SessionRef(0x5e55);

fn at(offset_s: i64) -> UtcNanos {
    UtcNanos::from_parts(T0 + offset_s, 0).unwrap()
}

fn secret(n: u8) -> RefreshSecret {
    RefreshSecret([n; 32])
}

fn open(now: i64) -> SessionRecord {
    let limits = SessionLimits::resolve(OrgKind::Business, SessionPolicy::default()).unwrap();
    SessionRecord::open(limits, &secret(1), at(now)).unwrap()
}

/// A §6.2 deadline a wait can aim at, read from the oracle's own state.
#[derive(Debug, Clone, Copy)]
enum Deadline {
    Access,
    Idle,
    Absolute,
}

/// The seconds that pass before a step: a fixed number (a negative one is a clock that went
/// backwards), or whatever reaches a deadline plus an offset of a second either way.
#[derive(Debug, Clone, Copy)]
enum Wait {
    By(i64),
    To(Deadline, i64),
}

/// One step of a session's life, with the wait before it.
#[derive(Debug, Clone)]
enum Op {
    Authorize(Request, Wait),
    Admit(Request, Wait),
    /// Present the token issued `back` rotations ago (0 is the current one; past the first, an
    /// unknown token), answered with `answer`, rotating to the current secret when `reuse_next`.
    Refresh {
        back: usize,
        answer: ProviderAnswer,
        reuse_next: bool,
        wait: Wait,
    },
    End(EndReason, Wait),
}

fn op() -> impl Strategy<Value = Op> {
    let deadline = prop_oneof![
        2 => Just(Deadline::Access),
        2 => Just(Deadline::Idle),
        1 => Just(Deadline::Absolute)
    ];
    let wait = prop_oneof![
        3 => (0..60i64).prop_map(Wait::By),
        2 => (0..400i64).prop_map(Wait::By),
        1 => (0..5_000i64).prop_map(Wait::By),
        1 => (-400..0i64).prop_map(Wait::By),
        4 => (deadline, -1..=1i64).prop_map(|(d, off)| Wait::To(d, off)),
    ];
    let request = prop_oneof![
        Just(Request::Pause),
        Just(Request::KillSwitch),
        Just(Request::Other)
    ];
    let status = prop_oneof![
        prop::sample::select(vec![408u16, 429, 499, 500, 503, 599, 600]),
        395u16..605,
    ];
    let answer = prop_oneof![
        4 => Just(ProviderAnswer::Granted),
        1 => Just(ProviderAnswer::Unreachable),
        3 => status.prop_map(ProviderAnswer::Status),
        1 => Just(ProviderAnswer::Deprovision),
    ];
    let back = prop_oneof![5 => Just(0usize), 3 => Just(1usize), 1 => 2usize..4];
    let reason = prop_oneof![
        Just(EndReason::SignOut),
        Just(EndReason::Deactivated),
        Just(EndReason::Admin)
    ];
    prop_oneof![
        5 => (request.clone(), wait.clone()).prop_map(|(r, w)| Op::Authorize(r, w)),
        3 => (request, wait.clone()).prop_map(|(r, w)| Op::Admit(r, w)),
        5 => (back, answer, prop::bool::weighted(0.1), wait.clone()).prop_map(
            |(back, answer, reuse_next, wait)| Op::Refresh { back, answer, reuse_next, wait }
        ),
        1 => (reason, wait).prop_map(|(r, w)| Op::End(r, w)),
    ]
}

/// The oracle: what the spec's rules admit, folded from the operations so far with its own
/// bookkeeping (token indexes, not digests; times in seconds). Once `ended` is set it never
/// changes, and every later call is refused with it.
#[derive(Debug)]
struct Oracle {
    ended: Option<EndReason>,
    outage: bool,
    opened: i64,
    last_activity: i64,
    access_until: i64,
    issued: usize,
}

const IDLE: i64 = HOUR;
const ABSOLUTE: i64 = 12 * HOUR;

impl Oracle {
    fn opened(now: i64) -> Self {
        Self {
            ended: None,
            outage: false,
            opened: now,
            last_activity: now,
            access_until: now + ACCESS_TOKEN_LIFETIME_S,
            issued: 0,
        }
    }

    /// The wait that lands on `wait`'s target from `now`; a deadline already behind waits nothing.
    fn wait(&self, wait: Wait, now: i64) -> i64 {
        let deadline = |d| match d {
            Deadline::Access => self.access_until,
            Deadline::Idle => self.last_activity + IDLE,
            Deadline::Absolute => self.opened + ABSOLUTE,
        };
        match wait {
            Wait::By(s) => s,
            Wait::To(d, off) => (deadline(d) + off - now).max(0),
        }
    }

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
        if judged_at - self.opened >= ABSOLUTE {
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

    /// `authorize`, and on success the kind of session it builds: reduction-only in an outage.
    fn admit(&mut self, request: Request, now: i64) -> Result<SessionKind, SessionRefusal> {
        self.authorize(request, now)?;
        Ok(if self.outage {
            SessionKind::ReductionOnly
        } else {
            SessionKind::Full
        })
    }

    fn refresh(
        &mut self,
        back: usize,
        answer: ProviderAnswer,
        reuse_next: bool,
        now: i64,
    ) -> Result<Refreshed, SessionRefusal> {
        if let Some(reason) = self.ended {
            return Err(SessionRefusal::Ended { reason });
        }
        if now < self.last_activity {
            return Err(SessionRefusal::ClockBehind);
        }
        let idle_lapsed = now - self.last_activity >= IDLE;
        if now - self.opened >= ABSOLUTE || (!self.outage && idle_lapsed) {
            return Err(self.end(EndReason::Expired));
        }
        if back > self.issued {
            return Err(SessionRefusal::UnknownRefreshToken);
        }
        if back > 0 {
            return Err(self.end(EndReason::RefreshReuse));
        }
        match answer {
            ProviderAnswer::Deprovision => return Err(self.end(EndReason::Deprovisioned)),
            ProviderAnswer::Status(code) if !(500..600).contains(&code) => {
                return Err(self.end(EndReason::RefreshFailed));
            }
            ProviderAnswer::Granted | ProviderAnswer::Unreachable | ProviderAnswer::Status(_) => {}
        }
        if reuse_next {
            return Err(SessionRefusal::RefreshSecretReused);
        }
        if answer != ProviderAnswer::Granted {
            self.outage = true;
            return Ok(Refreshed::Outage);
        }
        if idle_lapsed {
            return Err(self.end(EndReason::Expired));
        }
        self.issued += 1;
        self.outage = false;
        self.last_activity = now;
        self.access_until = now + ACCESS_TOKEN_LIFETIME_S;
        Ok(Refreshed::Rotated {
            access_expires_at: at(self.access_until),
        })
    }
}

fn follows_the_oracle(ops: Vec<Op>) -> Result<(), TestCaseError> {
    let mut now = 0;
    let mut s = open(now);
    let mut o = Oracle::opened(now);
    let mut steps_since_end = 0;
    for op in ops {
        match op {
            Op::Authorize(request, wait) => {
                now += o.wait(wait, now);
                let expected = o.authorize(request, now);
                let got = s.authorize(request, at(now));
                prop_assert_eq!(got, expected, "{:?} at {}", request, now);
            }
            Op::Admit(request, wait) => {
                now += o.wait(wait, now);
                let expected = o
                    .admit(request, now)
                    .map(|kind| Session::new(Seal::grant(), REFERENCE, kind, Vec::new()));
                let got = s.admit(request, at(now), REFERENCE, Vec::new());
                prop_assert_eq!(got, expected, "admit {:?} at {}", request, now);
            }
            Op::Refresh {
                back,
                answer,
                reuse_next,
                wait,
            } => {
                now += o.wait(wait, now);
                let current = u8::try_from(o.issued + 1).unwrap();
                let presented = if back > o.issued {
                    200
                } else {
                    current - u8::try_from(back).unwrap()
                };
                let next = if reuse_next { current } else { current + 1 };
                let expected = o.refresh(back, answer, reuse_next, now);
                let got = s.refresh(&secret(presented), answer, &secret(next), at(now));
                let what = format!("refresh {answer:?} back {back} reuse {reuse_next} at {now}");
                prop_assert_eq!(got, expected, "{}", what);
            }
            Op::End(reason, wait) => {
                now += o.wait(wait, now);
                let expected = o.ended.is_none().then_some(reason);
                o.ended = o.ended.or(Some(reason));
                prop_assert_eq!(s.end(reason), expected);
            }
        }
        prop_assert_eq!(s.ended(), o.ended);
        if o.ended.is_some() {
            steps_since_end += 1;
        }
        if steps_since_end > 1 {
            s = open(now);
            o = Oracle::opened(now);
            steps_since_end = 0;
        }
    }
    Ok(())
}

#[test]
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
