//! Browser sessions (§3.11 items 5 and 6, DEC-978 items 5 and 6).

mod common;

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use common::{at, hex};
use mandate_api_server::config::{MIN_SESSION_KEY_BYTES, SessionKey, read_session_key};
use mandate_api_server::session::{
    Answer, MAX_PER_PRINCIPAL, MAX_SESSIONS, ROTATION_GRACE_S, Random, Sessions, Unauthenticated,
};
use mandate_authn::Request;
use mandate_identity::PrincipalId;

const T0: i64 = 1_791_000_000;
const ACCESS_S: i64 = 300;
const IDLE_S: i64 = 3_600;
const ABSOLUTE_S: i64 = 12 * 3_600;

/// Bytes from a counter, so every id and secret differs and a test can repeat a run.
struct Counter(Mutex<u64>);

impl Random for Counter {
    fn fill(&self, out: &mut [u8]) -> Result<(), Unauthenticated> {
        let mut next = self.0.lock().expect("unpoisoned");
        for chunk in out.chunks_mut(8) {
            *next += 1;
            let bytes = next.to_be_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
        Ok(())
    }
}

struct Broken;

impl Random for Broken {
    fn fill(&self, _: &mut [u8]) -> Result<(), Unauthenticated> {
        Err(Unauthenticated)
    }
}

const KEY: [u8; MIN_SESSION_KEY_BYTES] = [0x6b; MIN_SESSION_KEY_BYTES];

fn key() -> SessionKey {
    use std::os::unix::fs::PermissionsExt;
    static KEYS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-session-{}-{}",
        std::process::id(),
        KEYS.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(mandate_api_server::config::SESSION_KEY_FILE);
    std::fs::write(&file, KEY).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("its mode");
    read_session_key(&dir).expect("the key reads")
}

fn sessions() -> Sessions {
    Sessions::new(&key(), Box::new(Counter(Mutex::new(0))))
}

fn alice() -> PrincipalId {
    PrincipalId(7)
}

fn bob() -> PrincipalId {
    PrincipalId(8)
}

const SUBJECT: &str = "sha256:aaaa";

#[test]
fn a_cookie_is_an_id_and_a_secret_tagged_under_the_session_key() {
    let store = sessions();
    let cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let value = cookie.value();
    assert_eq!(value.len(), 160);
    assert!(
        value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    let tag = ring::hmac::sign(
        &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &KEY),
        &decode(&value[..96]),
    );
    assert_eq!(&value[96..], hex(tag.as_ref()));
    assert_eq!(format!("{cookie:?}"), "Cookie(..)");
}

fn decode(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

#[test]
fn an_open_session_admits_its_principal() {
    let store = sessions();
    let cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let admitted = store
        .authenticate(cookie.value(), Request::Other, at(T0 + 1))
        .expect("admitted");
    assert_eq!(admitted.principal, alice());
    assert_eq!(store.len(), 1);
    assert_eq!(store.subject(cookie.value()), Ok(SUBJECT.to_owned()));
    let other = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let second = store
        .authenticate(other.value(), Request::Other, at(T0 + 1))
        .expect("admitted");
    assert_ne!(
        admitted.reference, second.reference,
        "each session its own reference"
    );
    assert_ne!(cookie, other);
}

#[test]
fn a_forged_truncated_or_foreign_cookie_admits_nothing() {
    let store = sessions();
    let cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let value = cookie.value().to_owned();
    let mut forged = value.clone().into_bytes();
    forged[10] = if forged[10] == b'0' { b'1' } else { b'0' };
    let forged = String::from_utf8(forged).expect("ASCII");
    let mut tag_flipped = value.clone().into_bytes();
    tag_flipped[159] = if tag_flipped[159] == b'0' { b'1' } else { b'0' };
    let tag_flipped = String::from_utf8(tag_flipped).expect("ASCII");
    let other_store = Sessions::new(&key(), Box::new(Counter(Mutex::new(1_000))));
    let foreign = other_store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    for bad in [
        forged.as_str(),
        tag_flipped.as_str(),
        &value[..158],
        &format!("{value}00"),
        &value.to_uppercase(),
        "",
        "zz",
        foreign.value(),
    ] {
        assert_eq!(
            store.authenticate(bad, Request::Other, at(T0 + 1)),
            Err(Unauthenticated),
            "{bad}"
        );
        assert_eq!(
            store.authenticate(bad, Request::KillSwitch, at(T0 + 1)),
            Err(Unauthenticated)
        );
        assert_eq!(store.end(bad), Err(Unauthenticated));
    }
    assert!(
        store
            .authenticate(&value, Request::Other, at(T0 + 2))
            .is_ok()
    );
}

#[test]
fn lapsed_access_keeps_pause_and_the_kill_switch_only() {
    for reduction in [Request::Pause, Request::KillSwitch] {
        let store = sessions();
        let cookie = store
            .open(alice(), SUBJECT.to_owned(), None, at(T0))
            .expect("opened");
        let lapsed = at(T0 + ACCESS_S + 1);
        assert_eq!(
            store.authenticate(cookie.value(), Request::Other, lapsed),
            Err(Unauthenticated)
        );
        let admitted = store
            .authenticate(cookie.value(), reduction, lapsed)
            .expect("risk reduction is served on a lapsed session");
        assert_eq!(admitted.principal, alice());
        assert_eq!(
            store.authenticate(cookie.value(), Request::Other, at(T0 + ACCESS_S + 2)),
            Err(Unauthenticated),
            "the outage reach holds no other request"
        );
        assert!(
            store
                .authenticate(cookie.value(), Request::Pause, at(T0 + ACCESS_S + 3))
                .is_ok()
        );
        assert!(
            store
                .authenticate(cookie.value(), Request::KillSwitch, at(T0 + ACCESS_S + 4))
                .is_ok()
        );
        let rotated = store
            .refresh(
                cookie.value(),
                Answer::Granted {
                    subject: SUBJECT.to_owned(),
                    principal: alice(),
                },
                at(T0 + ACCESS_S + 5),
            )
            .expect("a grant restores the session")
            .expect("and rotates its secret");
        assert!(
            store
                .authenticate(rotated.value(), Request::Other, at(T0 + ACCESS_S + 6))
                .is_ok()
        );
    }
}

#[test]
fn a_granted_refresh_rotates_and_the_old_cookie_presented_again_ends_the_session() {
    let store = sessions();
    let cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let rotated = store
        .refresh(
            cookie.value(),
            Answer::Granted {
                subject: SUBJECT.to_owned(),
                principal: alice(),
            },
            at(T0 + 200),
        )
        .expect("granted")
        .expect("a new cookie");
    assert_ne!(rotated, cookie);
    assert_eq!(
        &rotated.value()[..32],
        &cookie.value()[..32],
        "the same session id"
    );
    assert!(
        store
            .authenticate(rotated.value(), Request::Other, at(T0 + 450))
            .is_ok(),
        "the access lifetime restarts at the grant"
    );
    assert_eq!(
        store.authenticate(cookie.value(), Request::Other, at(T0 + 451)),
        Err(Unauthenticated),
        "the rotated-away cookie admits nothing"
    );
    assert_eq!(
        store.refresh(
            cookie.value(),
            Answer::Granted {
                subject: SUBJECT.to_owned(),
                principal: alice(),
            },
            at(T0 + 452)
        ),
        Err(Unauthenticated)
    );
    assert_eq!(
        store.authenticate(rotated.value(), Request::KillSwitch, at(T0 + 453)),
        Err(Unauthenticated),
        "reuse revokes the whole family"
    );
    assert!(store.is_empty());
}

#[test]
fn an_outage_refresh_keeps_the_secret_and_narrows_the_session() {
    let store = sessions();
    let cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    assert_eq!(
        store.refresh(cookie.value(), Answer::Unreachable, at(T0 + 10)),
        Ok(None)
    );
    assert_eq!(
        store.authenticate(cookie.value(), Request::Other, at(T0 + 11)),
        Err(Unauthenticated)
    );
    assert!(
        store
            .authenticate(cookie.value(), Request::Pause, at(T0 + 12))
            .is_ok()
    );
    assert!(
        store
            .authenticate(cookie.value(), Request::KillSwitch, at(T0 + 13))
            .is_ok()
    );
    assert!(
        store
            .refresh(
                cookie.value(),
                Answer::Granted {
                    subject: SUBJECT.to_owned(),
                    principal: alice(),
                },
                at(T0 + 14)
            )
            .expect("granted")
            .is_some()
    );
}

#[test]
fn a_failed_refresh_or_another_subject_ends_the_session() {
    for answer in [
        Answer::Failed,
        Answer::Granted {
            subject: "sha256:bbbb".to_owned(),
            principal: alice(),
        },
    ] {
        let store = sessions();
        let cookie = store
            .open(alice(), SUBJECT.to_owned(), None, at(T0))
            .expect("opened");
        assert_eq!(
            store.refresh(cookie.value(), answer.clone(), at(T0 + 10)),
            Err(Unauthenticated),
            "{answer:?}"
        );
        assert_eq!(
            store.authenticate(cookie.value(), Request::KillSwitch, at(T0 + 11)),
            Err(Unauthenticated)
        );
        assert!(store.is_empty());
    }
}

#[test]
fn sign_out_ends_the_session_once() {
    let store = sessions();
    let cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    assert_eq!(store.end(cookie.value()), Ok(()));
    assert_eq!(
        store.authenticate(cookie.value(), Request::Pause, at(T0 + 1)),
        Err(Unauthenticated)
    );
    assert_eq!(store.end(cookie.value()), Err(Unauthenticated));
    assert!(store.is_empty());
}

#[test]
fn opening_with_a_cookie_ends_that_session_and_never_reuses_it() {
    let store = sessions();
    let first = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let second = store
        .open(
            bob(),
            "sha256:bbbb".to_owned(),
            Some(first.value()),
            at(T0 + 1),
        )
        .expect("opened");
    assert_ne!(&first.value()[..32], &second.value()[..32]);
    assert_eq!(
        store.authenticate(first.value(), Request::Pause, at(T0 + 2)),
        Err(Unauthenticated)
    );
    assert_eq!(
        store
            .authenticate(second.value(), Request::Other, at(T0 + 2))
            .map(|a| a.principal),
        Ok(bob())
    );
    assert_eq!(store.len(), 1);
    let third = store
        .open(
            alice(),
            SUBJECT.to_owned(),
            Some("not-a-cookie"),
            at(T0 + 3),
        )
        .expect("an unreadable replaced cookie opens anyway");
    assert!(
        store
            .authenticate(third.value(), Request::Other, at(T0 + 4))
            .is_ok()
    );
}

#[test]
fn a_principal_holds_at_most_sixteen_sessions_and_the_oldest_ends() {
    let store = sessions();
    let mut cookies = Vec::new();
    for i in 0..MAX_PER_PRINCIPAL {
        cookies.push(
            store
                .open(alice(), SUBJECT.to_owned(), None, at(T0 + i as i64))
                .expect("opened"),
        );
    }
    let bobs = store
        .open(bob(), "sha256:bbbb".to_owned(), None, at(T0 + 20))
        .expect("opened");
    assert_eq!(store.len(), MAX_PER_PRINCIPAL + 1);
    let newest = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0 + 21))
        .expect("opened");
    assert_eq!(store.len(), MAX_PER_PRINCIPAL + 1);
    let now = at(T0 + 22);
    assert_eq!(
        store.authenticate(cookies[0].value(), Request::Pause, now),
        Err(Unauthenticated),
        "the oldest ended"
    );
    for cookie in cookies.iter().skip(1).chain([&newest, &bobs]) {
        assert!(
            store
                .authenticate(cookie.value(), Request::Other, now)
                .is_ok()
        );
    }
}

