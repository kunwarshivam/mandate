//! The process's start settings in a debug build (DEC-2629 item 3, DEC-2633 item 2). A release
//! build's refusal of any token URL is pinned by `cargo xtask ci exchange-release`, which runs the
//! release binary.

use std::collections::BTreeMap;

use crate::cell::{Refused, parse};
use crate::startup::{Endpoint, Settings, StartRefused, settings};
use mandate_connections::record::ConnectionId;

const CONNECTION: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";
const CELL_URL: &str = "http://127.0.0.1:8091/alpaca/oauth/token";

/// The settings `vars` give, as the process reads its environment.
fn from(vars: &[(&str, &str)]) -> Result<Settings, StartRefused> {
    let vars: BTreeMap<String, String> = vars
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    settings(|name| vars.get(name).cloned())
}

#[test]
#[ignore = "pending E10-13"]
fn without_a_token_url_the_process_sends_to_the_pinned_live_url() {
    for tier in ["dev", "ci", "staging", "production", "unknown"] {
        assert_eq!(
            from(&[
                ("MANDATE_SUBJECT", CONNECTION),
                ("MANDATE_DEPLOYMENT_TIER", tier)
            ]),
            Ok(Settings {
                connection: ConnectionId::new(CONNECTION).unwrap(),
                endpoint: Endpoint::Live,
            }),
            "{tier}"
        );
    }
    assert_eq!(
        from(&[("MANDATE_SUBJECT", CONNECTION)]).map(|s| s.endpoint),
        Ok(Endpoint::Live),
        "no tier at all"
    );
}

#[test]
#[ignore = "pending E10-13"]
fn a_dev_or_ci_cell_sends_to_the_url_the_cell_check_admits() {
    for tier in ["dev", "ci"] {
        assert_eq!(
            from(&[
                ("MANDATE_SUBJECT", CONNECTION),
                ("MANDATE_DEPLOYMENT_TIER", tier),
                ("MANDATE_TOKEN_URL", CELL_URL),
            ])
            .map(|s| s.endpoint),
            Ok(Endpoint::Cell(parse(Some(tier), CELL_URL).unwrap())),
            "{tier}"
        );
    }
}

/// Outside `dev` and `ci` a token URL refuses the start, whatever it names, an empty one
/// included: the variable being set at all is the refusal (DEC-2633 item 2).
#[test]
#[ignore = "pending E10-13"]
fn outside_dev_and_ci_any_token_url_refuses_the_start() {
    for tier in [None, Some("staging"), Some("production"), Some("Dev")] {
        for url in [CELL_URL, "", "https://api.alpaca.markets/oauth/token"] {
            let mut vars = vec![("MANDATE_SUBJECT", CONNECTION), ("MANDATE_TOKEN_URL", url)];
            if let Some(tier) = tier {
                vars.push(("MANDATE_DEPLOYMENT_TIER", tier));
            }
            assert_eq!(
                from(&vars),
                Err(StartRefused::Cell(Refused::Tier)),
                "{tier:?} {url:?}"
            );
        }
    }
}

#[test]
#[ignore = "pending E10-13"]
fn a_token_url_the_cell_check_refuses_refuses_the_start_in_dev() {
    for url in [
        "",
        "https://api.alpaca.markets/oauth/token",
        "http://evil.example/t",
    ] {
        assert_eq!(
            from(&[
                ("MANDATE_SUBJECT", CONNECTION),
                ("MANDATE_DEPLOYMENT_TIER", "dev"),
                ("MANDATE_TOKEN_URL", url),
            ]),
            Err(StartRefused::Cell(Refused::Url)),
            "{url:?}"
        );
    }
}

/// The token URL is judged before the subject, so a refused URL is the refusal named even when
/// the subject is missing too.
#[test]
#[ignore = "pending E10-13"]
fn the_token_url_is_judged_before_the_subject() {
    assert_eq!(
        from(&[("MANDATE_TOKEN_URL", CELL_URL)]),
        Err(StartRefused::Cell(Refused::Tier))
    );
}

#[test]
#[ignore = "pending E10-13"]
fn a_missing_or_malformed_subject_refuses_the_start() {
    for subject in [
        None,
        Some(""),
        Some("conn 1"),
        Some("conn_01JB3K6F0C9R2V7N4M8Q1T5W50\n"),
    ] {
        let mut vars = vec![("MANDATE_DEPLOYMENT_TIER", "dev")];
        if let Some(subject) = subject {
            vars.push(("MANDATE_SUBJECT", subject));
        }
        assert_eq!(from(&vars), Err(StartRefused::Subject), "{subject:?}");
    }
}

#[test]
fn each_start_refusal_has_its_stable_code() {
    assert_eq!(
        StartRefused::TokenUrlInRelease.code(),
        "token_exchange_token_url_in_release"
    );
    assert_eq!(StartRefused::Subject.code(), "token_exchange_subject");
    assert_eq!(
        StartRefused::Cell(Refused::Tier).code(),
        "token_exchange_cell_tier"
    );
    assert_eq!(
        StartRefused::Cell(Refused::Url).code(),
        "token_exchange_cell_url"
    );
}
