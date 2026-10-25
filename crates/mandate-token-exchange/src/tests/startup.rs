//! The process's start settings in a debug build (DEC-2629 item 3, DEC-2633 item 2). A release
//! build's refusal of any token URL is pinned by `cargo xtask ci exchange-release`, which runs the
//! release binary.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

use crate::cell::{Refused, parse};
use crate::startup::{
    Endpoint, Settings, StartRefused, request_deadline, settings, subject_account,
};
use mandate_connections::record::ConnectionId;
use mandate_connections::start::{ClientId, OAuthClient, RedirectUri, STATE_LIFETIME_SECS};
use mandate_time::UtcNanos;

const CONNECTION: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";
const CELL_URL: &str = "http://127.0.0.1:8091/alpaca/oauth/token";
const CLIENT: &str = "PKTESTCLIENT01";
/// [`CONNECTION`]'s ServiceAccount, its bytes in lowercase hex, computed by hand.
const ACCOUNT: &str = "x636f6e6e5f30314a42334b3646304339523256374e344d3851315435573530";
const REDIRECT: &str = "https://api-staging.owlhead.ai/v1/oauth/alpaca/callback";

/// What every start that passes its checks carries beside the subject and the tier.
const STARTED: [(&str, &str); 4] = [
    ("MANDATE_PROCESS", "token_exchange"),
    ("MANDATE_SUBJECT_ACCOUNT", ACCOUNT),
    ("MANDATE_ALPACA_CLIENT_ID", CLIENT),
    ("MANDATE_ALPACA_REDIRECT_URI", REDIRECT),
];

fn client() -> OAuthClient {
    OAuthClient {
        client_id: ClientId(CLIENT.to_owned()),
        redirect_uri: RedirectUri::parse(REDIRECT).unwrap(),
    }
}

/// `vars` with [`STARTED`] added, each unless `vars` names it.
fn started(vars: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut all = vars.to_vec();
    for (name, value) in STARTED {
        if !vars.iter().any(|(given, _)| *given == name) {
            all.push((name, value));
        }
    }
    all
}

/// The settings `vars` give, with [`STARTED`] added where `vars` names none of it, as the
/// process reads its environment.
fn from(vars: &[(&'static str, &'static str)]) -> Result<Settings, StartRefused> {
    from_omitting(vars, &[])
}

/// [`from`], with each variable `omitted` names left out entirely.
fn from_omitting(
    vars: &[(&'static str, &'static str)],
    omitted: &[&str],
) -> Result<Settings, StartRefused> {
    from_raw(
        &started(vars)
            .iter()
            .filter(|(name, _)| !omitted.contains(name))
            .map(|(name, value)| (*name, OsString::from(value)))
            .collect::<Vec<_>>(),
    )
}

/// The settings raw `vars` give, values that need not be UTF-8.
fn from_raw(vars: &[(&str, OsString)]) -> Result<Settings, StartRefused> {
    let vars: BTreeMap<String, OsString> = vars
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect();
    settings(|name| vars.get(name).cloned())
}

/// A token URL that is not UTF-8.
fn not_utf8() -> OsString {
    OsString::from_vec(b"http://127.0.0.1:8091/\xff".to_vec())
}

#[test]
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
                client: client(),
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

/// A token URL that is not UTF-8 is still set, and refuses the start in every tier (DEC-2633
/// item 2): outside dev and ci by the tier, in them by the URL.
#[test]
fn a_token_url_that_is_not_utf8_refuses_the_start() {
    for (tier, refused) in [
        ("production", Refused::Tier),
        ("staging", Refused::Tier),
        ("dev", Refused::Url),
        ("ci", Refused::Url),
    ] {
        assert_eq!(
            from_raw(&[
                ("MANDATE_SUBJECT", OsString::from(CONNECTION)),
                ("MANDATE_DEPLOYMENT_TIER", OsString::from(tier)),
                ("MANDATE_TOKEN_URL", not_utf8()),
            ]),
            Err(StartRefused::Cell(refused)),
            "{tier}"
        );
    }
}