#[test]
fn a_full_store_refuses_a_new_session() {
    let store = sessions();
    for i in 0..MAX_SESSIONS {
        store
            .open(
                PrincipalId(1_000 + i as u128),
                SUBJECT.to_owned(),
                None,
                at(T0),
            )
            .expect("opened");
    }
    assert_eq!(
        store.open(alice(), SUBJECT.to_owned(), None, at(T0)),
        Err(Unauthenticated)
    );
    assert_eq!(store.len(), MAX_SESSIONS);
}

#[test]
fn the_idle_timeout_and_the_absolute_lifetime_end_a_session() {
    let store = sessions();
    let cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    assert_eq!(
        store.authenticate(cookie.value(), Request::KillSwitch, at(T0 + IDLE_S)),
        Err(Unauthenticated),
        "idle for the business hour"
    );
    let store = sessions();
    let mut cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let mut t = T0;
    while t + 240 < T0 + ABSOLUTE_S {
        t += 240;
        cookie = store
            .refresh(
                cookie.value(),
                Answer::Granted {
                    subject: SUBJECT.to_owned(),
                    principal: alice(),
                },
                at(t),
            )
            .expect("granted")
            .expect("rotated");
    }
    assert!(
        store
            .authenticate(cookie.value(), Request::Other, at(t + 1))
            .is_ok()
    );
    assert_eq!(
        store.authenticate(cookie.value(), Request::KillSwitch, at(T0 + ABSOLUTE_S)),
        Err(Unauthenticated),
        "the absolute business lifetime"
    );
    assert!(store.is_empty());
}

