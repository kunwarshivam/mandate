//! The start checks and their order (DEC-2100 items 2, 3 and 5; invariants I1, I2 and I4).

use mandate_stub_issuer::start::{Refused, check};

const BIND: &str = "127.0.0.1:8899";

#[test]
fn the_codes_are_the_decisions_names() {
    assert_eq!(Refused::NotBuilt.code(), "stub_issuer_not_built");
    assert_eq!(Refused::Tier.code(), "stub_issuer_tier");
    assert_eq!(Refused::Bind.code(), "stub_issuer_bind");
    assert_eq!(Refused::Alpaca.code(), "stub_issuer_alpaca");
}

#[test]
fn only_dev_and_ci_start_it() {
    for tier in ["dev", "ci"] {
        assert_eq!(
            check(true, Some(tier), BIND),
            Ok(BIND.parse().expect("an address"))
        );
    }
    for tier in [
        None,
        Some("staging"),
        Some("production"),
        Some(""),
        Some("Dev"),
        Some("CI"),
        Some(" dev"),
        Some("dev "),
    ] {
        assert_eq!(check(true, tier, BIND), Err(Refused::Tier), "{tier:?}");
    }
}

#[test]
fn a_build_without_debug_assertions_refuses_before_reading_anything() {
    for tier in [Some("dev"), Some("production"), None] {
        for bind in [BIND, "0.0.0.0:8899", ""] {
            assert_eq!(
                check(false, tier, bind),
                Err(Refused::NotBuilt),
                "{tier:?} {bind}"
            );
        }
    }
}

#[test]
fn the_tier_is_checked_before_the_bind() {
    assert_eq!(
        check(true, Some("production"), "0.0.0.0:8899"),
        Err(Refused::Tier)
    );
    assert_eq!(check(true, None, ""), Err(Refused::Tier));
}

#[test]
fn only_a_loopback_ip_literal_with_a_port_binds() {
    for bind in ["127.0.0.1:8899", "[::1]:8899", "127.0.0.1:1", "[::1]:65535"] {
        assert_eq!(
            check(true, Some("ci"), bind),
            Ok(bind.parse().expect("an address"))
        );
    }
    for bind in [
        "0.0.0.0:8899",
        "[::]:8899",
        "10.0.0.1:8899",
        "127.0.0.2:8899",
        "[::ffff:127.0.0.1]:8899",
        "localhost:8899",
        "127.0.0.1:0",
        "[::1]:0",
        "127.0.0.1:65536",
        "127.0.0.1",
        "8899",
        "",
        " 127.0.0.1:8899",
    ] {
        assert_eq!(
            check(true, Some("dev"), bind),
            Err(Refused::Bind),
            "{bind:?}"
        );
    }
}
