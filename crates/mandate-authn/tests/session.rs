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
#[ignore = "pending E9-1"]
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
#[ignore = "pending E9-1"]
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
#[ignore = "pending E9-1"]
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
        Err(SessionRefusal::AbsoluteExpired)
    );

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
        Err(SessionRefusal::IdleExpired)
    );
    let late = idle.refresh(&secret(2), ProviderAnswer::Granted, &secret(3), at(quiet));
    assert_eq!(late, Err(SessionRefusal::IdleExpired));
    assert_eq!(
        idle.ended(),
        None,
        "an idle session lapses; nothing revoked it"
    );
}

#[test]
#[ignore = "pending E9-1"]
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