#[test]
fn no_randomness_opens_nothing() {
    let store = Sessions::new(&key(), Box::new(Broken));
    assert_eq!(
        store.open(alice(), SUBJECT.to_owned(), None, at(T0)),
        Err(Unauthenticated)
    );
    assert!(store.is_empty());
}

#[test]
fn pause_or_kill_switch_first_on_a_lapsed_session_is_served() {
    for reduction in [Request::Pause, Request::KillSwitch] {
        let store = sessions();
        let cookie = store
            .open(alice(), SUBJECT.to_owned(), None, at(T0))
            .expect("opened");
        assert!(!store.is_empty());
        assert!(
            store
                .authenticate(cookie.value(), reduction, at(T0 + ACCESS_S + 1))
                .is_ok(),
            "{reduction:?} with no request before it"
        );
    }
}

#[test]
fn an_ordinary_request_on_a_lapsed_session_does_not_narrow_it() {
    let store = sessions();
    let cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    assert_eq!(
        store.authenticate(cookie.value(), Request::Other, at(T0 + ACCESS_S + 1)),
        Err(Unauthenticated)
    );
    assert_eq!(
        store.authenticate(cookie.value(), Request::Pause, at(T0 + IDLE_S + 1)),
        Err(Unauthenticated),
        "still a full session, so its idle timeout ended it; an outage session would have none"
    );
}

