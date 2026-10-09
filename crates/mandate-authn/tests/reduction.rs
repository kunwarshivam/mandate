//! The route-2 gate (identity spec §6.4 route 2, DEC-833, DEC-834). The verifier is a recording
//! stand-in; every expected challenge is rebuilt from the bytes and time the test handed in.

use std::num::NonZeroU32;

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use mandate_authn::{
    CHALLENGE_LIFETIME_S, Challenge, ClientKey, ReductionGate, ReductionLimits, Unauthenticated,
    UtcNanos,
};

const S: i64 = 1_000_000_000;
const LIFETIME: i64 = 300 * S;
const MEMBER: &str = "the verified credential row";
const REFUSED: Result<&str, Unauthenticated> = Err(Unauthenticated);

/// `ns` nanoseconds after a fixed instant.
fn at(ns: i64) -> UtcNanos {
    UtcNanos::from_parts(1_800_000_000 + ns / S, u32::try_from(ns % S).unwrap()).unwrap()
}

fn client(address: &str, device: &str) -> ClientKey {
    let (address, device) = (address.to_owned(), device.to_owned());
    ClientKey { address, device }
}

fn random(n: u8) -> [u8; 32] {
    [n; 32]
}

/// The challenge of bytes `random(n)` issued at `ns`, rebuilt from the spec's 300 s lifetime.
fn issued(n: u8, ns: i64) -> Challenge {
    let (bytes, expires_at) = (random(n), at(ns + LIFETIME));
    Challenge { bytes, expires_at }
}

/// A gate and a passkey verifier stand-in that records every challenge it is shown.
struct Bench {
    gate: ReductionGate,
    seen: Vec<Challenge>,
}

impl Bench {
    fn new(challenges: (u32, u32), failures: (u32, u32)) -> Self {
        let n = |v| NonZeroU32::new(v).unwrap();
        let gate = ReductionGate::new(ReductionLimits {
            challenges_per_address: n(challenges.0),
            challenges_per_device: n(challenges.1),
            failures_per_address: n(failures.0),
            failures_per_device: n(failures.1),
        });
        Self {
            gate,
            seen: Vec::new(),
        }
    }

    fn issue(&mut self, who: &ClientKey, n: u8, ns: i64) -> Result<Challenge, Unauthenticated> {
        self.gate.issue(who, random(n), at(ns))
    }

    /// Presents an assertion over `random(n)` that the verifier accepts when `accepts`.
    fn present(
        &mut self,
        who: &ClientKey,
        n: u8,
        ns: i64,
        accepts: bool,
    ) -> Result<&'static str, Unauthenticated> {
        self.gate.present(who, &random(n), at(ns), |challenge| {
            self.seen.push(*challenge);
            accepts.then_some(MEMBER)
        })
    }

    fn reached(&self, who: &ClientKey, ns: i64) -> bool {
        self.gate.failure_limit_reached(who, at(ns))
    }
}

#[test]
fn a_verified_assertion_is_never_refused_for_a_rate_limit() {
    let mut bench = Bench::new((1, 1), (1, 1));
    let member = client("198.51.100.1", "laptop");
    let flood = client("198.51.100.1", "laptop");
    bench.issue(&member, 1, 0).unwrap();
    assert_eq!(
        bench.issue(&flood, 2, S),
        Err(Unauthenticated),
        "the one slot is held"
    );
    for n in 3..13 {
        assert_eq!(
            bench.present(&flood, n, 2 * S, true),
            REFUSED,
            "unknown challenge {n}"
        );
    }
    assert!(
        bench.reached(&member, 2 * S),
        "ten failures on the member's address and device"
    );
    assert_eq!(bench.present(&member, 1, 3 * S, true), Ok(MEMBER));
    bench.issue(&member, 20, 4 * S).unwrap();
    assert_eq!(bench.present(&member, 20, 4 * S, false), REFUSED);
    bench.issue(&member, 21, 5 * S).unwrap();
    assert_eq!(bench.present(&member, 21, 5 * S, true), Ok(MEMBER));
    assert_eq!(
        bench.seen,
        [issued(1, 0), issued(20, 4 * S), issued(21, 5 * S)]
    );
}

