//! The exchange waits for its code until the request's deadline (DEC-2363 item 3): the code may
//! arrive after the exchange started, a code that never arrives ends `code_missing` at the
//! deadline, and the code is taken once whoever races for it. The clock and the pauses are this
//! file's; the schedule a run must follow is computed here, apart from the implementation.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;
use secrecy::SecretString;

use super::support::{FixtureProvider, FixtureVault, SECRET, at, client, key};
use crate::ConnectError;
use crate::exchange::{
    ClientSecret, CodeWait, ExchangedGrant, MAX_PAUSE, PendingConnect, exchange_waiting_code,
};
use crate::grant::GrantedScopes;
use crate::start::{Environment, PkceVerifier};
use crate::vault::{AccessToken, AuthorizationCode, Vault, VaultKey, WaitingCode};

/// The instant every run starts at.
const START: i64 = 1_790_000_000;

/// A clock the runs share, in whole seconds, and the pauses it was asked for.
#[derive(Clone)]
struct Clock {
    now: Rc<Cell<i64>>,
    pauses: Rc<RefCell<Vec<Duration>>>,
}

impl Clock {
    fn new() -> Self {
        Self {
            now: Rc::new(Cell::new(START)),
            pauses: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn pauses(&self) -> Vec<u64> {
        self.pauses.borrow().iter().map(Duration::as_secs).collect()
    }
}

impl CodeWait for Clock {
    fn now(&mut self) -> UtcNanos {
        at(self.now.get())
    }

    fn pause(&mut self, pause: Duration) {
        assert!(
            !pause.is_zero(),
            "a pause of nothing would spin without end"
        );
        assert_eq!(
            pause.subsec_nanos(),
            0,
            "every deadline here is a whole second"
        );
        self.pauses.borrow_mut().push(pause);
        self.now
            .set(self.now.get() + i64::try_from(pause.as_secs()).unwrap());
    }
}

/// A vault the callback writes the code into once, at `arrives`, on the shared clock: every clone
/// shares the vault, the arrival and the count of takes, as exchanges racing for one code do.
#[derive(Clone)]
struct Arriving {
    vault: Rc<RefCell<FixtureVault>>,
    clock: Clock,
    arrives: Rc<Cell<Option<i64>>>,
    takes: Rc<Cell<u32>>,
}

impl Arriving {
    fn new(clock: &Clock, arrives: Option<i64>) -> Self {
        Self {
            vault: Rc::new(RefCell::new(FixtureVault::default())),
            clock: clock.clone(),
            arrives: Rc::new(Cell::new(arrives)),
            takes: Rc::new(Cell::new(0)),
        }
    }
}

impl Vault for Arriving {
    fn put_code(
        &mut self,
        key: &VaultKey,
        code: AuthorizationCode,
        verifier: PkceVerifier,
    ) -> Result<(), ConnectError> {
        self.vault.borrow_mut().put_code(key, code, verifier)
    }

    fn take_code(&mut self, key: &VaultKey) -> Result<Option<WaitingCode>, ConnectError> {
        if self
            .arrives
            .get()
            .is_some_and(|arrives| self.clock.now.get() >= arrives)
        {
            self.arrives.set(None);
            self.vault.borrow_mut().put_code(
                key,
                AuthorizationCode(SecretString::from(super::support::CODE)),
                PkceVerifier(SecretString::from(super::support::VERIFIER)),
            )?;
        }
        self.takes.set(self.takes.get() + 1);
        self.vault.borrow_mut().take_code(key)
    }

    fn put_token(
        &mut self,
        key: &VaultKey,
        token: AccessToken,
        scopes: &GrantedScopes,
    ) -> Result<(), ConnectError> {
        self.vault.borrow_mut().put_token(key, token, scopes)
    }

    fn delete_code(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        self.vault.borrow_mut().delete_code(key)
    }

    fn delete(&mut self, key: &VaultKey) -> Result<(), ConnectError> {
        self.vault.borrow_mut().delete(key)
    }
}

fn pending() -> PendingConnect {
    PendingConnect {
        connection_id: "conn-1".to_owned(),
        environment: Environment::Paper,
        vault_key: key(),
    }
}

fn run(
    vault: &mut Arriving,
    provider: &mut FixtureProvider,
    clock: &mut Clock,
    deadline: i64,
) -> Result<ExchangedGrant, ConnectError> {
    let secret = ClientSecret(SecretString::from(SECRET));
    exchange_waiting_code(
        &pending(),
        &client(),
        &secret,
        vault,
        provider,
        clock,
        at(deadline),
    )
}

/// The instants a run takes at, from `START` until `deadline`, as DEC-2363 item 3 schedules them:
/// a pause of 1 s, doubling up to [`MAX_PAUSE`], never past the deadline.
fn schedule(deadline: i64) -> Vec<i64> {
    let cap = i64::try_from(MAX_PAUSE.as_secs()).unwrap();
    let (mut now, mut pause, mut takes) = (START, 1, Vec::new());
    while now < deadline {
        takes.push(now);
        now += pause.min(deadline - now);
        pause = (pause * 2).min(cap);
    }
    takes
}

/// A code the callback writes 5 s after the exchange started is taken at the next take, 7 s in,
/// after pauses of 1, 2 and 4 s, and exchanged once.
#[test]
fn a_code_that_arrives_after_the_start_is_exchanged_once() {
    let mut clock = Clock::new();
    let mut vault = Arriving::new(&clock, Some(START + 5));
    let mut provider = FixtureProvider::granting("bearer", "trading data");
    assert!(run(&mut vault, &mut provider, &mut clock, START + 600).is_ok());
    assert_eq!(clock.pauses(), vec![1, 2, 4]);
    assert_eq!(vault.takes.get(), 4);
    assert_eq!(provider.calls.len(), 1, "one live request");
}

/// A code that never arrives ends `code_missing` at the deadline: takes at 0, 1, 3, 7, 15 and 30 s,
/// the last pause cut to the 10 s left, and nothing sent.
#[test]
fn a_code_that_never_arrives_ends_code_missing_at_the_deadline() {
    let mut clock = Clock::new();
    let mut vault = Arriving::new(&clock, None);
    let mut provider = FixtureProvider::granting("bearer", "trading data");
    assert_eq!(
        run(&mut vault, &mut provider, &mut clock, START + 40).err(),
        Some(ConnectError::CodeMissing)
    );
    assert_eq!(clock.pauses(), vec![1, 2, 4, 8, 15, 10]);
    assert_eq!(vault.takes.get(), 6);
    assert_eq!(clock.now.get(), START + 40, "never past the deadline");
    assert!(provider.calls.is_empty());
}

/// A deadline already reached takes nothing and pauses never: a pod whose clock runs ahead of the
/// request's deadline ends at once, even with a code waiting.
#[test]
fn a_deadline_already_reached_takes_nothing() {
    for deadline in [START, START - 1] {
        let mut clock = Clock::new();
        let mut vault = Arriving::new(&clock, Some(START));
        let mut provider = FixtureProvider::granting("bearer", "trading data");
        assert_eq!(
            run(&mut vault, &mut provider, &mut clock, deadline).err(),
            Some(ConnectError::CodeMissing)
        );
        assert_eq!(vault.takes.get(), 0, "deadline {deadline}");
        assert!(clock.pauses().is_empty());
        assert!(provider.calls.is_empty());
    }
}

/// Two exchanges racing for one code (DEC-2363 item 4): the first takes it and exchanges it, the
/// second never finds it and ends `code_missing` at its deadline; one live request in all.
#[test]
fn two_racing_exchanges_take_the_code_once() {
    let mut clock = Clock::new();
    let mut first = Arriving::new(&clock, Some(START + 3));
    let mut second = first.clone();
    let mut provider = FixtureProvider::granting("bearer", "trading data");
    assert!(run(&mut first, &mut provider, &mut clock, START + 60).is_ok());
    assert_eq!(
        run(&mut second, &mut provider, &mut clock, START + 60).err(),
        Some(ConnectError::CodeMissing)
    );
    assert_eq!(provider.calls.len(), 1, "the code is sent once");
}

/// For any arrival and any deadline within two lifetimes: the run takes exactly at the scheduled
/// instants until it finds the code, succeeds exactly when the code arrived by a scheduled take,
/// never pauses past the deadline or longer than [`MAX_PAUSE`], and sends at most one request.
#[test]
fn the_wait_follows_its_schedule_for_every_arrival_and_deadline() {
    let mut runner = TestRunner::default();
    let cases = (0i64..=1_200, proptest::option::of(0i64..=1_200));
    runner
        .run(&cases, |(window, arrival)| {
            let deadline = START + window;
            let arrives = arrival.map(|a| START + a);
            let mut clock = Clock::new();
            let mut vault = Arriving::new(&clock, arrives);
            let mut provider = FixtureProvider::granting("bearer", "trading data");
            let outcome = run(&mut vault, &mut provider, &mut clock, deadline);
            let takes = schedule(deadline);
            let found = arrives.and_then(|a| takes.iter().position(|&t| t >= a));
            let expected_takes = found.map_or(takes.len(), |index| index + 1);
            prop_assert_eq!(outcome.is_ok(), found.is_some());
            prop_assert_eq!(usize::try_from(vault.takes.get()).unwrap(), expected_takes);
            prop_assert!(clock.now.get() <= deadline.max(START));
            prop_assert!(
                clock
                    .pauses
                    .borrow()
                    .iter()
                    .all(|p| *p <= MAX_PAUSE && !p.is_zero())
            );
            prop_assert!(provider.calls.len() <= 1);
            Ok(())
        })
        .unwrap();
}