#[test]
fn a_session_is_purged_at_its_absolute_lifetime_exactly() {
    let store = sessions();
    let first = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    assert_eq!(
        store.refresh(first.value(), Answer::Unreachable, at(T0 + 1)),
        Ok(None),
        "an outage session has no idle timeout, so only its absolute end purges it"
    );
    store
        .open(
            bob(),
            "sha256:bbbb".to_owned(),
            None,
            at(T0 + ABSOLUTE_S - 1),
        )
        .expect("opened");
    assert_eq!(store.len(), 2, "one second before, the first is kept");
    store
        .open(bob(), "sha256:bbbb".to_owned(), None, at(T0 + ABSOLUTE_S))
        .expect("opened");
    assert_eq!(store.len(), 2, "at its absolute end the first is purged");
}

#[test]
fn the_system_clock_reads_the_present() {
    use mandate_api_server::clock::{Clock, SystemClock};
    let now = SystemClock.now().expect("the host's clock");
    assert!(now.secs() > 1_790_000_000, "{}", now.secs());
}

/// Bytes from a counter until switched off, then nothing.
struct Toggle(Counter, AtomicBool);

impl Random for Toggle {
    fn fill(&self, out: &mut [u8]) -> Result<(), Unauthenticated> {
        if self.1.load(Ordering::SeqCst) {
            self.0.fill(out)
        } else {
            Err(Unauthenticated)
        }
    }
}

