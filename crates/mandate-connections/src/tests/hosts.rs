//! Only `POST /oauth/token` reaches the live host; every trading and account call goes to the
//! paper host (DEC-821 item 3, guardrails a and b).

use crate::ConnectError;
use crate::hosts::{LIVE_TOKEN_URL, LiveTokenRequest, Method, Outbound, PaperRequest, admit};

fn paper(method: Method, path: &str) -> PaperRequest {
    PaperRequest::new(method, path).unwrap_or_else(|e| panic!("{method:?} {path}: {e:?}"))
}

#[test]
fn the_live_token_request_is_post_oauth_token() {
    let request = LiveTokenRequest::token_endpoint();
    assert_eq!(request.method(), Method::Post);
    assert_eq!(request.url(), "https://api.alpaca.markets/oauth/token");
}

#[test]
#[ignore = "pending E10-13"]
fn the_live_host_admits_only_post_oauth_token() {
    assert_eq!(
        admit(Method::Post, LIVE_TOKEN_URL),
        Ok(Outbound::LiveToken(LiveTokenRequest::token_endpoint()))
    );
    let refused_urls = [
        "https://api.alpaca.markets/oauth/token/",
        "https://api.alpaca.markets/oauth/token/.",
        "https://api.alpaca.markets/oauth//token",
        "https://api.alpaca.markets//oauth/token",
        "https://api.alpaca.markets/oauth/%74oken",
        "https://api.alpaca.markets/oauth/token?env=live",
        "https://api.alpaca.markets/oauth/token?x",
        "https://api.alpaca.markets/oauth/token?",
        "https://api.alpaca.markets/oauth/token;x",
        "https://api.alpaca.markets/oauth/token#x",
        "https://api.alpaca.markets:443/oauth/token",
        "https://api.alpaca.markets:8443/oauth/token",
        "https://api.alpaca.markets:/oauth/token",
        "https://api.alpaca.markets./oauth/token",
        "https://API.ALPACA.MARKETS/oauth/token",
        "https://Api.alpaca.markets/oauth/token",
        "https://api.alpaca.markets/OAUTH/TOKEN",
        "HTTPS://api.alpaca.markets/oauth/token",
        "http://api.alpaca.markets/oauth/token",
        "https:\\\\api.alpaca.markets\\oauth\\token",
        "https://api.alpaca.markets\\oauth/token",
        " https://api.alpaca.markets/oauth/token",
        "https://api.alpaca.markets/oauth/token ",
        "https://api.alpaca.markets/oauth/token\n",
        "\nhttps://api.alpaca.markets/oauth/token",
        "https://user@api.alpaca.markets/oauth/token",
        "https://api.alpaca.markets/oauth/../v2/orders",
        "https://api.alpaca.markets/v2/orders",
        "https://api.alpaca.markets/v2/account",
        "https://api.alpaca.markets/v2/account/configurations",
        "https://api.alpaca.markets/v2/wallets/transfers",
    ];
    for url in refused_urls {
        for method in [Method::Get, Method::Post, Method::Patch, Method::Delete] {
            assert_eq!(
                admit(method, url),
                Err(ConnectError::RequestRefused),
                "{method:?} {url:?}"
            );
        }
    }
    for method in [Method::Get, Method::Patch, Method::Delete] {
        assert_eq!(
            admit(method, LIVE_TOKEN_URL),
            Err(ConnectError::RequestRefused),
            "{method:?}"
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
        (Method::Patch, "/v2/orders/abc"),
    ] {
        let request = paper(method, path);
        assert_eq!(
            request.url(),
            Ok(format!("https://paper-api.alpaca.markets{path}"))
        );
        assert_eq!(
            admit(method, &format!("https://paper-api.alpaca.markets{path}")),
            Ok(Outbound::Paper(request.clone())),
            "{method:?} {path}"
        );
        for other in [Method::Get, Method::Post, Method::Patch, Method::Delete] {
            if other != method {
                assert_ne!(paper(other, path), request, "the method is kept");
            }
        }
    }
    for path in [
        "https://api.alpaca.markets/v2/orders",
        "//api.alpaca.markets/v2/orders",
        "v2/orders",
        "/v2",
        "/v2/../oauth/token",
        "/v2/./orders",
        "/v2//orders",
        "/v2/%6Frders",
        "/v2/orders?x=1",
        "/v2/orders#x",
        "/v2/orders\\x",
        "/v2/orders ",
        " /v2/orders",
        "/v2/orders\n",
        "/V2/orders",
        "/oauth/token",
        "",
    ] {
        assert_eq!(
            PaperRequest::new(Method::Post, path),
            Err(ConnectError::RequestRefused),
            "{path:?}"
        );
    }
}

#[test]
#[ignore = "pending E10-13"]
fn other_hosts_and_schemes_are_refused() {
    assert_eq!(
        admit(Method::Post, "https://paper-api.alpaca.markets/v2/orders"),
        Ok(Outbound::Paper(paper(Method::Post, "/v2/orders"))),
        "the control case"
    );
    for url in [
        "https://paper-api.alpaca.markets.evil.example/v2/orders",
        "https://paper-api.alpaca.markets./v2/orders",
        "https://paper-api.alpaca.markets:443/v2/orders",
        "https://PAPER-API.alpaca.markets/v2/orders",
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
            "{url:?}"
        );
    }
}