#[test]
fn over_limit_requests_verify_only_against_an_outstanding_challenge_consumed_on_first_use() {
    let mut bench = Bench::new((4, 4), (1, 1));
    let who = client("198.51.100.2", "phone");
    assert_eq!(bench.present(&who, 9, 0, true), REFUSED, "never issued");
    assert!(bench.reached(&who, 0), "over the limit from here on");
    bench.issue(&who, 1, S).unwrap();
    assert_eq!(
        bench.present(&who, 1, 2 * S, false),
        REFUSED,
        "a failed assertion"
    );
    assert_eq!(
        bench.present(&who, 1, 3 * S, true),
        REFUSED,
        "consumed by the failure"
    );
    bench.issue(&who, 2, 4 * S).unwrap();
    assert_eq!(bench.present(&who, 2, 5 * S, true), Ok(MEMBER));
    assert_eq!(
        bench.present(&who, 2, 6 * S, true),
        REFUSED,
        "consumed by the success"
    );
    assert_eq!(
        bench.present(&who, 9, 7 * S, true),
        REFUSED,
        "still never issued"
    );
    assert_eq!(bench.seen, [issued(1, S), issued(2, 4 * S)]);
}

#[test]
fn reduction_session_failures_are_indistinguishable() {
    let mut bench = Bench::new((16, 16), (8, 8));
    let who = client("198.51.100.3", "tablet");
    let mut answers = Vec::new();
    bench.issue(&who, 1, 0).unwrap();
    answers.push((
        "an expired challenge",
        bench.present(&who, 1, LIFETIME, true),
    ));
    answers.push((
        "an unknown challenge",
        bench.present(&who, 2, LIFETIME, true),
    ));
    let verifier_refusals = [
        "an unknown credential ID",
        "a removed row",
        "a suspended row",
        "a bad signature",
        "a refused subject",
    ];
    for (n, case) in (3..).zip(verifier_refusals) {
        bench.issue(&who, n, LIFETIME).unwrap();
        answers.push((case, bench.present(&who, n, LIFETIME, false)));
    }
    assert!(
        !bench.reached(&who, LIFETIME),
        "seven failures, each counted once"
    );
    answers.push(("a used challenge", bench.present(&who, 3, LIFETIME, true)));
    assert!(bench.reached(&who, LIFETIME), "the eighth failure");
    bench.issue(&who, 10, LIFETIME).unwrap();
    answers.push(("over the limit", bench.present(&who, 10, LIFETIME, false)));
    assert_eq!(answers.len(), 9);
    for (case, answer) in answers {
        let refusal = answer.unwrap_err();
        assert_eq!(refusal, Unauthenticated, "{case}");
        assert_eq!(format!("{refusal:?}"), "Unauthenticated", "{case}");
        assert_eq!(refusal.to_string(), "unauthenticated", "{case}");
        assert_eq!(refusal.code(), "unauthenticated", "{case}");
    }
    assert_eq!(
        bench.seen.len(),
        6,
        "the five verifier refusals and the last"
    );
}

#[test]
fn reduction_session_challenge_reads_and_returns_nothing_workspace_specific() {
    let quiet = Bench::new((1, 1), (1, 1));
    let mut busy = Bench::new((5, 3), (7, 2));
    let other = client("192.0.2.9", "kiosk");
    busy.issue(&other, 7, 0).unwrap();
    assert_eq!(busy.present(&other, 8, S, true), REFUSED);
    let expected = Challenge {
        bytes: random(1),
        expires_at: at(340 * S + 123),
    };
    for (mut bench, who) in [(quiet, client("198.51.100.4", "laptop")), (busy, other)] {
        assert_eq!(bench.issue(&who, 1, 40 * S + 123), Ok(expected));
    }
}

#[test]
fn the_refusal_text_and_the_challenge_lifetime_are_fixed() {
    assert_eq!(
        CHALLENGE_LIFETIME_S * S,
        LIFETIME,
        "identity spec §6.4 route 2: 300 s"
    );
    assert_eq!(format!("{Unauthenticated:?}"), "Unauthenticated");
    assert_eq!(Unauthenticated.to_string(), "unauthenticated");
    assert_eq!(Unauthenticated.code(), "unauthenticated");
}

#[test]
fn a_challenge_is_outstanding_for_exactly_300_seconds() {
    let mut bench = Bench::new((1, 1), (9, 9));
    let who = client("198.51.100.5", "laptop");
    assert_eq!(bench.issue(&who, 1, 500), Ok(issued(1, 500)));
    assert_eq!(
        bench.issue(&who, 2, LIFETIME + 499),
        Err(Unauthenticated),
        "held 1 ns before"
    );
    assert_eq!(
        bench.present(&who, 1, LIFETIME + 499, true),
        Ok(MEMBER),
        "verifies 1 ns before"
    );
    assert_eq!(
        bench.issue(&who, 3, LIFETIME + 499),
        Ok(issued(3, LIFETIME + 499)),
        "consumed"
    );
    assert_eq!(
        bench.issue(&who, 4, 2 * LIFETIME + 498),
        Err(Unauthenticated)
    );
    assert_eq!(
        bench.issue(&who, 5, 2 * LIFETIME + 499),
        Ok(issued(5, 2 * LIFETIME + 499))
    );
    assert_eq!(
        bench.present(&who, 3, 2 * LIFETIME + 499, true),
        REFUSED,
        "expired at 300 s"
    );
    assert_eq!(bench.seen, [issued(1, 500)]);
}

