//! Property tests for the paper trading connector, against oracles written separately from the
//! crate ([task brief](../../../docs/project/tasks/M6-K-executor-and-connector.md)).
//!
//! 1. **A path oracle** decides membership of the seven endpoints by splitting the path into
//!    segments and comparing them, rather than by the crate's prefix walk, so the two agreeing
//!    means something.
//! 2. **A redaction oracle** walks the recorded bodies as `serde_json` values and looks for the
//!    personal-data fields journal spec §6.4 names, independently of the crate's own pass.
//!
//! Every property first compares a **count** with the oracle's, so none can pass vacuously.

mod common;

use std::collections::BTreeSet;

use common::{every_scenario, scenario};
use mandate_alpaca::http::{HttpRequest, Method, is_paper_trading_path};
use mandate_alpaca::{PAPER_HOST, record, wire};
use proptest::prelude::*;

/// The oracle's own reading of the endpoint list, by segments rather than by prefix.
fn oracle_allows(path_and_query: &str) -> bool {
    let (path, query) = match path_and_query.split_once('?') {
        Some(split) => split,
        None => (path_and_query, ""),
    };
    let unreserved = |text: &str, extra: &str| {
        text.chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
    };
    if !unreserved(query, "-._~%&=:,") {
        return false;
    }
    let segments: Vec<&str> = path.split('/').collect();
    if segments
        .iter()
        .any(|s| matches!(percent_decoded(s).as_str(), "." | ".."))
    {
        return false;
    }
    let one = |s: &&str| !s.is_empty() && unreserved(s, "-._~%");
    match segments.as_slice() {
        ["", "v2", "orders"] => true,
        ["", "v2", "orders:by_client_order_id"] => true,
        ["", "v2", "orders", id] => one(id),
        ["", "v2", "positions"] => true,
        ["", "v2", "positions", symbol] => one(symbol),
        ["", "v2", "account"] => true,
        ["", "v2", "account", "activities"] => true,
        _ => false,
    }
}

/// A segment as a URL parser reads it for dot-segment removal: every `%XX` triple decoded to its
/// byte. The oracle decodes every triple, not only `%2e`, so it reads a dot segment its own way
/// rather than the crate's.
fn percent_decoded(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let triple = bytes.get(at..at + 3).and_then(|t| {
            let hex = std::str::from_utf8(t.get(1..)?).ok()?;
            (t.first() == Some(&b'%')).then(|| u8::from_str_radix(hex, 16).ok())?
        });
        match triple {
            Some(byte) => {
                decoded.push(byte);
                at += 3;
            }
            None => {
                decoded.extend(bytes.get(at));
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// Every spelling of a dot segment a URL parser removes, and the ordinary segments that look like
/// one (#174: `url` 2.5.8 sends `/v2/orders/%2e` to `/v2/orders/`).
const DOT_LIKE: [&str; 12] = [
    ".",
    "..",
    "%2e",
    "%2E",
    ".%2e",
    "%2e.",
    "%2E%2e",
    "%2e%2E",
    "A..B",
    "...",
    "%2e%2e%2e",
    "BRK.B",
];

/// A generator that produces both plausible endpoints and plausible attacks on them.
fn candidate_path() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(DOT_LIKE.as_slice()).prop_map(|s| format!("/v2/orders/{s}")),
        prop::sample::select(DOT_LIKE.as_slice()).prop_map(|s| format!("/v2/positions/{s}")),
        (
            prop::sample::select(DOT_LIKE.as_slice()),
            "[A-Za-z0-9]{1,8}"
        )
            .prop_map(|(s, tail)| format!("/v2/orders/{s}/{tail}")),
        Just("/v2/orders".to_owned()),
        Just("/v2/account".to_owned()),
        Just("/v2/account/activities".to_owned()),
        Just("/v2/account/configurations".to_owned()),
        Just("/v2/positions".to_owned()),
        Just("/v2/transfers".to_owned()),
        Just("/v2/journals".to_owned()),
        "[A-Za-z0-9./:?=&%_-]{0,40}".prop_map(|rest| format!("/v2/orders/{rest}")),
        "[A-Za-z0-9./:?=&%_-]{0,40}".prop_map(|rest| format!("/v2/positions/{rest}")),
        "[A-Za-z0-9./:?=&%_-]{0,40}".prop_map(|rest| format!("/v2/account{rest}")),
        "[A-Za-z0-9./:?=&%_ -]{0,30}".prop_map(|rest| rest.to_owned()),
    ]
}

/// The personal data journal §6.4 names in a broker body, found by walking the JSON itself: the
/// `id` of the **account object** (the object that carries `account_number`), and every
/// `account_number` or `account_id` anywhere. An order's `id` is not personal data — trading §13
/// keeps raw exchanges as records, and redacting order ids would destroy the audit trail for no
/// privacy gain (the coordinator's ruling on #152).
fn personal_data(value: &serde_json::Value) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut walk = vec![value];
    while let Some(node) = walk.pop() {
        match node {
            serde_json::Value::Object(map) => {
                let account = map.contains_key("account_number");
                for (key, inner) in map {
                    let personal =
                        key == "account_number" || key == "account_id" || (account && key == "id");
                    if personal
                        && let Some(text) = inner.as_str()
                        && !text.starts_with("pii:")
                    {
                        found.insert(text.to_owned());
                    }
                    walk.push(inner);
                }
            }
            serde_json::Value::Array(items) => walk.extend(items.iter()),
            _ => {}
        }
    }
    found
}

