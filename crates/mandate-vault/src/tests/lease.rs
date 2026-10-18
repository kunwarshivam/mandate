//! E21-9's renewal rule, against an oracle that replays the same events its own way.

use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;
use proptest::prelude::*;
use secrecy::{ExposeSecret, SecretSlice};

use crate::SourceError;
use crate::lease::{LEASE_SECS, Lease, LeaseKeeper, RENEW_SECS, lease_times};

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

fn lease(connection: &str, value: u8, now: i64) -> Lease {
    let (renew_at, expires_at) = lease_times(at(now), None).unwrap();
    Lease {
        connection: ConnectionId::new(connection).unwrap(),
        secret: SecretSlice::from(vec![value]),
        expires_at,
        renew_at,
    }
}

#[test]
fn a_lease_renews_hourly_and_lasts_a_day_capped_by_the_backend() {
    assert_eq!(
        lease_times(at(1_000), None),
        Some((at(1_000 + RENEW_SECS), at(1_000 + LEASE_SECS)))
    );
    assert_eq!(
        lease_times(at(1_000), Some(at(5_000))),
        Some((at(1_000 + RENEW_SECS), at(5_000)))
    );
    assert_eq!(
        lease_times(at(1_000), Some(at(2_000))),
        Some((at(2_000), at(2_000))),
        "a backend expiry before the renewal brings the renewal forward"
    );
    assert_eq!(lease_times(at(253_402_300_799), None), None);
}

#[test]
fn an_outage_keeps_the_lease_until_it_expires() {
    let mut keeper = LeaseKeeper::new(lease("c1", 7, 0));
    assert!(!keeper.renewal_due(at(RENEW_SECS - 1)));
    assert!(keeper.renewal_due(at(RENEW_SECS)));
    for outage in [SourceError::Unavailable, SourceError::Sealed] {
        keeper.apply(Err(outage), at(RENEW_SECS));
        assert_eq!(
            keeper.secret(at(RENEW_SECS)).map(|s| s.expose_secret()[0]),
            Some(7)
        );
    }
    assert!(keeper.secret(at(LEASE_SECS - 1)).is_some());
    assert!(keeper.secret(at(LEASE_SECS)).is_none(), "dropped at expiry");
    assert!(keeper.lease().is_none());
    assert!(!keeper.renewal_due(at(LEASE_SECS)));
    assert!(
        keeper.secret(at(0)).is_none(),
        "a dropped lease never returns"
    );
}

#[test]
fn a_refusal_drops_the_lease_at_once() {
    for refusal in [
        SourceError::Denied,
        SourceError::Missing,
        SourceError::Integrity,
        SourceError::Malformed,
        SourceError::GrantInvalid,
    ] {
        let mut keeper = LeaseKeeper::new(lease("c1", 7, 0));
        keeper.apply(Err(refusal.clone()), at(10));
        assert!(keeper.secret(at(10)).is_none(), "{refusal:?}");
    }
}

#[test]
fn a_renewal_replaces_the_lease_and_one_for_another_connection_drops_it() {
    let mut keeper = LeaseKeeper::new(lease("c1", 7, 0));
    keeper.apply(Ok(lease("c1", 8, RENEW_SECS)), at(RENEW_SECS));
    assert_eq!(
        keeper
            .secret(at(LEASE_SECS + 1))
            .map(|s| s.expose_secret()[0]),
        Some(8)
    );
    keeper.apply(Ok(lease("c2", 9, RENEW_SECS)), at(RENEW_SECS));
    assert!(keeper.secret(at(RENEW_SECS)).is_none());
}

#[derive(Debug, Clone)]
enum Event {
    Tick(i64),
    Renewed(u8),
    Outage,
    Refused,
}

fn events() -> impl Strategy<Value = Vec<Event>> {
    prop::collection::vec(
        prop_oneof![
            (1_i64..20_000).prop_map(Event::Tick),
            any::<u8>().prop_map(Event::Renewed),
            Just(Event::Outage),
            Just(Event::Refused),
        ],
        0..60,
    )
}

proptest! {
    /// The oracle keeps only the instant of the last successful renewal and whether a refusal
    /// came after it: the secret is served iff no refusal followed and less than a day passed.
    #[test]
    fn the_keeper_serves_exactly_what_the_rule_allows(events in events()) {
        let mut now = 0_i64;
        let mut keeper = LeaseKeeper::new(lease("c1", 0, 0));
        let mut last_ok = 0_i64;
        let mut last_value = 0_u8;
        let mut refused = false;
        for event in events {
            match event {
                Event::Tick(d) => now += d,
                Event::Renewed(v) => {
                    keeper.apply(Ok(lease("c1", v, now)), at(now));
                    if !refused && now < last_ok + LEASE_SECS {
                        last_ok = now;
                        last_value = v;
                    }
                }
                Event::Outage => keeper.apply(Err(SourceError::Unavailable), at(now)),
                Event::Refused => {
                    keeper.apply(Err(SourceError::Denied), at(now));
                    refused = true;
                }
            }
            let live = !refused && now < last_ok + LEASE_SECS;
            if !live {
                refused = true;
            }
            let got = keeper.secret(at(now)).map(|s| s.expose_secret()[0]);
            prop_assert_eq!(got, live.then_some(last_value), "at {}", now);
        }
    }
}
