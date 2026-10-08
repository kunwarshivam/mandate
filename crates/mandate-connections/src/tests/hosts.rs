//! Only `POST /oauth/token` reaches the live host; every trading and account call goes to the
//! paper host (DEC-821 item 3, guardrails a and b).

use crate::ConnectError;
use crate::hosts::{LIVE_TOKEN_URL, LiveTokenRequest, Method, Outbound, PaperRequest, admit};

#[test]
#[ignore = "pending E10-13"]
fn the_live_host_admits_only_post_oauth_token() {
    assert_eq!(
        admit(Method::Post, LIVE_TOKEN_URL),
        Ok(Outbound::LiveToken(LiveTokenRequest::token_endpoint()))
    );
    let refused = [
        (Method::Get, LIVE_TOKEN_URL),
        (Method::Patch, LIVE_TOKEN_URL),
        (Method::Delete, LIVE_TOKEN_URL),
        (Method::Post, "https://api.alpaca.markets/oauth/token/"),
        (
            Method::Post,
            "https://api.alpaca.markets/oauth/token?env=live",
        ),
        (Method::Post, "https://api.alpaca.markets/oauth/token#x"),
        (Method::Post, "https://api.alpaca.markets:443/oauth/token"),
        (Method::Post, "https://API.ALPACA.MARKETS/oauth/token"),
        (Method::Post, "http://api.alpaca.markets/oauth/token"),
        (Method::Post, "https://user@api.alpaca.markets/oauth/token"),
        (
            Method::Post,
            "https://api.alpaca.markets/oauth/../v2/orders",
        ),
        (Method::Post, "https://api.alpaca.markets/v2/orders"),
        (Method::Get, "https://api.alpaca.markets/v2/account"),
        (
            Method::Get,
            "https://api.alpaca.markets/v2/account/configurations",
        ),
        (Method::Delete, "https://api.alpaca.markets/v2/orders"),
        (
            Method::Post,
            "https://api.alpaca.markets/v2/wallets/transfers",
        ),
    ];
    for (method, url) in refused {
        assert_eq!(
            admit(method, url),
            Err(ConnectError::RequestRefused),
            "{method:?} {url}"
        );
    }
}

#[test]
#[ignore = "pending E10-13"]
fn trading_and_account_calls_go_only_to_the_paper_host() {
    for (method, path) in [
        (Method::Post, "/v2/orders"),
        (Method::Get, "/v2/account"),
        (Method::Delete, "/v2/orders/abc"),
    ] {
        let request = PaperRequest::new(method, path).unwrap();
        assert_eq!(
            request.url(),
            Ok(format!("https://paper-api.alpaca.markets{path}"))
        );
        assert_eq!(
            admit(method, &format!("https://paper-api.alpaca.markets{path}")),
            Ok(Outbound::Paper(request))
        );
    }
    for path in [
        "https://api.alpaca.markets/v2/orders",
        "//api.alpaca.markets/v2/orders",
        "v2/orders",
        "/v2/../oauth/token",
        "/oauth/token",
        "",
    ] {
        assert_eq!(
            PaperRequest::new(Method::Post, path),
            Err(ConnectError::RequestRefused),
            "{path}"
        );
    }
}

#[test]
#[ignore = "pending E10-13"]
fn other_hosts_and_schemes_are_refused() {
    for url in [
        "https://paper-api.alpaca.markets.evil.example/v2/orders",
        "https://paper-api.alpaca.markets@api.alpaca.markets/v2/orders",
        "https://evil.example/oauth/token",
        "http://paper-api.alpaca.markets/v2/orders",
        "https://data.alpaca.markets/v2/stocks/bars",
        "https://app.alpaca.markets/oauth/authorize",
        "paper-api.alpaca.markets/v2/orders",
    ] {
        assert_eq!(
            admit(Method::Post, url),
            Err(ConnectError::RequestRefused),
            "{url}"
        );
    }
}