#[test]
fn a_challenge_is_consumed_by_whichever_client_presents_it_and_never_replays() {
    let mut bench = Bench::new((1, 1), (1, 1));
    let (issuer, other) = (
        client("198.51.100.6", "laptop"),
        client("203.0.113.6", "phone"),
    );
    bench.issue(&issuer, 1, 0).unwrap();
    assert_eq!(
        bench.present(&other, 1, S, true),
        Ok(MEMBER),
        "a phone that changed networks"
    );
    assert_eq!(
        bench.present(&issuer, 1, 2 * S, true),
        REFUSED,
        "replayed by its issuer"
    );
    assert_eq!(
        bench.present(&other, 1, 2 * S, true),
        REFUSED,
        "replayed by its presenter"
    );
    assert!(
        bench.reached(&issuer, 2 * S),
        "the issuer's replay is its failure"
    );
    bench.issue(&issuer, 2, 3 * S).unwrap();
    assert_eq!(bench.present(&other, 2, 4 * S, false), REFUSED);
    assert_eq!(
        bench.issue(&issuer, 3, 5 * S),
        Ok(issued(3, 5 * S)),
        "freed by another's failure"
    );
    assert_eq!(bench.seen, [issued(1, 0), issued(2, 3 * S)]);
}

#[test]
fn the_challenge_limit_counts_outstanding_challenges_per_address_and_per_device() {
    let mut by_address = Bench::new((1, 9), (9, 9));
    let mut by_device = Bench::new((9, 1), (9, 9));
    let (a1d1, a1d2, a2d1) = (client("a1", "d1"), client("a1", "d2"), client("a2", "d1"));
    assert_eq!(by_address.issue(&a1d1, 1, 0), Ok(issued(1, 0)));
    assert_eq!(
        by_address.issue(&a1d2, 2, S),
        Err(Unauthenticated),
        "address a1 is full"
    );
    assert_eq!(by_address.issue(&a2d1, 3, S), Ok(issued(3, S)));
    assert_eq!(by_device.issue(&a1d1, 1, 0), Ok(issued(1, 0)));
    assert_eq!(
        by_device.issue(&a2d1, 2, S),
        Err(Unauthenticated),
        "device d1 is full"
    );
    assert_eq!(by_device.issue(&a1d2, 3, S), Ok(issued(3, S)));
    let mut roomy = Bench::new((9, 9), (9, 9));
    assert_eq!(roomy.issue(&a1d1, 1, 0), Ok(issued(1, 0)));
    assert_eq!(
        roomy.issue(&a2d1, 1, S),
        Err(Unauthenticated),
        "those bytes are outstanding"
    );
}

#[test]
fn the_failure_limit_counts_per_address_and_per_device_for_300_seconds() {
    let mut bench = Bench::new((9, 9), (3, 2));
    let keys = [
        client("a1", "d1"),
        client("a1", "d2"),
        client("a2", "d1"),
        client("a2", "d2"),
    ];
    let reached = |bench: &Bench, ns| keys.clone().map(|who| bench.reached(&who, ns));
    bench.issue(&keys[0], 1, 0).unwrap();
    assert_eq!(bench.present(&keys[0], 1, 0, true), Ok(MEMBER));
    assert_eq!(
        reached(&bench, 0),
        [false; 4],
        "a verified assertion counts nothing"
    );
    assert_eq!(bench.present(&keys[0], 9, 7, true), REFUSED);
    assert_eq!(reached(&bench, 7), [false; 4]);
    assert_eq!(bench.present(&keys[0], 9, 10 * S, true), REFUSED);
    assert_eq!(
        reached(&bench, 10 * S),
        [true, false, true, false],
        "two on device d1"
    );
    assert_eq!(bench.present(&keys[1], 9, 20 * S, true), REFUSED);
    assert_eq!(
        reached(&bench, 20 * S),
        [true, true, true, false],
        "three on address a1"
    );
    assert_eq!(
        reached(&bench, LIFETIME + 6),
        [true, true, true, false],
        "1 ns before"
    );
    assert_eq!(
        reached(&bench, LIFETIME + 7),
        [false; 4],
        "the first failure lapsed"
    );
}

