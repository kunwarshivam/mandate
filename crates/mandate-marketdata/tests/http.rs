//! Credentials never reach output, and only the market-data host and endpoints are reachable
//! (AGENTS.md rule 7; ADR-0001 ES-23). No test here opens a connection.

use std::fs;
use std::path::Path;

use mandate_marketdata::client::{Transport, TransportError};
use mandate_marketdata::http::{
    AlpacaDataHttp, Credentials, CredentialsError, DATA_HOST, KEY_ID_VAR, SECRET_VAR,
    is_market_data_path,
};

const KEY: &str = "PKSENTINELKEYID00000";
const SECRET: &str = "sentinel-secret-value-never-printed";

#[test]
fn credentials_are_redacted_in_debug_output() {
    let credentials = Credentials::new(KEY.to_owned(), SECRET.to_owned());
    let shown = format!("{credentials:?}");
    assert!(!shown.contains(KEY) && !shown.contains(SECRET), "{shown}");
    let http = AlpacaDataHttp::new(credentials).unwrap();
    let shown = format!("{http:?} {http:#?}");
    assert!(!shown.contains(KEY) && !shown.contains(SECRET));
}

#[test]
fn missing_credentials_name_the_variable_not_a_value() {
    let only_key = |name: &str| (name == KEY_ID_VAR).then(|| KEY.to_owned());
    let err = Credentials::from_lookup(only_key).unwrap_err();
    assert_eq!(
        err,
        CredentialsError::Missing {
            variable: SECRET_VAR
        }
    );
    let text = err.to_string();
    assert!(text.contains(SECRET_VAR) && !text.contains(KEY), "{text}");

    let empty = |_: &str| Some(String::new());
    assert_eq!(
        Credentials::from_lookup(empty).unwrap_err(),
        CredentialsError::Missing {
            variable: KEY_ID_VAR
        }
    );
    let both = |name: &str| Some(if name == KEY_ID_VAR { KEY } else { SECRET }.to_owned());
    assert!(Credentials::from_lookup(both).is_ok());
}

fn rust_sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push((
                path.display().to_string(),
                fs::read_to_string(&path).unwrap(),
            ));
        }
    }
}

#[test]
fn the_data_host_is_the_only_alpaca_host_in_the_source() {
    assert_eq!(DATA_HOST, "https://data.alpaca.markets");
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut sources = Vec::new();
    rust_sources(&crates.join("mandate-marketdata/src"), &mut sources);
    assert!(sources.len() > 5, "the scan must see the crate sources");
    let domain = concat!("alpaca", ".markets");
    let mut hosts = 0;
    for (file, text) in &sources {
        for (at, _) in text.match_indices(domain) {
            let host_start = text[..at]
                .char_indices()
                .rev()
                .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '.' || *c == '-'))
                .map_or(0, |(i, c)| i + c.len_utf8());
            let host = &text[host_start..at + domain.len()];
            assert_eq!(host, "data.alpaca.markets", "{file} names {host}");
            hosts += 1;
        }
    }
    assert_eq!(hosts, 1, "the host is written once, in DATA_HOST");
}

#[test]
fn only_market_data_paths_are_requested() {
    for path in [
        "/v2/stocks/bars?symbols=SPY",
        "/v2/stocks/trades?symbols=SPY",
        "/v1beta3/crypto/us/bars?symbols=BTC%2FUSD",
        "/v1beta3/crypto/us/trades?symbols=BTC%2FUSD",
    ] {
        assert!(is_market_data_path(path), "{path}");
    }
    for path in [
        "/v2/account",
        "/v2/orders",
        "/v2/stocks/bars",
        "/v2/stocks/quotes?symbols=SPY",
        "//api.example.com/v2/stocks/bars?symbols=SPY",
        "@api.example.com/v2/stocks/bars?symbols=SPY",
        "/v2/stocks/bars?symbols=SPY#@example.com",
        "/v2/stocks/bars?symbols=SPY\r\nHost: example.com",
        "/v2/stocks/bars/../../orders?x=1",
        "",
    ] {
        assert!(!is_market_data_path(path), "{path:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_refused_path_sends_nothing() {
    let http = AlpacaDataHttp::new(Credentials::new(KEY.to_owned(), SECRET.to_owned())).unwrap();
    assert_eq!(
        http.get("/v2/orders?status=all").await,
        Err(TransportError::RefusedPath)
    );
}
