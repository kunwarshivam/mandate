//! The risk-reduction routes 1 and 2 of identity spec §6.4 and the deprovision signal of §11.1
//! (backlog E9-1, slice A2, DEC-652 item 3): an unreachable provider, a failed refresh, a
//! deprovision, the reduction-only session, and an idle lapse a granted refresh does not restore
//! (DEC-816 item 8). A lapse ends the session as `expired`, and every later call is refused the
//! same way. The property over random histories is in `routes_properties.rs`. Times are whole
//! seconds from [`T0`].

use mandate_authn::{
    EndReason, OrgKind, ProviderAnswer, REDUCTION_ONLY_LIFETIME_S, Reach, RefreshSecret, Refreshed,
    Request, SessionLimits, SessionPolicy, SessionRecord, SessionRefusal, SubjectStanding,
    UtcNanos,
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

/// Every call after the one that ended `s` is refused `Ended { reason }`, whatever it asks, and
/// `end` reports nothing new, so the reason is journaled once.
fn assert_refused_after(s: &mut SessionRecord, token: u8, now: UtcNanos, reason: EndReason) {
    assert_eq!(s.ended(), Some(reason), "the reason to journal");
    for request in [Request::Pause, Request::KillSwitch, Request::Other] {
        assert_eq!(s.authorize(request, now), Err(ended(reason)), "{request:?}");
    }
    let refresh = s.refresh(&secret(token), ProviderAnswer::Granted, &secret(200), now);
    assert_eq!(refresh, Err(ended(reason)), "a refresh");
    assert_eq!(s.end(EndReason::SignOut), None, "journaled once");
}

#[test]
#[ignore = "pending E9-1"]
fn an_unreachable_provider_leaves_pause_and_the_kill_switch_until_the_absolute_lifetime() {
    for answer in [
        ProviderAnswer::Unreachable,
        ProviderAnswer::Status(500),
        ProviderAnswer::Status(503),
        ProviderAnswer::Status(599),
    ] {
        let mut s = open();
        let outcome = s.refresh(&secret(1), answer, &secret(2), at(300));
        assert_eq!(outcome, Ok(Refreshed::Outage), "{answer:?}");
        assert_eq!(s.reach(), Reach::Outage);
        assert!(!s.can_present_step_up());
        assert_eq!(
            s.authorize(Request::Other, at(301)),
            Err(SessionRefusal::RiskReductionOnly)
        );
        assert_eq!(
            s.authorize(Request::Pause, at(5 * HOUR)),
            Ok(()),
            "no idle timeout in an outage"
        );
        assert_eq!(s.authorize(Request::KillSwitch, at(12 * HOUR - 1)), Ok(()));
        assert_eq!(
            s.authorize(Request::KillSwitch, at(12 * HOUR)),
            Err(ended(EndReason::Expired)),
            "{answer:?}"
        );
        assert_refused_after(&mut s, 1, at(12 * HOUR + 1), EndReason::Expired);
    }
    let mut s = in_an_outage();
    let restored = s.refresh(&secret(1), ProviderAnswer::Granted, &secret(3), at(900));
    assert_eq!(
        restored,
        Ok(Refreshed::Rotated {
            access_expires_at: at(1200)
        }),
        "the token stayed current"
    );
    assert_eq!(s.reach(), Reach::Full);
    assert_eq!(s.authorize(Request::Other, at(901)), Ok(()));
    let reused = s.refresh(&secret(1), ProviderAnswer::Granted, &secret(4), at(902));
    assert_eq!(reused, Err(ended(EndReason::RefreshReuse)));
}

#[test]
#[ignore = "pending E9-1"]
fn any_other_answer_ends_the_session_with_every_permission() {
    let cases = [400, 401, 403, 404, 408, 429, 200, 204, 302, 499, 600]
        .map(|code| (ProviderAnswer::Status(code), EndReason::RefreshFailed))
        .into_iter()
        .chain([(ProviderAnswer::Deprovision, EndReason::Deprovisioned)]);
    for (answer, reason) in cases {
        for outage_first in [false, true] {
            let mut s = open();
            if outage_first {
                let outage =
                    s.refresh(&secret(1), ProviderAnswer::Unreachable, &secret(2), at(300));
                assert_eq!(outage, Ok(Refreshed::Outage));
            }
            let outcome = s.refresh(&secret(1), answer, &secret(2), at(400));
            assert_eq!(outcome, Err(ended(reason)), "{answer:?}");
            assert_eq!(s.ended(), Some(reason), "{answer:?}");
            for request in [Request::Pause, Request::KillSwitch, Request::Other] {
                assert_eq!(
                    s.authorize(request, at(401)),
                    Err(ended(reason)),
                    "{answer:?}"
                );
            }
        }
    }
    let mut s = open();
    let logout = s.end(EndReason::Deprovisioned);
    assert_eq!(
        logout,
        Some(EndReason::Deprovisioned),
        "a back-channel logout"
    );
    assert_eq!(s.end(EndReason::Deprovisioned), None);
    assert_eq!(
        s.authorize(Request::KillSwitch, at(1)),
        Err(ended(EndReason::Deprovisioned))
    );
}

#[test]
#[ignore = "pending E9-1"]
fn only_a_deprovision_signal_closes_the_local_passkey_route() {
    let signed_in = SubjectStanding {
        last_sign_in: Some(at(-DAY)),
        last_deprovision: None,
    };
    for (answer, reason) in [
        (ProviderAnswer::Status(429), EndReason::RefreshFailed),
        (ProviderAnswer::Status(408), EndReason::RefreshFailed),
        (ProviderAnswer::Status(400), EndReason::RefreshFailed),
        (ProviderAnswer::Deprovision, EndReason::Deprovisioned),
    ] {
        let mut s = open();
        assert_eq!(
            s.refresh(&secret(1), answer, &secret(2), at(300)),
            Err(ended(reason)),
            "{answer:?}"
        );
        let standing = if answer == ProviderAnswer::Deprovision {
            signed_in.saw_deprovision(at(300))
        } else {
            signed_in
        };
        let route_two = SessionRecord::open_reduction_only(standing, at(301));
        if answer == ProviderAnswer::Deprovision {
            assert_eq!(route_two, Err(SessionRefusal::DeprovisionSeen));
        } else {
            assert!(
                route_two.is_ok(),
                "a refresh_failed leaves route 2 open: {answer:?}"
            );
        }
    }
    let seen = signed_in.saw_deprovision(at(5));
    assert_eq!(seen.last_deprovision, Some(at(5)));
    assert_eq!(seen.last_sign_in, signed_in.last_sign_in);
    let late_older = seen.saw_deprovision(at(2));
    assert_eq!(
        late_older.last_deprovision,
        Some(at(5)),
        "an older signal arriving late moves nothing back"
    );
    let signed_in_again = SubjectStanding {
        last_sign_in: Some(at(10)),
        ..seen
    };
    assert!(
        SessionRecord::open_reduction_only(signed_in_again, at(11)).is_ok(),
        "a later successful sign-in reopens route 2"
    );
}

#[test]
#[ignore = "pending E9-1"]
fn a_deprovision_signal_closes_route_two_whatever_any_session_is_doing() {
    let signed_in = SubjectStanding {
        last_sign_in: Some(at(-DAY)),
        last_deprovision: None,
    };
    let mut signed_out = open();
    assert_eq!(signed_out.end(EndReason::SignOut), Some(EndReason::SignOut));
    let mut idle = open();
    assert_eq!(
        idle.authorize(Request::Pause, at(HOUR)),
        Err(ended(EndReason::Expired)),
        "a full session's idle timeout binds a pause too"
    );
    let mut lapsed = open();
    assert_eq!(
        lapsed.authorize(Request::Pause, at(12 * HOUR)),
        Err(ended(EndReason::Expired))
    );
    let mut reused = open();
    assert!(
        reused
            .refresh(&secret(1), ProviderAnswer::Granted, &secret(2), at(60))
            .is_ok()
    );
    assert_eq!(
        reused.refresh(&secret(1), ProviderAnswer::Granted, &secret(3), at(61)),
        Err(ended(EndReason::RefreshReuse))
    );
    assert_eq!(signed_out.ended(), Some(EndReason::SignOut));
    assert_refused_after(&mut idle, 1, at(HOUR + 1), EndReason::Expired);
    assert_refused_after(&mut lapsed, 1, at(12 * HOUR + 1), EndReason::Expired);
    assert!(
        SessionRecord::open_reduction_only(signed_in, at(13 * HOUR)).is_ok(),
        "an expiry is no deprovision signal"
    );
    assert_eq!(reused.ended(), Some(EndReason::RefreshReuse));
    let standing = signed_in.saw_deprovision(at(13 * HOUR));
    assert_eq!(
        SessionRecord::open_reduction_only(standing, at(13 * HOUR + 1)),
        Err(SessionRefusal::DeprovisionSeen),
        "the signal reads the subject's standing, not any session"
    );
}

#[test]
#[ignore = "pending E9-1"]
fn an_outage_restores_the_full_session_only_inside_the_idle_timeout() {
    let mut quiet = in_an_outage();
    assert_eq!(
        quiet.authorize(Request::Pause, at(2 * HOUR)),
        Ok(()),
        "no idle limit on pause"
    );
    for over_s in [0, 1] {
        let mut late = active_in_an_outage();
        let grant = late.refresh(
            &secret(1),
            ProviderAnswer::Granted,
            &secret(3),
            at(3_000 + HOUR + over_s),
        );
        assert_eq!(
            grant,
            Err(ended(EndReason::Expired)),
            "{over_s} s past the hour since the pause"
        );
        assert_refused_after(&mut late, 1, at(3_000 + HOUR + over_s), EndReason::Expired);
    }
    let mut active = active_in_an_outage();
    let restored = active.refresh(
        &secret(1),
        ProviderAnswer::Granted,
        &secret(3),
        at(3_000 + HOUR - 1),
    );
    assert_eq!(
        restored,
        Ok(Refreshed::Rotated {
            access_expires_at: at(3_000 + HOUR - 1 + 300)
        })
    );
    assert_eq!(active.reach(), Reach::Full);
    assert_eq!(active.authorize(Request::Other, at(3_000 + HOUR)), Ok(()));
}

/// A session whose refresh met an outage at 300 s, reduced to pause and the kill switch.
fn in_an_outage() -> SessionRecord {
    let mut s = open();
    let outage = s.refresh(&secret(1), ProviderAnswer::Unreachable, &secret(2), at(300));
    assert_eq!(outage, Ok(Refreshed::Outage));
    assert_eq!(s.reach(), Reach::Outage);
    let other = s.authorize(Request::Other, at(301));
    assert_eq!(other, Err(SessionRefusal::RiskReductionOnly));
    s
}

/// [`in_an_outage`] with a pause at 3 000 s, which the idle timer counts from.
fn active_in_an_outage() -> SessionRecord {
    let mut s = in_an_outage();
    assert_eq!(
        s.authorize(Request::Pause, at(3_000)),
        Ok(()),
        "a pause counts as activity"
    );
    s
}

#[test]
#[ignore = "pending E9-1"]
fn a_lapsed_idle_session_is_not_restored_by_a_granted_refresh() {
    for paused_at in [None, Some(2_000)] {
        let mut s = in_an_outage();
        let last = match paused_at {
            Some(t) => {
                assert_eq!(s.authorize(Request::Pause, at(t)), Ok(()));
                t
            }
            None => 0,
        };
        let again = s.refresh(
            &secret(1),
            ProviderAnswer::Unreachable,
            &secret(3),
            at(last + 2 * HOUR),
        );
        assert_eq!(
            again,
            Ok(Refreshed::Outage),
            "only a grant ends an idle outage session"
        );
        let restore = s.refresh(
            &secret(1),
            ProviderAnswer::Granted,
            &secret(4),
            at(last + 2 * HOUR),
        );
        assert_eq!(
            restore,
            Err(ended(EndReason::Expired)),
            "two hours since the last admitted request: {paused_at:?}"
        );
        assert_refused_after(&mut s, 1, at(last + 2 * HOUR + 1), EndReason::Expired);
    }
    let mut edge = in_an_outage();
    let restore = edge.refresh(&secret(1), ProviderAnswer::Granted, &secret(3), at(HOUR));
    assert_eq!(
        restore,
        Err(ended(EndReason::Expired)),
        "the idle timeout lapses at exactly an hour"
    );
    assert_refused_after(&mut edge, 1, at(HOUR), EndReason::Expired);
}

#[test]
#[ignore = "pending E9-1"]
fn a_reduction_only_session_reaches_pause_and_the_kill_switch_for_fifteen_minutes() {
    assert_eq!(REDUCTION_ONLY_LIFETIME_S, 900);
    let standing = SubjectStanding {
        last_sign_in: Some(at(-DAY)),
        last_deprovision: Some(at(-2 * DAY)),
    };
    let mut s = SessionRecord::open_reduction_only(standing, at(0)).unwrap();
    assert_eq!(s.reach(), Reach::ReductionOnly);
    assert!(!s.can_present_step_up(), "§7.3: no step-up through it");
    assert_eq!(
        s.authorize(Request::Other, at(1)),
        Err(SessionRefusal::RiskReductionOnly)
    );
    assert_eq!(s.authorize(Request::Pause, at(899)), Ok(()));
    assert_eq!(s.authorize(Request::KillSwitch, at(899)), Ok(()));
    let refresh = s.refresh(&secret(1), ProviderAnswer::Granted, &secret(2), at(899));
    assert_eq!(refresh, Err(SessionRefusal::NotRefreshable));
    assert_eq!(s.reach(), Reach::ReductionOnly, "nothing widens it");
    assert_eq!(
        s.authorize(Request::KillSwitch, at(900)),
        Err(ended(EndReason::Expired))
    );
    assert_refused_after(&mut s, 1, at(901), EndReason::Expired);
    let never_refused = SubjectStanding {
        last_sign_in: None,
        last_deprovision: None,
    };
    assert!(SessionRecord::open_reduction_only(never_refused, at(0)).is_ok());
    for (sign_in, refusal) in [
        (Some(-DAY), Some(-1)),
        (Some(-DAY), Some(-DAY)),
        (None, Some(-DAY)),
    ] {
        let standing = SubjectStanding {
            last_sign_in: sign_in.map(at),
            last_deprovision: refusal.map(at),
        };
        let refused = SessionRecord::open_reduction_only(standing, at(0));
        assert_eq!(
            refused,
            Err(SessionRefusal::DeprovisionSeen),
            "{sign_in:?} {refusal:?}"
        );
    }
}