/// The token URL is judged before everything else, so a refused URL is the refusal named even
/// when the process, the subject and the client are missing too: a release build refuses it
/// before it reads anything else.
#[test]
fn the_token_url_is_judged_before_everything_else() {
    assert_eq!(
        from_raw(&[("MANDATE_TOKEN_URL", OsString::from(CELL_URL))]),
        Err(StartRefused::Cell(Refused::Tier))
    );
}

/// Only the process the deployment manager starts as `token_exchange` runs (DEC-1175 item 1):
/// any other value, a missing one, or one that is not UTF-8 refuses the start, before the
/// subject is read.
#[test]
fn only_the_token_exchange_process_runs() {
    for process in [
        "",
        "executor",
        "runtime",
        "token-exchange",
        "TOKEN_EXCHANGE",
        "token_exchange ",
    ] {
        assert_eq!(
            from(&[("MANDATE_PROCESS", process)]),
            Err(StartRefused::Process),
            "{process:?}"
        );
    }
    assert_eq!(
        from_omitting(&[], &["MANDATE_PROCESS"]),
        Err(StartRefused::Process)
    );
    assert_eq!(
        from_raw(&[(
            "MANDATE_PROCESS",
            OsString::from_vec(b"token_exchange\xff".to_vec())
        )]),
        Err(StartRefused::Process)
    );
    assert_eq!(
        from_omitting(&[("MANDATE_PROCESS", "executor")], &["MANDATE_SUBJECT"]),
        Err(StartRefused::Process),
        "the process is judged before the subject"
    );
}

/// The client id must be 1 to 128 of `A-Z a-z 0-9 . _ -`, as the chart's schema admits.
#[test]
fn the_client_id_is_required_and_strict() {
    let longest = "a".repeat(128);
    let too_long = "a".repeat(129);
    for refused in ["", "has space", "a/b", "é", too_long.as_str()] {
        let leaked: &'static str = Box::leak(refused.to_owned().into_boxed_str());
        assert_eq!(
            from(&[
                ("MANDATE_SUBJECT", CONNECTION),
                ("MANDATE_ALPACA_CLIENT_ID", leaked)
            ]),
            Err(StartRefused::ClientId),
            "{refused:?}"
        );
    }
    assert_eq!(
        from_omitting(
            &[("MANDATE_SUBJECT", CONNECTION)],
            &["MANDATE_ALPACA_CLIENT_ID"]
        ),
        Err(StartRefused::ClientId)
    );
    let leaked: &'static str = Box::leak(longest.clone().into_boxed_str());
    assert_eq!(
        from(&[
            ("MANDATE_SUBJECT", CONNECTION),
            ("MANDATE_ALPACA_CLIENT_ID", leaked)
        ])
        .map(|s| s.client.client_id),
        Ok(ClientId(longest))
    );
    assert_eq!(
        from(&[
            ("MANDATE_SUBJECT", CONNECTION),
            ("MANDATE_ALPACA_CLIENT_ID", "a.b_c-D9")
        ])
        .map(|s| s.client.client_id),
        Ok(ClientId("a.b_c-D9".to_owned()))
    );
}

/// The redirect URI must be one `RedirectUri::parse` admits: https on a host, or loopback http,
/// with exactly the callback's path.
#[test]
fn the_redirect_uri_is_required_and_parsed() {
    for refused in [
        "",
        "http://evil.example/v1/oauth/alpaca/callback",
        "https://api.example/elsewhere",
        "https://api.example/v1/oauth/alpaca/callback?x=1",
    ] {
        assert_eq!(
            from(&[
                ("MANDATE_SUBJECT", CONNECTION),
                ("MANDATE_ALPACA_REDIRECT_URI", refused)
            ]),
            Err(StartRefused::RedirectUri),
            "{refused:?}"
        );
    }
    assert_eq!(
        from_omitting(
            &[("MANDATE_SUBJECT", CONNECTION)],
            &["MANDATE_ALPACA_REDIRECT_URI"]
        ),
        Err(StartRefused::RedirectUri)
    );
    let loopback = "http://127.0.0.1:8080/v1/oauth/alpaca/callback";
    assert_eq!(
        from(&[
            ("MANDATE_SUBJECT", CONNECTION),
            ("MANDATE_ALPACA_REDIRECT_URI", loopback)
        ])
        .map(|s| s.client.redirect_uri),
        Ok(RedirectUri::parse(loopback).unwrap())
    );
}

