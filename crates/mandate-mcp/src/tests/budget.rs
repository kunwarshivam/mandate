//! The rate budget: reads never take the reserved exit tokens, checked against an oracle that
//! steps one refill period at a time instead of dividing; and a read is told exactly when the
//! ordinary bucket's next token comes, without that question changing any admission (DEC-959
//! item 2).

use std::time::Duration;

use proptest::prelude::*;

use crate::{BucketConfig, BudgetConfig, CallClass, McpError, RateBudget};

/// One bucket, advanced period by period from its anchor.
struct Oracle {
    capacity: u32,
    period: Duration,
    tokens: u32,
    anchor: Duration,
}

impl Oracle {
    fn new(config: BucketConfig) -> Self {
        Self {
            capacity: config.capacity,
            period: config.refill_every,
            tokens: config.capacity,
            anchor: Duration::ZERO,
        }
    }

    fn take(&mut self, now: Duration) -> bool {
        while self.tokens < self.capacity && self.anchor + self.period <= now {
            self.tokens += 1;
            self.anchor += self.period;
        }
        if self.tokens == self.capacity {
            self.anchor = self.anchor.max(now);
        }
        let admitted = self.tokens > 0;
        self.tokens -= u32::from(admitted);
        admitted
    }
}

fn bucket(capacity: u32, millis: u64) -> BucketConfig {
    BucketConfig {
        capacity,
        refill_every: Duration::from_millis(millis),
    }
}

#[test]
fn exhausted_reads_leave_the_reserve_and_refill_brings_them_back() {
    let config = BudgetConfig {
        ordinary: bucket(2, 100),
        reserved: bucket(1, 100),
    };
    let mut budget = RateBudget::new(config, Duration::ZERO).unwrap();
    let at = Duration::from_millis;
    let mut take = |class, now| {
        budget
            .try_acquire(class, now)
            .map_or_else(|e| e.code(), |()| "ok")
    };
    let read = CallClass::Ordinary;
    let exit = CallClass::RiskReducing;
    let got = [
        take(read, at(0)),
        take(read, at(10)),
        take(read, at(20)),
        take(exit, at(30)),
        take(exit, at(40)),
        take(read, at(99)),
        take(read, at(100)),
        take(exit, at(250)),
        take(exit, at(251)),
        take(exit, at(300)),
        take(exit, at(301)),
    ];
    let expected = [
        "ok",
        "ok",
        "throttled",
        "ok",
        "throttled",
        "throttled",
        "ok",
        "ok",
        "ok",
        "ok",
        "throttled",
    ];
    assert_eq!(got, expected);
}

#[test]
fn a_zero_capacity_or_refill_period_is_refused() {
    let good = bucket(1, 1);
    for (ordinary, reserved) in [(bucket(0, 1), good), (good, bucket(1, 0))] {
        let refused = RateBudget::new(BudgetConfig { ordinary, reserved }, Duration::ZERO);
        assert!(matches!(refused, Err(McpError::BadConfig)), "{refused:?}");
    }
}

/// The ordinary bucket's next token: none needed while it holds one, then the rest of the
/// current period, then whole periods again as it refills; the reserved bucket never counts.
#[test]
fn a_read_is_told_when_the_ordinary_buckets_next_token_comes() {
    let config = BudgetConfig {
        ordinary: bucket(1, 100),
        reserved: bucket(1, 100),
    };
    let mut budget = RateBudget::new(config, Duration::ZERO).unwrap();
    let at = Duration::from_millis;
    assert_eq!(budget.read_token_in(at(0)), Duration::ZERO);
    budget.try_acquire(CallClass::Read, at(30)).unwrap();
    assert_eq!(budget.read_token_in(at(30)), at(100));
    assert_eq!(budget.read_token_in(at(99)), at(31));
    assert_eq!(budget.read_token_in(at(130)), Duration::ZERO);
    budget.try_acquire(CallClass::Read, at(130)).unwrap();
    assert_eq!(budget.read_token_in(at(130)), at(100));
    assert_eq!(
        budget.read_token_in(at(40)),
        at(190),
        "a clock gone back counts the way back"
    );
    assert!(budget.try_acquire(CallClass::Read, at(229)).is_err());
    assert!(budget.try_acquire(CallClass::RiskReducing, at(229)).is_ok());
    assert_eq!(budget.read_token_in(at(229)), at(1));
}

