//! The fixture provider's start checks and their order (DEC-1176 item 8): built, then the tier,
//! then any address with a port, then the one public host it answers.

use mandate_stub_issuer::start::{Fixture, Refused, check_fixture};

const BIND: &str = "0.0.0.0:8091";
const HOST: &str = "oauth-fixture:8091";

fn fixture(bind: &str, host: &str) -> Fixture {
    Fixture {
        bind: bind.parse().expect("an address"),
        host: host.to_owned(),
    }
}

#[test]
fn the_public_host_refusal_has_its_decisions_name() {
    assert_eq!(Refused::PublicHost.code(), "stub_issuer_public_host");
}

#[test]
fn in_dev_and_ci_it_listens_on_any_address_and_answers_its_public_host() {
    for tier in ["dev", "ci"] {
        for (bind, host) in [
            (BIND, HOST),
            ("127.0.0.1:8091", "localhost:8091"),
            ("[::]:1", "a.b-c.d9:1"),
            ("10.42.0.7:65535", "oauth-fixture.ws-ws1.svc:65535"),
            ("0.0.0.0:8091", "127.0.0.1:8091"),
        ] {
            assert_eq!(
                check_fixture(true, Some(tier), bind, host),
                Ok(fixture(bind, host)),
                "{tier} {bind} {host}"
            );
        }
    }
}

#[test]
fn a_build_without_debug_assertions_refuses_before_reading_anything() {
    for tier in [Some("dev"), Some("production"), None] {
        for (bind, host) in [(BIND, HOST), ("", ""), ("0.0.0.0:0", "http://x")] {
            assert_eq!(
                check_fixture(false, tier, bind, host),
                Err(Refused::NotBuilt),
                "{tier:?} {bind} {host}"
            );
        }
    }
}

#[test]
fn outside_dev_and_ci_it_refuses_before_its_bind_or_host() {
    for tier in [
        None,
        Some("staging"),
        Some("production"),
        Some(""),
        Some("Dev"),
        Some(" ci"),
    ] {
        for (bind, host) in [(BIND, HOST), ("", "")] {
            assert_eq!(
                check_fixture(true, tier, bind, host),
                Err(Refused::Tier),
                "{tier:?} {bind} {host}"
            );
        }
    }
}

#[test]
fn a_bind_without_a_port_or_that_does_not_print_back_is_refused_before_the_host() {
    for bind in [
        "",
        "0.0.0.0",
        "0.0.0.0:0",
        "0.0.0.0:08091",
        "0.0.0.0:65536",
        "oauth-fixture:8091",
        " 0.0.0.0:8091",
        "[0:0::0]:8091",
    ] {
        for host in [HOST, ""] {
            assert_eq!(
                check_fixture(true, Some("dev"), bind, host),
                Err(Refused::Bind),
                "{bind:?} {host:?}"
            );
        }
    }
}

#[test]
fn a_public_host_that_is_not_a_lowercase_name_and_a_port_is_refused() {
    let long_label = format!("{}:8091", "a".repeat(64));
    let long_name = format!("{}a:8091", "a.".repeat(127));
    for host in [
        "",
        "oauth-fixture",
        ":8091",
        "oauth-fixture:",
        "oauth-fixture:0",
        "oauth-fixture:65536",
        "oauth-fixture:08091",
        "oauth-fixture:+8091",
        "oauth-fixture:8091:8091",
        "OAuth-Fixture:8091",
        "http://oauth-fixture:8091",
        "oauth-fixture:8091/",
        "oauth-fixture:8091/alpaca",
        "user@oauth-fixture:8091",
        "oauth fixture:8091",
        "oauth_fixture:8091",
        "-oauth:8091",
        "oauth-:8091",
        "a..b:8091",
        ".a:8091",
        "a.:8091",
        "[::1]:8091",
        " oauth-fixture:8091",
        "oauth-fixture:8091 ",
        "oauth-fixture\n:8091",
        long_label.as_str(),
        long_name.as_str(),
    ] {
        assert_eq!(
            check_fixture(true, Some("ci"), BIND, host),
            Err(Refused::PublicHost),
            "{host:?}"
        );
    }
}

#[test]
fn a_label_of_63_and_a_name_of_253_are_the_longest_answered() {
    let label = format!("{}:8091", "a".repeat(63));
    let labels = vec!["a".repeat(63); 4].join(".");
    let name = format!("{}:8091", labels.get(..253).expect("long"));
    for host in [label, name] {
        assert_eq!(
            check_fixture(true, Some("dev"), BIND, &host),
            Ok(fixture(BIND, &host)),
            "{host}"
        );
    }
}
