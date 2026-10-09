//! Only `POST /oauth/token` reaches the live host; every trading and account call goes to the
//! paper host (DEC-821 items 2 and 4).

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
        "https://api.alpaca.xn--markts-6of/oauth/token",
        "https://api.alpaca.mark\u{0435}ts/oauth/token",
        "https://xn--pi-6kc.alpaca.markets/oauth/token",
        "https://\u{0430}pi.alpaca.markets/oauth/token",
        "https://1.2.3.4/oauth/token",
        "https://[::1]/oauth/token",
        "https://16909060/oauth/token",
        "https://0x01020304/oauth/token",
        "https://0x1.0x2.0x3.0x4/oauth/token",
        "https://api.alpaca.markets@evil.example/oauth/token",
        "https://evil.example@api.alpaca.markets/oauth/token",
        "https://api.alpaca.markets/oauth/token\0",
        "\0https://api.alpaca.markets/oauth/token",
        "https://api.alpaca.markets/oauth/token\u{00A0}",
        "\u{3000}https://api.alpaca.markets/oauth/token",
        "https://api.alpaca.markets/oauth/to\u{200B}ken",
        "https://api.alpaca.markets\u{FF0F}oauth/token",
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
        "/v2/orders\0",
        "\u{00A0}/v2/orders",
        "/v2/orders\u{2028}",
        "/v2/ord\u{0435}rs",
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
fn other_hosts_and_schemes_are_refused() {
    let refused_urls = [
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
        "https://paper-api.alpaca.markets/v2/orders?x=1",
        "https://paper-api.alpaca.markets/v2/orders?",
        "https://paper-api.alpaca.markets/v2/orders#x",
        "https://paper-api.alpaca.markets/v2/./orders",
        "https://paper-api.alpaca.markets/v2/../v2/orders",
        "https://paper-api.alpaca.markets/v2/orders/..",
        "https://paper-api.alpaca.markets/v2/../../oauth/token",
        "HTTPS://paper-api.alpaca.markets/v2/orders",
        "Https://paper-api.alpaca.markets/v2/orders",
        "https://paper-api.alpaca.markets:8443/v2/orders",
        "https://paper-api.alpaca.markets:80/v2/orders",
        "https://paper-api.alpaca.markets:/v2/orders",
        "https://paper-api.alpaca.markets",
        "https://paper-api.alpaca.markets/",
        "https://paper-api.alpaca.xn--markts-6of/v2/orders",
        "https://paper-api.alpaca.mark\u{0435}ts/v2/orders",
        "https://1.2.3.4/v2/orders",
        "https://[::1]/v2/orders",
        "https://16909060/v2/orders",
        "https://0x01020304/v2/orders",
        "https://paper-api.alpaca.markets@evil.example/v2/orders",
        "https://evil.example@paper-api.alpaca.markets/v2/orders",
        "https://api.alpaca.markets@evil.example/oauth/token",
        "https://evil.example@api.alpaca.markets/oauth/token",
        "https://paper-api.alpaca.markets/v2/orders\0",
        "https://paper-api.alpaca.markets/v2/orders\u{3000}",
        "\u{FEFF}https://paper-api.alpaca.markets/v2/orders",
    ];
    for method in [Method::Get, Method::Post, Method::Patch, Method::Delete] {
        assert_eq!(
            admit(method, "https://paper-api.alpaca.markets/v2/orders"),
            Ok(Outbound::Paper(paper(method, "/v2/orders"))),
            "the control case, {method:?}"
        );
        for url in refused_urls {
            assert_eq!(
                admit(method, url),
                Err(ConnectError::RequestRefused),
                "{method:?} {url:?}"
            );
        }
    }
}