fn events() -> impl Strategy<Value = Vec<(bool, u64)>> {
    prop::collection::vec((any::<bool>(), 0u64..40), 0..80)
}

/// A call's class: 0 an opening, 1 a read, 2 an exit.
fn classed() -> impl Strategy<Value = Vec<(u8, u64)>> {
    prop::collection::vec((0u8..3, 0u64..40), 0..80)
}

fn class_of(class: u8) -> CallClass {
    match class {
        0 => CallClass::Ordinary,
        1 => CallClass::Read,
        _ => CallClass::RiskReducing,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn admissions_match_the_oracle_and_reads_never_take_an_exit_token(
        ordinary in (1u32..4, 1u64..30),
        reserved in (1u32..4, 1u64..30),
        events in events(),
    ) {
        let config = BudgetConfig { ordinary: bucket(ordinary.0, ordinary.1), reserved: bucket(reserved.0, reserved.1) };
        let mut budget = RateBudget::new(config, Duration::ZERO).unwrap();
        let (mut general, mut reserve, mut exits_only) =
            (Oracle::new(config.ordinary), Oracle::new(config.reserved), Oracle::new(config.reserved));
        let mut now = Duration::ZERO;
        for (is_exit, gap) in events {
            now += Duration::from_millis(gap);
            let class = if is_exit { CallClass::RiskReducing } else { CallClass::Ordinary };
            let got = budget.try_acquire(class, now);
            let expected = if is_exit { reserve.take(now) || general.take(now) } else { general.take(now) };
            prop_assert_eq!(got.is_ok(), expected, "{:?} at {:?}", class, now);
            if is_exit && exits_only.take(now) {
                prop_assert!(got.is_ok(), "reads took an exit token at {:?}", now);
            }
        }
    }

    /// Over any sequence of openings, reads and exits and any budget state they leave: a read's
    /// answer is zero exactly when it would be admitted now; the token it names is there at that
    /// moment and, when it is not now, not one nanosecond before; asking changes no admission; and
    /// openings and reads draw on the same bucket as the oracle's, exits first on their own.
    #[test]
    fn a_reads_next_token_is_exact_and_asking_changes_nothing(
        ordinary in (1u32..4, 1u64..30),
        reserved in (1u32..4, 1u64..30),
        events in classed(),
    ) {
        let config = BudgetConfig { ordinary: bucket(ordinary.0, ordinary.1), reserved: bucket(reserved.0, reserved.1) };
        let mut asked = RateBudget::new(config, Duration::ZERO).unwrap();
        let mut unasked = RateBudget::new(config, Duration::ZERO).unwrap();
        let (mut general, mut reserve) = (Oracle::new(config.ordinary), Oracle::new(config.reserved));
        let mut now = Duration::ZERO;
        for (class, gap) in events {
            now += Duration::from_millis(gap);
            let class = class_of(class);
            let wait = asked.read_token_in(now);
            prop_assert_eq!(wait.is_zero(), asked.clone().try_acquire(CallClass::Read, now).is_ok(), "at {:?}", now);
            prop_assert!(wait <= Duration::from_millis(ordinary.1), "{:?} at {:?}", wait, now);
            prop_assert!(asked.clone().try_acquire(CallClass::Read, now + wait).is_ok(), "{:?} at {:?}", wait, now);
            if !wait.is_zero() {
                let early = now + wait - Duration::from_nanos(1);
                prop_assert!(asked.clone().try_acquire(CallClass::Read, early).is_err(), "{:?} at {:?}", wait, now);
            }
            let got = asked.try_acquire(class, now);
            prop_assert_eq!(got.is_ok(), unasked.try_acquire(class, now).is_ok(), "{:?} at {:?}", class, now);
            let expected = match class {
                CallClass::RiskReducing => reserve.take(now) || general.take(now),
                CallClass::Ordinary | CallClass::Read => general.take(now),
            };
            prop_assert_eq!(got.is_ok(), expected, "{:?} at {:?}", class, now);
        }
    }
}