#[derive(Debug, Clone)]
enum Step {
    Issue(usize),
    Present {
        who: usize,
        target: Option<usize>,
        accepts: bool,
    },
    Advance(i64),
}

fn step() -> impl Strategy<Value = Step> {
    let present = (0..3usize, proptest::option::of(0..12usize), any::<bool>());
    let advance = prop_oneof![
        Just(1),
        Just(LIFETIME - 1),
        Just(LIFETIME),
        0..LIFETIME + 100
    ];
    prop_oneof![
        (0..3usize).prop_map(Step::Issue),
        present.prop_map(|(who, target, accepts)| Step::Present {
            who,
            target,
            accepts
        }),
        advance.prop_map(Step::Advance),
    ]
}

/// A challenge in the model: its bytes, issuer, issue time, and whether it was presented.
struct Issued {
    n: u8,
    by: usize,
    ns: i64,
    used: bool,
}

/// The model is folded from the history alone and never calls the gate: the challenges issued,
/// and every refused presentation with its presenter and time.
#[test]
fn random_histories_verify_exactly_the_outstanding_challenges_within_the_limits() {
    let keys = [client("a0", "d0"), client("a0", "d1"), client("a1", "d1")];
    let shares = |a: usize, b: usize, address: bool| match address {
        true => keys[a].address == keys[b].address,
        false => keys[a].device == keys[b].device,
    };
    let limit = 1..4u32;
    let history = (
        limit.clone(),
        limit.clone(),
        limit.clone(),
        limit,
        vec(step(), 1..60),
    );
    let mut runner = TestRunner::new(ProptestConfig::with_cases(256));
    let outcome = runner.run(&history, |(ca, cd, fa, fd, steps)| {
        let mut bench = Bench::new((ca, cd), (fa, fd));
        let (mut now, mut challenges, mut failures) = (0, Vec::<Issued>::new(), Vec::new());
        for step in steps {
            match step {
                Step::Advance(ns) => now += ns,
                Step::Issue(who) => {
                    let n = u8::try_from(challenges.len()).unwrap();
                    let live = challenges
                        .iter()
                        .filter(|c| !c.used && now < c.ns + LIFETIME);
                    let held =
                        |address| live.clone().filter(|c| shares(c.by, who, address)).count();
                    let room = held(true) < ca as usize && held(false) < cd as usize;
                    let answer = bench.issue(&keys[who], n, now);
                    if answer.is_ok() {
                        prop_assert!(held(true) < ca as usize, "address over its limit");
                        prop_assert!(held(false) < cd as usize, "device over its limit");
                    }
                    prop_assert_eq!(
                        answer,
                        if room {
                            Ok(issued(n, now))
                        } else {
                            Err(Unauthenticated)
                        }
                    );
                    if room {
                        challenges.push(Issued {
                            n,
                            by: who,
                            ns: now,
                            used: false,
                        });
                    }
                }
                Step::Present {
                    who,
                    target,
                    accepts,
                } => {
                    let target = target.and_then(|i| challenges.get_mut(i));
                    let n = target.as_ref().map_or(200, |c| c.n);
                    let mut expected_seen = Vec::new();
                    if let Some(c) = target {
                        if !c.used && now < c.ns + LIFETIME {
                            expected_seen.push(issued(c.n, c.ns));
                        }
                        c.used = true;
                    }
                    let before = bench.seen.len();
                    let answer = bench.present(&keys[who], n, now, accepts);
                    prop_assert_eq!(
                        &bench.seen[before..],
                        &expected_seen[..],
                        "verified iff outstanding"
                    );
                    let verified = accepts && !expected_seen.is_empty();
                    prop_assert_eq!(answer, if verified { Ok(MEMBER) } else { REFUSED });
                    if !verified {
                        failures.push((who, now));
                    }
                }
            }
            for (k, key) in keys.iter().enumerate() {
                let recent = failures.iter().filter(|(_, at)| now < at + LIFETIME);
                let by = |address| {
                    recent
                        .clone()
                        .filter(|(by, _)| shares(*by, k, address))
                        .count()
                };
                let expected = by(true) >= fa as usize || by(false) >= fd as usize;
                prop_assert_eq!(bench.reached(key, now), expected, "key {}", k);
            }
        }
        Ok(())
    });
    outcome.unwrap();
}