#[test]
fn without_randomness_pause_and_the_kill_switch_are_still_served() {
    let toggle = std::sync::Arc::new(Toggle(Counter(Mutex::new(0)), AtomicBool::new(true)));
    struct Shared(std::sync::Arc<Toggle>);
    impl Random for Shared {
        fn fill(&self, out: &mut [u8]) -> Result<(), Unauthenticated> {
            self.0.fill(out)
        }
    }
    let store = Sessions::new(&key(), Box::new(Shared(std::sync::Arc::clone(&toggle))));
    let live = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let lapsing = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    toggle.1.store(false, Ordering::SeqCst);
    for request in [Request::Other, Request::Pause, Request::KillSwitch] {
        assert!(
            store
                .authenticate(live.value(), request, at(T0 + 10))
                .is_ok(),
            "{request:?} on a live session"
        );
    }
    for request in [Request::Pause, Request::KillSwitch] {
        assert!(
            store
                .authenticate(lapsing.value(), request, at(T0 + ACCESS_S + 10))
                .is_ok(),
            "{request:?} on a lapsed session"
        );
    }
    assert_eq!(
        store.open(alice(), SUBJECT.to_owned(), Some(live.value()), at(T0 + 20)),
        Err(Unauthenticated)
    );
    assert!(
        store
            .authenticate(live.value(), Request::Other, at(T0 + 21))
            .is_ok(),
        "a refused open ends no session"
    );
    assert_eq!(store.len(), 2);
}

#[test]
fn an_idle_full_session_is_purged_but_an_outage_session_is_not() {
    let store = sessions();
    let idle = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let outage = store
        .open(bob(), "sha256:bbbb".to_owned(), None, at(T0))
        .expect("opened");
    assert_eq!(
        store.refresh(outage.value(), Answer::Unreachable, at(T0 + 1)),
        Ok(None)
    );
    store
        .open(
            PrincipalId(9),
            "sha256:cccc".to_owned(),
            None,
            at(T0 + IDLE_S - 1),
        )
        .expect("opened");
    assert_eq!(
        store.len(),
        3,
        "one second before the idle timeout, all are kept"
    );
    store
        .open(
            PrincipalId(9),
            "sha256:cccc".to_owned(),
            None,
            at(T0 + IDLE_S),
        )
        .expect("opened");
    assert_eq!(
        store.len(),
        3,
        "the idle full session is purged; the outage one is kept"
    );
    assert_eq!(
        store.authenticate(idle.value(), Request::Pause, at(T0 + IDLE_S)),
        Err(Unauthenticated)
    );
    assert!(
        store
            .authenticate(outage.value(), Request::Pause, at(T0 + IDLE_S))
            .is_ok()
    );
}

#[test]
fn a_rotated_cookie_presented_at_sign_in_ends_its_session() {
    let store = sessions();
    let first = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let rotated = store
        .refresh(
            first.value(),
            Answer::Granted {
                subject: SUBJECT.to_owned(),
                principal: alice(),
            },
            at(T0 + 10),
        )
        .expect("granted")
        .expect("rotated");
    store
        .open(
            alice(),
            SUBJECT.to_owned(),
            Some(first.value()),
            at(T0 + 20),
        )
        .expect("opened");
    assert_eq!(
        store.authenticate(rotated.value(), Request::Pause, at(T0 + 21)),
        Err(Unauthenticated)
    );
    assert_eq!(store.len(), 1);
}

