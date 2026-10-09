//! A full session's lifetime (identity spec §6.2; backlog E9-1, slice A2): the limits an org may
//! only shorten, the 5-minute access token, the idle and absolute limits, and refresh rotation
//! with reuse revoking the family. Times are whole seconds from [`T0`]; each expected outcome is
//! written from the spec's numbers.

use mandate_authn::{
    ACCESS_TOKEN_LIFETIME_S, EndReason, OrgKind, ProviderAnswer, Reach, RefreshSecret, Refreshed,
    Request, SessionLimits, SessionPolicy, SessionRecord, SessionRefusal, UtcNanos,
};

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
fn limits_default_by_org_kind_and_an_org_may_only_shorten_them() {
    let defaults = |kind| SessionLimits::resolve(kind, SessionPolicy::default()).unwrap();
    let retail = defaults(OrgKind::Individual);
    assert_eq!((retail.idle_s(), retail.absolute_s()), (DAY, 7 * DAY));
    let fund = defaults(OrgKind::Business);
    assert_eq!((fund.idle_s(), fund.absolute_s()), (HOUR, 12 * HOUR));
    let policy = |idle_s, absolute_s| SessionPolicy { idle_s, absolute_s };
    let shorter = SessionLimits::resolve(OrgKind::Business, policy(Some(600), Some(HOUR))).unwrap();
    assert_eq!((shorter.idle_s(), shorter.absolute_s()), (600, HOUR));
    let same = SessionLimits::resolve(OrgKind::Individual, policy(Some(DAY), Some(7 * DAY)));
    assert_eq!(same, Ok(retail));
    for (kind, idle, absolute) in [
        (OrgKind::Business, Some(HOUR + 1), None),
        (OrgKind::Business, None, Some(12 * HOUR + 1)),
        (OrgKind::Business, Some(DAY), None),
        (OrgKind::Individual, None, Some(7 * DAY + 1)),
        (OrgKind::Individual, Some(0), None),
        (OrgKind::Individual, None, Some(-1)),
    ] {
        let resolved = SessionLimits::resolve(kind, policy(idle, absolute));
        assert_eq!(
            resolved,
            Err(SessionRefusal::LimitNotShortened),
            "{kind:?} {idle:?} {absolute:?}"
        );
    }
}

#[test]
fn an_access_token_lasts_five_minutes_and_a_granted_refresh_rotates_it() {
    assert_eq!(ACCESS_TOKEN_LIFETIME_S, 300);
    let mut s = open();
    assert_eq!(s.reach(), Reach::Full);
    assert!(s.can_present_step_up());
    assert_eq!(s.authorize(Request::Other, at(299)), Ok(()));
    assert_eq!(
        s.authorize(Request::Other, at(300)),
        Err(SessionRefusal::AccessExpired)
    );
    let rotated = s.refresh(&secret(1), ProviderAnswer::Granted, &secret(2), at(300));
    assert_eq!(
        rotated,
        Ok(Refreshed::Rotated {
            access_expires_at: at(600)
        })
    );
    assert_eq!(s.authorize(Request::Other, at(599)), Ok(()));
    assert_eq!(
        s.authorize(Request::Other, at(600)),
        Err(SessionRefusal::AccessExpired)
    );
}

#[test]
fn the_idle_timeout_counts_from_the_last_admitted_request_and_the_absolute_from_the_opening() {
    let mut s = open();
    let mut token = 1;
    let mut now = 0;
    while now + 50 * 60 < 12 * HOUR {
        now += 50 * 60;
        let next = token + 1;
        assert!(
            s.refresh(
                &secret(token),
                ProviderAnswer::Granted,
                &secret(next),
                at(now)
            )
            .is_ok(),
            "{now}"
        );
        token = next;
        assert_eq!(s.authorize(Request::Other, at(now)), Ok(()), "{now}");
    }
    let end = 12 * HOUR - 1;
    assert!(end - now < HOUR, "the run stays within the idle timeout");
    assert!(
        s.refresh(
            &secret(token),
            ProviderAnswer::Granted,
            &secret(token + 1),
            at(end)
        )
        .is_ok()
    );
    assert_eq!(s.authorize(Request::Pause, at(end)), Ok(()));
    assert_eq!(
        s.authorize(Request::Pause, at(12 * HOUR)),
        Err(ended(EndReason::Expired)),
        "the absolute lifetime ends the session"
    );
    assert_eq!(s.ended(), Some(EndReason::Expired));
    assert_refused_after(&mut s, token + 1, at(12 * HOUR + 1), EndReason::Expired);

    let mut idle = open();
    let refreshed = idle.refresh(
        &secret(1),
        ProviderAnswer::Granted,
        &secret(2),
        at(HOUR - 1),
    );
    assert!(refreshed.is_ok(), "one second inside the idle timeout");
    assert_eq!(idle.authorize(Request::Other, at(HOUR + 298)), Ok(()));
    let quiet = 2 * HOUR + 298;
    assert_eq!(
        idle.authorize(Request::Pause, at(quiet)),
        Err(ended(EndReason::Expired)),
        "the idle timeout ends the session"
    );
    assert_eq!(idle.ended(), Some(EndReason::Expired));
    assert_refused_after(&mut idle, 2, at(quiet), EndReason::Expired);

    let mut quiet_refresh = open();
    let lapsed = quiet_refresh.refresh(&secret(1), ProviderAnswer::Granted, &secret(2), at(HOUR));
    assert_eq!(
        lapsed,
        Err(ended(EndReason::Expired)),
        "a refresh at the idle timeout"
    );
    assert_eq!(quiet_refresh.ended(), Some(EndReason::Expired));
    assert_refused_after(&mut quiet_refresh, 1, at(HOUR), EndReason::Expired);
}

