//! When a session ends untouched (DEC-2113 item 2; identity spec §6.2): the earlier of its absolute
//! lifetime and, for a full session, the idle timeout from its last admitted request or granted
//! refresh. Reading it changes nothing, and a cookie that names no live session by its current
//! secret reads nothing.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;

use common::at;
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::session::{Answer, OsRandom, Sessions, Unauthenticated};
use mandate_authn::Request;
use mandate_identity::PrincipalId;
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestError, TestRunner};

const T0: i64 = 1_791_000_000;
const ACCESS_S: i64 = 300;
const IDLE_S: i64 = 3_600;
const ABSOLUTE_S: i64 = 12 * 3_600;
const SUBJECT: &str = "sha256:aaaa";

static DIRS: Mutex<u32> = Mutex::new(0);

fn sessions() -> Sessions {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-expiry-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x3e_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    Sessions::new(
        &read_session_key(&dir).expect("the key reads"),
        Box::new(OsRandom::new()),
    )
}

fn alice() -> PrincipalId {
    PrincipalId(7)
}

fn granted() -> Answer {
    Answer::Granted {
        subject: SUBJECT.to_owned(),
        principal: alice(),
    }
}

#[test]
fn a_full_session_ends_at_its_idle_deadline_unless_its_absolute_one_is_earlier() {
    let store = sessions();
    let cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened")
        .value()
        .to_owned();
    assert_eq!(store.expires_at(&cookie, at(T0)), Ok(at(T0 + IDLE_S)));
    assert_eq!(
        store.expires_at(&cookie, at(T0 + IDLE_S - 1)),
        Ok(at(T0 + IDLE_S)),
        "reading it is no activity"
    );
    assert_eq!(
        store.expires_at(&cookie, at(T0 + IDLE_S)),
        Err(Unauthenticated)
    );
    let cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened")
        .value()
        .to_owned();
    store
        .authenticate(&cookie, Request::Other, at(T0 + 100))
        .expect("admitted");
    assert_eq!(
        store.expires_at(&cookie, at(T0 + 100)),
        Ok(at(T0 + 100 + IDLE_S)),
        "an admitted request moves the idle deadline"
    );
    let mut cookie = cookie;
    let mut t = T0 + 100;
    while t <= T0 + ABSOLUTE_S - IDLE_S {
        t += ACCESS_S - 60;
        cookie = store
            .refresh(&cookie, granted(), at(t))
            .expect("granted")
            .expect("rotated")
            .value()
            .to_owned();
    }
    assert_eq!(
        store.expires_at(&cookie, at(t)),
        Ok(at(T0 + ABSOLUTE_S)),
        "a refresh moves the idle deadline, never the absolute one"
    );
    assert_eq!(
        store.expires_at(&cookie, at(T0 + ABSOLUTE_S - 1)),
        Ok(at(T0 + ABSOLUTE_S))
    );
    assert_eq!(
        store.expires_at(&cookie, at(T0 + ABSOLUTE_S)),
        Err(Unauthenticated)
    );
}

#[test]
fn only_a_live_session_by_its_current_secret_has_an_expiry() {
    let store = sessions();
    let opened = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened")
        .value()
        .to_owned();
    let current = store
        .refresh(&opened, granted(), at(T0 + 10))
        .expect("granted")
        .expect("rotated")
        .value()
        .to_owned();
    assert_eq!(
        store.expires_at(&opened, at(T0 + 11)),
        Err(Unauthenticated),
        "a secret rotated away"
    );
    assert_eq!(
        store.expires_at(&current, at(T0 + 11)),
        Ok(at(T0 + 10 + IDLE_S)),
        "and reading with it ended nothing"
    );
    let mut forged = current.clone();
    forged.replace_range(..1, if current.starts_with('0') { "1" } else { "0" });
    for cookie in [forged.as_str(), "", "zz", &"0".repeat(160)] {
        assert_eq!(store.expires_at(cookie, at(T0 + 11)), Err(Unauthenticated));
    }
    store.end(&current).expect("ended");
    assert_eq!(
        store.expires_at(&current, at(T0 + 11)),
        Err(Unauthenticated)
    );
    let narrowed = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened")
        .value()
        .to_owned();
    assert_eq!(
        store.refresh(&narrowed, Answer::Unreachable, at(T0 + 1)),
        Ok(None)
    );
    assert_eq!(
        store.expires_at(&narrowed, at(T0 + IDLE_S + 1)),
        Ok(at(T0 + ABSOLUTE_S)),
        "an outage keeps no idle timeout"
    );
}

/// One step: wait `gap` seconds and refresh, then read the expiry `probe` seconds later.
type Step = (i64, i64);

type Expiry = fn(&Sessions, &str, UtcNanos) -> Result<UtcNanos, Unauthenticated>;

fn real(store: &Sessions, cookie: &str, now: UtcNanos) -> Result<UtcNanos, Unauthenticated> {
    store.expires_at(cookie, now)
}

fn one_second_late(
    store: &Sessions,
    cookie: &str,
    now: UtcNanos,
) -> Result<UtcNanos, Unauthenticated> {
    store
        .expires_at(cookie, now)
        .map(|t| at(t.secs().saturating_add(1)))
}

fn idle_from_the_read(
    store: &Sessions,
    cookie: &str,
    now: UtcNanos,
) -> Result<UtcNanos, Unauthenticated> {
    store
        .expires_at(cookie, now)
        .map(|t| at(t.secs().max(now.secs().saturating_add(IDLE_S))))
}

/// Replays `steps` on a session opened at `T0`, comparing `expiry` with deadlines the oracle
/// keeps itself: the absolute one fixed at the opening, the idle one moved by each refresh.
fn agrees(steps: &[Step], expiry: Expiry) -> Result<(), TestCaseError> {
    let store = sessions();
    let mut cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened")
        .value()
        .to_owned();
    let absolute = T0 + ABSOLUTE_S;
    let (mut t, mut last, mut ended) = (T0, T0, false);
    for (gap, probe) in steps {
        t += gap;
        if !ended && t < absolute {
            cookie = store
                .refresh(&cookie, granted(), at(t))
                .expect("granted")
                .expect("rotated")
                .value()
                .to_owned();
            last = t;
        } else {
            ended = true;
        }
        let read = t + probe;
        let deadline = absolute.min(last + IDLE_S);
        let expected = if ended || read >= deadline {
            Err(Unauthenticated)
        } else {
            Ok(at(deadline))
        };
        prop_assert_eq!(
            expiry(&store, &cookie, at(read)),
            expected,
            "read at {}",
            read
        );
    }
    Ok(())
}

fn runner() -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases: 64,
        failure_persistence: None,
        ..ProptestConfig::default()
    })
}

#[test]
fn the_expiry_matches_deadlines_kept_apart_and_catches_seeded_bugs() {
    let steps = proptest::collection::vec((1..ACCESS_S, 0..IDLE_S + 60), 0..200);
    if let Err(failure) = runner().run(&steps, |steps| agrees(&steps, real)) {
        panic!("{failure}");
    }
    for (name, bug) in [
        ("reads one second late", one_second_late as Expiry),
        ("runs the idle timeout from the read", idle_from_the_read),
    ] {
        let caught = runner().run(&steps, |steps| agrees(&steps, bug));
        assert!(
            matches!(caught, Err(TestError::Fail(..))),
            "the oracle catches the bug that {name}"
        );
    }
}
