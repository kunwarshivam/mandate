//! A cell's token URL: only in `dev` or `ci`, only on a loopback host, never an Alpaca host, so it
//! cannot send a real exchange anywhere but the pinned live URL (DEC-2629 item 3).

use crate::cell::{Refused, parse};

const FIXTURE: &str = "http://127.0.0.1:8899/alpaca/oauth/token";

#[test]
fn only_a_dev_or_ci_tier_admits_a_token_url() {
    for tier in ["dev", "ci"] {
        assert_eq!(
            parse(Some(tier), FIXTURE).map(|url| url.as_str().to_owned()),
            Ok(FIXTURE.to_owned()),
            "{tier}"
        );
    }
    for tier in [
        None,
        Some(""),
        Some("staging"),
        Some("production"),
        Some("prod"),
        Some("DEV"),
    ] {
        assert_eq!(parse(tier, FIXTURE), Err(Refused::Tier), "{tier:?}");
    }
}

#[test]
fn a_loopback_url_with_a_path_and_nothing_else_is_admitted_as_given() {
    let admitted = [
        FIXTURE,
        "http://[::1]:8899/alpaca/oauth/token",
        "http://[::1]/alpaca/oauth/token",
        "http://localhost:8899/alpaca/oauth/token",
        "https://localhost/alpaca/oauth/token",
        "http://127.0.0.1/t",
    ];
    for text in admitted {
        assert_eq!(
            parse(Some("ci"), text).map(|url| url.as_str().to_owned()),
            Ok(text.to_owned()),
            "{text}"
        );
    }
}

/// No Alpaca host is ever admitted, in any spelling, so the override cannot point a real exchange
/// anywhere: the only way to Alpaca is the pinned live URL.
#[test]
fn no_alpaca_host_is_admitted_in_any_tier() {
    let alpaca = [
        "https://api.alpaca.markets/oauth/token",
        "https://paper-api.alpaca.markets/oauth/token",
        "https://alpaca.markets/oauth/token",
        "https://API.ALPACA.MARKETS/oauth/token",
        "https://api.alpaca.markets./oauth/token",
        "https://api.alpaca.markets:443/oauth/token",
        "http://api.alpaca.markets/oauth/token",
        "https://x.y.alpaca.markets/oauth/token",
    ];
    for text in alpaca {
        for tier in ["dev", "ci"] {
            assert_eq!(parse(Some(tier), text), Err(Refused::Url), "{tier} {text}");
        }
    }
}

#[test]
fn any_other_url_is_refused() {
    let refused = [
        "",
        "127.0.0.1:8899/alpaca/oauth/token",
        "ftp://127.0.0.1/alpaca/oauth/token",
        "HTTP://127.0.0.1/alpaca/oauth/token",
        "http://127.0.0.1",
        "http://127.0.0.1:8899",
        "http://user@127.0.0.1/alpaca/oauth/token",
        "http://127.0.0.1/alpaca/oauth/token?x=1",
        "http://127.0.0.1/alpaca/oauth/token?",
        "http://127.0.0.1/alpaca/oauth/token#x",
        "http://127.0.0.2/alpaca/oauth/token",
        "http://10.0.0.1/alpaca/oauth/token",
        "http://evil.example/alpaca/oauth/token",
        "http://localhost.evil.example/alpaca/oauth/token",
        "http://127.0.0.1.nip.io/alpaca/oauth/token",
        "http://[::ffff:127.0.0.1]/alpaca/oauth/token",
        "http://[::1]8899/alpaca/oauth/token",
        "http://[::1]x/alpaca/oauth/token",
        " http://127.0.0.1/alpaca/oauth/token",
        "http://127.0.0.1/alpaca/oauth/token ",
        "http://127.0.0.1/alpaca\\oauth/token",
        "http://127.0.0.1:/alpaca/oauth/token",
        "http://127.0.0.1:99999/alpaca/oauth/token",
        "http://xn--lcalhost-5za/alpaca/oauth/token",
        "http://xn--localhost/alpaca/oauth/token",
        "http://0.0.0.0/alpaca/oauth/token",
        "http://[::]/alpaca/oauth/token",
        "http://169.254.169.254/alpaca/oauth/token",
    ];
    for text in refused {
        assert_eq!(parse(Some("dev"), text), Err(Refused::Url), "{text:?}");
    }
}

#[test]
fn each_refusal_has_its_stable_code() {
    assert_eq!(Refused::Tier.code(), "token_exchange_cell_tier");
    assert_eq!(Refused::Url.code(), "token_exchange_cell_url");
}