/// Every call after the one that ended `s` is refused `Ended { reason }`, whatever it asks, and
/// `end` reports nothing new, so the reason is journaled once.
fn assert_refused_after(s: &mut SessionRecord, token: u8, now: UtcNanos, reason: EndReason) {
    for request in [Request::Pause, Request::KillSwitch, Request::Other] {
        assert_eq!(s.authorize(request, now), Err(ended(reason)), "{request:?}");
    }
    let refresh = s.refresh(&secret(token), ProviderAnswer::Granted, &secret(200), now);
    assert_eq!(refresh, Err(ended(reason)), "a refresh");
    assert_eq!(s.ended(), Some(reason), "the first reason stays");
    assert_eq!(s.end(EndReason::SignOut), None, "journaled once");
}

#[test]
fn a_rotated_refresh_token_presented_again_revokes_the_whole_family() {
    let mut s = open();
    let first = s.refresh(&secret(1), ProviderAnswer::Granted, &secret(2), at(300));
    assert!(matches!(first, Ok(Refreshed::Rotated { .. })));
    let unknown = s.refresh(&secret(9), ProviderAnswer::Granted, &secret(3), at(310));
    assert_eq!(unknown, Err(SessionRefusal::UnknownRefreshToken));
    assert_eq!(s.ended(), None, "an unknown token revokes nothing");
    let reused = s.refresh(&secret(1), ProviderAnswer::Granted, &secret(3), at(320));
    assert_eq!(reused, Err(ended(EndReason::RefreshReuse)));
    assert_eq!(s.ended(), Some(EndReason::RefreshReuse));
    assert!(
        !s.can_present_step_up(),
        "an ended session presents nothing"
    );
    let current = s.refresh(&secret(2), ProviderAnswer::Granted, &secret(4), at(330));
    assert_eq!(
        current,
        Err(ended(EndReason::RefreshReuse)),
        "the current token died with it"
    );
    for request in [Request::Pause, Request::KillSwitch, Request::Other] {
        assert_eq!(
            s.authorize(request, at(331)),
            Err(ended(EndReason::RefreshReuse))
        );
    }
    assert_eq!(
        s.end(EndReason::SignOut),
        None,
        "journaled once, as the reuse"
    );
}

#[test]
fn an_idle_timeout_longer_than_the_absolute_lifetime_is_refused() {
    let policy = |idle_s, absolute_s| SessionPolicy { idle_s, absolute_s };
    let short_life = SessionLimits::resolve(OrgKind::Business, policy(None, Some(600)));
    assert_eq!(
        short_life,
        Err(SessionRefusal::IdleLongerThanAbsolute),
        "the default idle hour"
    );
    let equal = SessionLimits::resolve(OrgKind::Business, policy(Some(600), Some(600))).unwrap();
    assert_eq!((equal.idle_s(), equal.absolute_s()), (600, 600));
    let longer = SessionLimits::resolve(OrgKind::Individual, policy(Some(601), Some(600)));
    assert_eq!(longer, Err(SessionRefusal::IdleLongerThanAbsolute));
}