/// The pod must run as the connection's own ServiceAccount, `x` and the lowercase hex of the
/// subject's bytes, which the manager's admission policy pins (DEC-1177): another connection's,
/// an uppercase spelling, or none refuses the start.
#[test]
fn the_subject_account_must_be_the_connection_s_own() {
    let other = "x636f6e6e5f30314a42334b3646304339523256374e344d3851315435573531";
    let upper = "x636F6E6E5F30314A42334B3646304339523256374E344D3851315435573530";
    for refused in [
        other,
        upper,
        "",
        "636f6e6e5f30314a42334b3646304339523256374e344d3851315435573530",
    ] {
        assert_eq!(
            from(&[
                ("MANDATE_SUBJECT", CONNECTION),
                ("MANDATE_SUBJECT_ACCOUNT", refused)
            ]),
            Err(StartRefused::SubjectAccount),
            "{refused:?}"
        );
    }
    assert_eq!(
        from_omitting(
            &[("MANDATE_SUBJECT", CONNECTION)],
            &["MANDATE_SUBJECT_ACCOUNT"]
        ),
        Err(StartRefused::SubjectAccount)
    );
    assert_eq!(
        from_omitting(
            &[("MANDATE_SUBJECT", CONNECTION)],
            &["MANDATE_SUBJECT_ACCOUNT", "MANDATE_ALPACA_CLIENT_ID"]
        ),
        Err(StartRefused::SubjectAccount),
        "the account is judged before the client id"
    );
    assert_eq!(
        subject_account(&ConnectionId::new(CONNECTION).unwrap()),
        ACCOUNT
    );
}

/// The checks run in their stated order: the subject before the client id, the client id before
/// the redirect URI.
#[test]
fn the_later_checks_run_in_order() {
    assert_eq!(
        from_omitting(&[], &["MANDATE_SUBJECT", "MANDATE_ALPACA_CLIENT_ID"]),
        Err(StartRefused::Subject)
    );
    assert_eq!(
        from_omitting(
            &[("MANDATE_SUBJECT", CONNECTION)],
            &["MANDATE_ALPACA_CLIENT_ID", "MANDATE_ALPACA_REDIRECT_URI"]
        ),
        Err(StartRefused::ClientId)
    );
}

#[test]
fn a_missing_or_malformed_subject_refuses_the_start() {
    for subject in [
        None,
        Some(""),
        Some("conn 1"),
        Some("conn_01JB3K6F0C9R2V7N4M8Q1T5W50\n"),
    ] {
        let vars = [
            ("MANDATE_DEPLOYMENT_TIER", "dev"),
            ("MANDATE_SUBJECT", subject.unwrap_or("")),
        ];
        let refused = if subject.is_some() {
            from(&vars)
        } else {
            from_omitting(&vars, &["MANDATE_SUBJECT"])
        };
        assert_eq!(refused, Err(StartRefused::Subject), "{subject:?}");
    }
}

#[test]
fn each_start_refusal_has_its_stable_code() {
    assert_eq!(
        StartRefused::TokenUrlInRelease.code(),
        "token_exchange_token_url_in_release"
    );
    assert_eq!(StartRefused::Subject.code(), "token_exchange_subject");
    assert_eq!(StartRefused::Process.code(), "token_exchange_process");
    assert_eq!(
        StartRefused::SubjectAccount.code(),
        "token_exchange_subject_account"
    );
    assert_eq!(StartRefused::ClientId.code(), "token_exchange_client_id");
    assert_eq!(
        StartRefused::RedirectUri.code(),
        "token_exchange_redirect_uri"
    );
    assert_eq!(
        StartRefused::Cell(Refused::Tier).code(),
        "token_exchange_cell_tier"
    );
    assert_eq!(
        StartRefused::Cell(Refused::Url).code(),
        "token_exchange_cell_url"
    );
}

