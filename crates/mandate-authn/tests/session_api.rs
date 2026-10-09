//! What the session API prints and journals (identity spec §12.1, ID-9; backlog E9-1, slice A2):
//! a refresh secret's `Debug`, each `SessionRevoked` reason code, and a record that keeps only a
//! digest of its refresh token; and that a clock gone backwards never refuses pause or the kill
//! switch (DEC-652 item 5, ID-10, rule 13).

use mandate_authn::{
    EndReason, OrgKind, ProviderAnswer, Reach, RefreshSecret, Refreshed, Request, SessionLimits,
    SessionPolicy, SessionRecord, SessionRefusal, SubjectStanding, UtcNanos,
};

fn at(offset_s: i64) -> UtcNanos {
    UtcNanos::from_parts(1_800_000_000 + offset_s, 0).unwrap()
}

#[test]
fn a_session_keeps_only_a_digest_of_its_refresh_token() {
    let limits = SessionLimits::resolve(OrgKind::Business, SessionPolicy::default()).unwrap();
    let s = SessionRecord::open(limits, &RefreshSecret([1; 32]), at(0)).unwrap();
    let mut sha256_of_token = [0u8; 32];
    sha256_of_token.copy_from_slice(ring::digest::digest(&ring::digest::SHA256, &[1; 32]).as_ref());
    assert_eq!(s.refresh_digest(), Some(sha256_of_token));
    assert_ne!(s.refresh_digest(), Some([1; 32]));
    assert_eq!(s.rotated_digests(), Vec::<[u8; 32]>::new());
    let shown = format!("{s:?}");
    assert!(!shown.contains(&format!("{:?}", [1u8; 32])), "{shown}");
    assert!(!shown.contains(&"01".repeat(8)), "{shown}");
}

/// Where §12.1 lists each reason; the match fails to compile when a variant is added unlisted.
fn spec_position(reason: EndReason) -> usize {
    match reason {
        EndReason::SignOut => 0,
        EndReason::Deactivated => 1,
        EndReason::Deprovisioned => 2,
        EndReason::RefreshReuse => 3,
        EndReason::RefreshFailed => 4,
        EndReason::Expired => 5,
        EndReason::Admin => 6,
    }
}

#[test]
fn a_refresh_secret_prints_nothing_and_each_end_reason_has_its_spec_code() {
    assert_eq!(format!("{:?}", RefreshSecret([7; 32])), "RefreshSecret(..)");
    let section_12_1 = [
        "sign_out",
        "deactivated",
        "deprovisioned",
        "refresh_reuse",
        "refresh_failed",
        "expired",
        "admin",
    ];
    let reasons = [
        EndReason::SignOut,
        EndReason::Deactivated,
        EndReason::Deprovisioned,
        EndReason::RefreshReuse,
        EndReason::RefreshFailed,
        EndReason::Expired,
        EndReason::Admin,
    ];
    assert_eq!(reasons.map(EndReason::code), section_12_1);
    assert_eq!(reasons.map(spec_position), [0, 1, 2, 3, 4, 5, 6]);
}

#[test]
fn a_backwards_clock_never_refuses_pause_or_the_kill_switch() {
    let limits = SessionLimits::resolve(OrgKind::Business, SessionPolicy::default()).unwrap();
    let reductions = [Request::Pause, Request::KillSwitch];

    let mut full = SessionRecord::open(limits, &RefreshSecret([1; 32]), at(0)).unwrap();
    assert_eq!(full.authorize(Request::Other, at(100)), Ok(()));
    for request in reductions {
        assert_eq!(
            full.authorize(request, at(99)),
            Ok(()),
            "{request:?} behind"
        );
        assert_eq!(
            full.authorize(request, at(-1)),
            Ok(()),
            "{request:?} before opening"
        );
    }
    assert_eq!(
        full.authorize(Request::Other, at(99)),
        Err(SessionRefusal::ClockBehind),
        "the clamped admissions moved no timer back"
    );
    assert_eq!(full.reach(), Reach::Full);
    assert_eq!(full.ended(), None, "a backwards clock ends nothing");

    let mut outage = SessionRecord::open(limits, &RefreshSecret([2; 32]), at(0)).unwrap();
    assert_eq!(outage.authorize(Request::Other, at(100)), Ok(()));
    let cut = outage.refresh(
        &RefreshSecret([2; 32]),
        ProviderAnswer::Unreachable,
        &RefreshSecret([3; 32]),
        at(200),
    );
    assert_eq!(cut, Ok(Refreshed::Outage));
    assert_eq!(outage.authorize(Request::Pause, at(250)), Ok(()));
    for request in reductions {
        assert_eq!(
            outage.authorize(request, at(150)),
            Ok(()),
            "{request:?} behind"
        );
        assert_eq!(
            outage.authorize(request, at(-1)),
            Ok(()),
            "{request:?} before opening"
        );
    }
    assert_eq!(
        outage.authorize(Request::Other, at(150)),
        Err(SessionRefusal::ClockBehind),
        "a backwards clock is refused before the reach"
    );
    assert_eq!(outage.reach(), Reach::Outage);

    let standing = SubjectStanding {
        last_sign_in: Some(at(-1_000)),
        last_deprovision: None,
    };
    let mut route_2 = SessionRecord::open_reduction_only(standing, at(0)).unwrap();
    assert_eq!(route_2.authorize(Request::KillSwitch, at(10)), Ok(()));
    for request in reductions {
        assert_eq!(
            route_2.authorize(request, at(5)),
            Ok(()),
            "{request:?} behind"
        );
    }
    assert_eq!(
        route_2.authorize(Request::Other, at(5)),
        Err(SessionRefusal::ClockBehind),
        "a backwards clock is refused before the reach"
    );

    assert_eq!(full.end(EndReason::SignOut), Some(EndReason::SignOut));
    assert_eq!(
        full.authorize(Request::KillSwitch, at(-1)),
        Err(SessionRefusal::Ended {
            reason: EndReason::SignOut
        }),
        "the clamp revives no ended session"
    );
}