#[test]
fn a_refresh_never_rotates_to_a_secret_used_before() {
    let mut s = open();
    let same = s.refresh(&secret(1), ProviderAnswer::Granted, &secret(1), at(60));
    assert_eq!(
        same,
        Err(SessionRefusal::RefreshSecretReused),
        "the presented secret"
    );
    assert!(
        s.refresh(&secret(1), ProviderAnswer::Granted, &secret(2), at(61))
            .is_ok()
    );
    let rotated = s.refresh(&secret(2), ProviderAnswer::Granted, &secret(1), at(62));
    assert_eq!(
        rotated,
        Err(SessionRefusal::RefreshSecretReused),
        "a rotated secret"
    );
    assert_eq!(s.ended(), None, "the refusal revokes nothing");
    assert!(
        s.refresh(&secret(2), ProviderAnswer::Granted, &secret(3), at(63))
            .is_ok()
    );
}

#[test]
fn a_clock_behind_the_session_fails_closed() {
    let mut s = open();
    assert_eq!(
        s.authorize(Request::Other, at(-1)),
        Err(SessionRefusal::ClockBehind),
        "a request before the opening"
    );
    let early = s.refresh(&secret(1), ProviderAnswer::Granted, &secret(2), at(-1));
    assert_eq!(
        early,
        Err(SessionRefusal::ClockBehind),
        "a refresh before the opening"
    );
    assert_eq!(s.authorize(Request::Other, at(100)), Ok(()));
    assert_eq!(
        s.authorize(Request::Other, at(99)),
        Err(SessionRefusal::ClockBehind),
        "a request before the last activity"
    );
    let behind = s.refresh(&secret(1), ProviderAnswer::Granted, &secret(2), at(50));
    assert_eq!(
        behind,
        Err(SessionRefusal::ClockBehind),
        "a refresh before the last activity"
    );
    assert_eq!(s.authorize(Request::Pause, at(99)), Ok(()), "rule 13");
    assert_eq!(s.authorize(Request::KillSwitch, at(-1)), Ok(()), "rule 13");
    assert_eq!(s.ended(), None, "a clock behind ends nothing");
    let kept = s.refresh(
        &secret(1),
        ProviderAnswer::Granted,
        &secret(2),
        at(HOUR + 99),
    );
    assert_eq!(
        kept,
        Ok(Refreshed::Rotated {
            access_expires_at: at(HOUR + 399)
        }),
        "the idle timer still counts from 100, and no refused refresh consumed the token"
    );
    assert_eq!(
        s.authorize(Request::Other, at(HOUR + 99)),
        Ok(()),
        "the same instant is not behind"
    );
}

#[test]
fn the_record_holds_sha256_digests_and_never_a_raw_secret() {
    let sha = |n: u8| {
        let mut out = [0u8; 32];
        out.copy_from_slice(ring::digest::digest(&ring::digest::SHA256, &[n; 32]).as_ref());
        out
    };
    let mut s = open();
    assert_eq!(s.refresh_digest(), Some(sha(1)));
    assert_eq!(s.rotated_digests(), Vec::<[u8; 32]>::new());
    for (n, t) in [(2u8, 60), (3, 120), (4, 180)] {
        assert!(
            s.refresh(&secret(n - 1), ProviderAnswer::Granted, &secret(n), at(t))
                .is_ok()
        );
    }
    assert_eq!(s.refresh_digest(), Some(sha(4)));
    let mut rotated = vec![sha(1), sha(2), sha(3)];
    rotated.sort();
    assert_eq!(s.rotated_digests(), rotated);
    let stored: Vec<[u8; 32]> = s
        .rotated_digests()
        .into_iter()
        .chain(s.refresh_digest())
        .collect();
    for n in 1..=4u8 {
        assert!(!stored.contains(&[n; 32]), "a raw secret {n} is stored");
    }
}

#[test]
fn the_idle_timer_never_runs_past_the_absolute_lifetime_at_the_clock_s_end() {
    const MAX_SECS: i64 = 253_402_300_799;
    let limits = SessionLimits::resolve(
        OrgKind::Business,
        SessionPolicy {
            idle_s: Some(HOUR),
            absolute_s: Some(HOUR),
        },
    )
    .unwrap();
    let start = MAX_SECS - HOUR;
    let opened = UtcNanos::from_parts(start, 0).unwrap();
    let mut s = SessionRecord::open(limits, &secret(1), opened).unwrap();
    let late = UtcNanos::from_parts(start + 10, 0).unwrap();
    assert_eq!(
        s.authorize(Request::Other, late),
        Ok(()),
        "an hour past `late` is past the clock"
    );
    let last = UtcNanos::from_parts(start + 299, 0).unwrap();
    assert_eq!(s.authorize(Request::KillSwitch, last), Ok(()));
    let clock_end = UtcNanos::from_parts(MAX_SECS, 0).unwrap();
    assert_eq!(
        s.authorize(Request::KillSwitch, clock_end),
        Err(ended(EndReason::Expired)),
        "the absolute lifetime ends at the clock's last second, with no overflow"
    );
}