/// The pod's start in the deadline tests, a whole second.
const POD_START: i64 = 1_790_000_000;

fn instant(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

/// The deadline as the manager renders it: the journal's timestamp form.
fn rendered(secs: i64) -> OsString {
    OsString::from(instant(secs).to_string())
}

/// DEC-2363 item 2: the request's deadline is read exactly as the manager renders it, and a pod
/// restarted later in the same request reads the same deadline, never a fresh lifetime.
#[test]
#[ignore = "pending E10-13"]
fn the_request_s_deadline_is_read_as_rendered_and_a_restart_gets_no_more() {
    let deadline = POD_START + 420;
    let value = rendered(deadline);
    assert_eq!(
        request_deadline(Some(&value), instant(POD_START)),
        Ok(instant(deadline))
    );
    assert_eq!(
        request_deadline(Some(&value), instant(POD_START + 300)),
        Ok(instant(deadline)),
        "a restart 300 s later waits until the same deadline"
    );
}

/// DEC-2363 item 2: no deadline, one in any other form, and one already reached each refuse the
/// start, the last also for a pod whose clock runs ahead of the request's.
#[test]
#[ignore = "pending E10-13"]
fn a_missing_malformed_or_reached_deadline_refuses_the_start() {
    let now = instant(POD_START);
    assert_eq!(
        request_deadline(None, now),
        Err(StartRefused::RequestDeadlineMissing)
    );
    for malformed in [
        OsString::from(""),
        OsString::from("2026-10-25T20:00:00Z"),
        OsString::from("2026-10-25T20:00:00.000000000+00:00"),
        OsString::from(" 2026-10-25T20:00:00.000000000Z"),
        OsString::from("1790000600"),
        not_utf8(),
    ] {
        assert_eq!(
            request_deadline(Some(&malformed), now),
            Err(StartRefused::RequestDeadlineMalformed),
            "{malformed:?}"
        );
    }
    for reached in [POD_START, POD_START - 1, POD_START - 600] {
        assert_eq!(
            request_deadline(Some(&rendered(reached)), now),
            Err(StartRefused::RequestExpired),
            "{reached}"
        );
    }
    let ahead_of_the_request = instant(POD_START + 30);
    assert_eq!(
        request_deadline(Some(&rendered(POD_START + 20)), ahead_of_the_request),
        Err(StartRefused::RequestExpired),
        "a pod clock 30 s ahead of a deadline 20 s off"
    );
}

/// DEC-2363 item 3: a deadline further than one lifetime ahead, from a pod clock behind or a bad
/// value, waits one lifetime from the pod's start and no longer.
#[test]
#[ignore = "pending E10-13"]
fn a_deadline_far_ahead_waits_one_lifetime_at_most() {
    let now = instant(POD_START);
    assert_eq!(
        request_deadline(Some(&rendered(POD_START + 86_400)), now),
        Ok(instant(POD_START + STATE_LIFETIME_SECS))
    );
    assert_eq!(
        request_deadline(Some(&rendered(POD_START + STATE_LIFETIME_SECS)), now),
        Ok(instant(POD_START + STATE_LIFETIME_SECS))
    );
}

/// The deadline's refusals have codes of their own (DEC-2363 item 2).
#[test]
fn the_deadline_s_refusals_have_their_codes() {
    assert_eq!(
        StartRefused::RequestDeadlineMissing.code(),
        "request_deadline_missing"
    );
    assert_eq!(
        StartRefused::RequestDeadlineMalformed.code(),
        "request_deadline_malformed"
    );
    assert_eq!(StartRefused::RequestExpired.code(), "request_expired");
}
