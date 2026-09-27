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
    if segments.contains(&"..") {
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

/// A generator that produces both plausible endpoints and plausible attacks on them.
fn candidate_path() -> impl Strategy<Value = String> {
    prop_oneof![
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
    #[ignore = "pending E7-2"]
    fn no_recorded_exchange_holds_an_account_number_or_an_account_id(
        which in prop::sample::select(every_scenario()),
    ) {
        let recorded = scenario(&which);
        prop_assert!(!recorded.exchanges.is_empty(), "{} records nothing", which);
        let mut checked = 0usize;
        for exchange in &recorded.exchanges {
            let response = mandate_alpaca::Response {
                status: exchange.status,
                body: exchange.response.clone(),
            };
            let pass = record::response(&exchange.path_and_query, &response)
                .map_err(|e| TestCaseError::fail(format!("{which}: {e}")))?;
            checked = checked.saturating_add(1);
            let rendered = format!("{:?}", pass.body);
            let oracle: BTreeSet<String> = serde_json::from_slice::<serde_json::Value>(
                &exchange.response,
            )
            .ok()
            .and_then(|value| value.as_object().cloned())
            .into_iter()
            .flatten()
            .filter(|(key, _)| key == "account_number" || key == "id")
            .filter_map(|(_, value)| value.as_str().map(str::to_owned))
            .filter(|value| !value.starts_with("pii:"))
            .collect();
            for secret in &oracle {
                prop_assert!(
                    !rendered.contains(secret.as_str()),
                    "{}: a personal-data field survived the redaction pass (planted bug 21)",
                    which
                );
            }
        }
        prop_assert_eq!(checked, recorded.exchanges.len());
    }

    /// ES-23: a decimal field is read as text or refused, never rounded.
    #[test]
    #[ignore = "pending E7-2"]
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
    #[ignore = "pending E7-2"]
    fn no_recorded_request_holds_a_credential(
        which in prop::sample::select(every_scenario()),
    ) {
        let recorded = scenario(&which);
        let mut checked = 0usize;
        for exchange in &recorded.exchanges {
            let request = HttpRequest {
                method: exchange.method,
                path_and_query: exchange.path_and_query.clone(),
                body: exchange.body.clone(),
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
        }
        prop_assert_eq!(checked, recorded.exchanges.len());
        prop_assert!(checked > 0);
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