/// Puts realistic, unredacted personal data back into a recorded body, because the committed
/// fixtures are already redacted and a redaction pass over them would have nothing to find.
fn plant_personal_data(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            if map.contains_key("account_number") {
                map.insert(
                    "account_number".to_owned(),
                    serde_json::json!("PA3PLANTED0001"),
                );
                map.insert(
                    "id".to_owned(),
                    serde_json::json!("8f1c5b2a-1111-4000-8000-00000000plnt"),
                );
            }
            for inner in map.values_mut() {
                plant_personal_data(inner);
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(plant_personal_data),
        _ => {}
    }
}

proptest! {
    /// ES-23: every request this client can build targets the paper host and one of the seven
    /// endpoints, whatever a symbol or an order id contains.
    #[test]
    fn every_request_the_client_can_build_targets_the_paper_host(path in candidate_path()) {
        prop_assert_eq!(
            is_paper_trading_path(&path),
            oracle_allows(&path),
            "the crate and the oracle disagree about `{}`",
            path
        );
        if is_paper_trading_path(&path) {
            let url = format!("{PAPER_HOST}{path}");
            prop_assert!(
                url.starts_with("https://paper-api.alpaca.markets/v2/"),
                "an allowed path builds only a paper URL: {}",
                url
            );
            prop_assert!(
                !url.contains("//api.alpaca.markets") && !url.contains('\n'),
                "and nothing in it can reach a live host or split a header: {}",
                url
            );
            let parsed = reqwest::Url::parse(&url).map(|u| u.to_string());
            prop_assert_eq!(
                parsed,
                Ok(url.clone()),
                "and a URL parser sends it exactly as it was built, so the path judged is the \
                 path sent (#174)"
            );
        }
    }

    /// `AGENTS.md` rule 8: no path this client accepts is a funding endpoint.
    #[test]
    fn no_accepted_path_moves_money(path in candidate_path()) {
        prop_assume!(is_paper_trading_path(&path));
        for forbidden in ["transfer", "journal", "ach", "withdraw", "deposit", "relationship"] {
            prop_assert!(
                !path.contains(forbidden),
                "`{}` would move money, and there is no such endpoint in this crate",
                path
            );
        }
    }

    /// Journal §6.4, interpretation 24: nothing the recorder saved holds an account number or an
    /// account id, and the redaction pass agrees with an oracle that walks the JSON itself.
    #[test]
    fn no_recorded_exchange_holds_an_account_number_or_an_account_id(
        which in prop::sample::select(every_scenario()),
    ) {
        let recorded = scenario(&which);
        prop_assert!(!recorded.exchanges.is_empty(), "{} records nothing", which);
        let mut checked = 0usize;
        for exchange in &recorded.exchanges {
            let mut body: serde_json::Value = serde_json::from_slice(&exchange.response)
                .map_err(|e| TestCaseError::fail(format!("{which}: {e}")))?;
            plant_personal_data(&mut body);
            let planted = serde_json::to_vec(&body)
                .map_err(|e| TestCaseError::fail(format!("{which}: {e}")))?;
            let response = mandate_alpaca::Response {
                status: exchange.status,
                body: planted,
            };
            let pass = record::response(&exchange.path_and_query, &response)
                .map_err(|e| TestCaseError::fail(format!("{which}: {e}")))?;
            checked = checked.saturating_add(1);
            let rendered = format!("{:?}", pass.body);
            let oracle = personal_data(&body);
            for secret in &oracle {
                prop_assert!(
                    !rendered.contains(secret.as_str()),
                    "{}: `{}` survived the redaction pass (planted bug 21)",
                    which,
                    secret
                );
            }
            prop_assert_eq!(
                pass.pii_refs.len(),
                oracle.len(),
                "{}: one `pii_refs` entry per personal-data field replaced, no more and no fewer",
                which
            );
            let kept = match &pass.body {
                mandate_alpaca::RecordedBody::Inline(text) => !text.is_empty(),
                mandate_alpaca::RecordedBody::Artifact { bytes, .. } => !bytes.is_empty(),
            };
            prop_assert!(
                kept,
                "{}: a response is recorded, not dropped (trading §13 keeps raw exchanges)",
                which
            );
        }
        prop_assert_eq!(checked, recorded.exchanges.len());
    }

    /// ES-23: a decimal field is read as text or refused, never rounded.
    #[test]
    fn no_decimal_field_is_ever_read_through_a_float(
        text in "(0|[1-9][0-9]{0,6})(\\.[0-9]{1,12})?",
    ) {
        let raw = serde_json::Value::String(text.clone());
        match wire::decimal_text(&raw, "price") {
            Ok(parsed) => prop_assert_eq!(
                &parsed,
                &text,
                "a value read as text comes back byte for byte"
            ),
            Err(error) => prop_assert!(
                ["too_many_places", "exponent_form"].contains(&error.code()),
                "a refusal names why, and rounding is never one of the answers: {}",
                error
            ),
        }
        let as_number: serde_json::Value = serde_json::json!(1.5);
        prop_assert_eq!(
            wire::decimal_text(&as_number, "price").err().map(|e| e.code()),
            Some("float_number"),
            "a JSON number has already been through a float and is always refused"
        );
    }

    /// `AGENTS.md` rule 7: no request this crate builds carries a credential in its body or its
    /// path, because credentials travel only as headers the transport sets.
    #[test]
    fn no_recorded_request_holds_a_credential(
        which in prop::sample::select(every_scenario()),
    ) {
        let recorded = scenario(&which);
        let mut checked = 0usize;
        let mut account_wide = 0usize;
        for exchange in &recorded.exchanges {
            let Ok(request) = HttpRequest::new(
                exchange.method,
                &exchange.path_and_query,
                exchange.body.clone(),
            ) else {
                prop_assert!(
                    mandate_alpaca::endpoint_for(exchange.method, &exchange.path_and_query)
                        .is_some_and(|endpoint| endpoint.account_wide),
                    "{}: `{} {}` is refused and is not an account-wide endpoint",
                    which,
                    exchange.method.as_str(),
                    exchange.path_and_query
                );
                account_wide = account_wide.saturating_add(1);
                continue;
            };
            let pass = record::request(&request)
                .map_err(|e| TestCaseError::fail(format!("{which}: {e}")))?;
            checked = checked.saturating_add(1);
            let rendered = format!("{pass:?}");
            for needle in ["APCA", "apca", common::SENTINEL_KEY_ID, common::SENTINEL_SECRET] {
                prop_assert!(
                    !rendered.contains(needle),
                    "{}: a recorded request holds `{}`",
                    which,
                    needle
                );
            }
            let path = exchange
                .path_and_query
                .split_once('?')
                .map_or(exchange.path_and_query.as_str(), |(path, _)| path);
            prop_assert_eq!(
                pass.endpoint.as_str(),
                path,
                "{}: the record names the endpoint and no query value",
                which
            );
            let expected_body = exchange.body.clone().unwrap_or_default();
            prop_assert_eq!(
                &pass.body,
                &mandate_alpaca::RecordedBody::Inline(expected_body),
                "{}: a request body carries no personal data and is kept byte for byte",
                which
            );
        }
        prop_assert_eq!(checked.saturating_add(account_wide), recorded.exchanges.len());
        prop_assert!(checked > 0 || account_wide > 0);
    }

    /// Every method a fixture records is one of the three this crate uses; `PUT` is absent
    /// because §5.1 forbids our own replaces.
    #[test]
    fn no_fixture_uses_a_method_this_crate_does_not_have(
        which in prop::sample::select(every_scenario()),
    ) {
        let recorded = scenario(&which);
        prop_assert!(!recorded.exchanges.is_empty());
        for exchange in &recorded.exchanges {
            prop_assert!(
                matches!(exchange.method, Method::Get | Method::Post | Method::Delete),
                "{} uses a method outside the three (§5.1: no replace or amend of ours)",
                which
            );
        }
    }
}