/// A command in flight while a refresh rotates the cookie (DEC-1022): for [`ROTATION_GRACE_S`]
/// seconds after a granted refresh, the cookie it rotated away still serves a pause and a kill
/// switch; any other call with it is refused with the session kept; after the window, or on the
/// refresh route, it is reuse and ends the session.
#[test]
fn a_just_rotated_cookie_serves_risk_reduction_briefly_and_is_reuse_otherwise() {
    let granted = || Answer::Granted {
        subject: SUBJECT.to_owned(),
        principal: alice(),
    };
    let store = sessions();
    let cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let rotated_at = T0 + 100;
    let rotated = store
        .refresh(cookie.value(), granted(), at(rotated_at))
        .expect("granted")
        .expect("a new cookie");
    let edge = at(rotated_at + ROTATION_GRACE_S);
    assert!(
        store
            .authenticate(cookie.value(), Request::Pause, edge)
            .is_ok()
    );
    assert!(
        store
            .authenticate(cookie.value(), Request::KillSwitch, edge)
            .is_ok()
    );
    assert_eq!(
        store.authenticate(cookie.value(), Request::Other, edge),
        Err(Unauthenticated),
        "an ordinary call with it is refused"
    );
    assert!(
        store
            .authenticate(rotated.value(), Request::Other, edge)
            .is_ok(),
        "and the session is kept"
    );
    assert_eq!(
        store.authenticate(
            cookie.value(),
            Request::KillSwitch,
            at(rotated_at + ROTATION_GRACE_S + 1)
        ),
        Err(Unauthenticated),
        "after the window it admits nothing"
    );
    assert!(
        store.is_empty(),
        "and its use is reuse, which ends the session"
    );

    let store = sessions();
    let cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let rotated = store
        .refresh(cookie.value(), granted(), at(T0 + 100))
        .expect("granted")
        .expect("a new cookie");
    let _again = store
        .refresh(rotated.value(), granted(), at(T0 + 110))
        .expect("granted")
        .expect("a third cookie");
    assert!(
        store
            .authenticate(rotated.value(), Request::Pause, at(T0 + 111))
            .is_ok(),
        "the cookie the last refresh rotated away is served"
    );
    assert_eq!(
        store.authenticate(cookie.value(), Request::Pause, at(T0 + 112)),
        Err(Unauthenticated),
        "an older one is not, even within its own window"
    );
    assert!(store.is_empty(), "it is reuse");

    let store = sessions();
    let cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let rotated = store
        .refresh(cookie.value(), granted(), at(T0 + 100))
        .expect("granted")
        .expect("a new cookie");
    assert_eq!(
        store.refresh(cookie.value(), granted(), at(T0 + 101)),
        Err(Unauthenticated),
        "a refresh with it, even within the window, is reuse"
    );
    assert_eq!(
        store.authenticate(rotated.value(), Request::KillSwitch, at(T0 + 102)),
        Err(Unauthenticated)
    );
    assert!(store.is_empty(), "and reuse ends the session");
}

/// The window admits only what the session itself still admits (DEC-1022): a narrowed session
/// still refuses an ordinary call, the absolute lifetime still ends it, and signing out takes the
/// current cookie only.
#[test]
fn a_graced_cookie_admits_no_more_than_the_session_does() {
    let granted = || Answer::Granted {
        subject: SUBJECT.to_owned(),
        principal: alice(),
    };
    let store = sessions();
    let cookie = store
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let rotated = store
        .refresh(cookie.value(), granted(), at(T0 + 100))
        .expect("granted")
        .expect("a new cookie");
    assert_eq!(
        store.refresh(rotated.value(), Answer::Unreachable, at(T0 + 105)),
        Ok(None),
        "an outage narrows the session"
    );
    assert!(
        store
            .authenticate(cookie.value(), Request::Pause, at(T0 + 111))
            .is_ok(),
        "a narrowed session still serves a pause with it"
    );
    assert_eq!(
        store.end(cookie.value()),
        Err(Unauthenticated),
        "signing out takes the current cookie only"
    );
    assert!(
        store
            .authenticate(rotated.value(), Request::Pause, at(T0 + 112))
            .is_ok(),
        "and the refused sign-out kept the session"
    );

    let late = sessions();
    let mut held = late
        .open(alice(), SUBJECT.to_owned(), None, at(T0))
        .expect("opened");
    let mut t = T0;
    while t + 240 < T0 + ABSOLUTE_S - 10 {
        t += 240;
        held = late
            .refresh(held.value(), granted(), at(t))
            .expect("granted")
            .expect("rotated");
    }
    let before = held.clone();
    let _last = late
        .refresh(held.value(), granted(), at(T0 + ABSOLUTE_S - 10))
        .expect("granted")
        .expect("rotated");
    assert!(
        late.authenticate(before.value(), Request::Pause, at(T0 + ABSOLUTE_S - 1))
            .is_ok(),
        "served before the absolute lifetime ends"
    );
    assert_eq!(
        late.authenticate(before.value(), Request::KillSwitch, at(T0 + ABSOLUTE_S + 1)),
        Err(Unauthenticated),
        "but not past it, within the window or not"
    );
}
