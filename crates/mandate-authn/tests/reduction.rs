//! The route-2 gate (identity spec §6.4 route 2, DEC-833, DEC-834). The verifier is a recording
//! stand-in; every expected challenge is rebuilt from the bytes and time the test handed in.

use std::num::NonZeroU32;

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
#[ignore = "pending E9-1"]
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
#[ignore = "pending E9-1"]
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
#[ignore = "pending E9-1"]
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
#[ignore = "pending E9-1"]
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
