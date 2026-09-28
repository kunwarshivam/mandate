//! The recorded fixtures carry no credential, no broker account number, and no personal datum
//! (`AGENTS.md` rule 7, journal spec §6.4), and every one of them is well formed.
//!
//! These cases are **not** pending: they check the fixtures this PR commits, which exist now, and
//! a fixture that leaked a credential would be a defect to fix today rather than one to schedule.

mod common;

use std::collections::BTreeSet;
use std::fs;

use common::{
    data_fixtures_dir, data_scenario, every_data_scenario, every_scenario, fixtures_dir,
    looks_like_key_id, scenario,
};
use mandate_alpaca::{KEY_ID_VAR, SECRET_VAR, is_paper_trading_path};

/// Every scenario the task brief's fixture plan names, so one that was dropped is a failure and
/// not a silent gap.
const REQUIRED: [&str; 26] = [
    "account_active",
    "account_blocked",
    "activities_fills",
    "asset_absent",
    "asset_crypto",
    "asset_equity",
    "cancel_all_account_scope",
    "cancel_confirmed",
    "cancel_rejected_already_filled",
    "close_position_account_scope",
    "late_fill_after_terminal",
    "open_orders_page",
    "order_by_client_id_absent",
    "order_by_client_id_found",
    "partial_then_filled",
    "positions",
    "replace_pending_then_replaced",
    "status_unrecognised",
    "submit_bracket_accepted",
    "submit_crypto_stop_limit",
    "submit_duplicate_client_order_id",
    "submit_limit_accepted",
    "submit_oco_accepted",
    "submit_rejected",
    "submit_timeout_then_absent",
    "submit_timeout_then_found",
];

#[test]
fn every_scenario_the_fixture_plan_names_is_present() {
    let present: BTreeSet<String> = every_scenario().into_iter().collect();
    for name in REQUIRED {
        assert!(present.contains(name), "the fixture plan names `{name}`");
    }
    assert_eq!(
        present.len(),
        REQUIRED.len(),
        "a scenario was added without a row in the plan: {present:?}"
    );
}

/// The recorded and hand-built scenarios, as the README beside them declares them. A scenario
/// the table does not name, or names twice, or a row with no directory, is a failure: the
/// provenance of a fixture is part of what it proves (review round 1, finding 10).
#[test]
fn the_provenance_table_names_every_scenario_once() {
    let readme = fs::read_to_string(fixtures_dir().join("README.md"))
        .expect("alpaca-trading/README.md declares each scenario's provenance");
    let mut recorded = BTreeSet::new();
    let mut hand_built = BTreeSet::new();
    for row in readme.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        let (Some(name), Some(provenance), Some(why)) = (cells.get(1), cells.get(2), cells.get(3))
        else {
            panic!("a provenance row has three cells: {row}");
        };
        let name = name.trim_matches('`').to_owned();
        let fresh = match *provenance {
            "recorded" => {
                assert!(why.is_empty(), "a recording needs no reason: {row}");
                recorded.insert(name.clone())
            }
            "hand-built" => {
                assert!(!why.is_empty(), "a hand-built scenario says why: {row}");
                hand_built.insert(name.clone())
            }
            other => panic!("`{other}` is neither `recorded` nor `hand-built`: {row}"),
        };
        assert!(fresh, "{name} is named twice");
    }
    assert!(
        recorded.is_disjoint(&hand_built),
        "a scenario is recorded or hand-built, never both"
    );
    let declared: BTreeSet<String> = recorded.union(&hand_built).cloned().collect();
    let present: BTreeSet<String> = every_scenario().into_iter().collect();
    assert_eq!(
        declared, present,
        "the table and the directory list name the same scenarios"
    );
    assert_eq!(recorded.len(), 11, "eleven were recorded against paper");
    assert_eq!(
        hand_built.len(),
        15,
        "twelve were built by hand from the spec, and E7-8's three asset reads"
    );
}

/// The latest-quote scenarios E7-8 names (DEC-168).
const REQUIRED_DATA: [&str; 4] = [
    "quote_crypto",
    "quote_crypto_absent",
    "quote_equity",
    "quote_equity_absent",
];

/// The data host's scenarios, as its README declares them, and nothing else in the directory.
#[test]
fn the_data_provenance_table_names_every_scenario_once() {
    let readme = fs::read_to_string(data_fixtures_dir().join("README.md"))
        .expect("alpaca-data/README.md declares each scenario's provenance");
    let mut declared = BTreeSet::new();
    for row in readme.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = row.split('|').map(str::trim).collect();
        let (Some(name), Some(provenance), Some(why)) = (cells.get(1), cells.get(2), cells.get(3))
        else {
            panic!("a provenance row has three cells: {row}");
        };
        match *provenance {
            "recorded" => assert!(why.is_empty(), "a recording needs no reason: {row}"),
            "hand-built" => assert!(!why.is_empty(), "a hand-built scenario says why: {row}"),
            other => panic!("`{other}` is neither `recorded` nor `hand-built`: {row}"),
        }
        assert!(
            declared.insert(name.trim_matches('`').to_owned()),
            "{name} is named twice"
        );
    }
    let present: BTreeSet<String> = every_data_scenario().into_iter().collect();
    let required: BTreeSet<String> = REQUIRED_DATA.iter().map(|n| (*n).to_owned()).collect();
    assert_eq!(declared, present, "the table and the directory list agree");
    assert_eq!(present, required, "and both are the scenarios E7-8 names");
}

/// Every data scenario is one `GET` of a latest-quote path on the data host, with a JSON body,
/// and names no credential, no trading path, and no live trading host.
#[test]
fn every_data_fixture_is_one_latest_quote_read_and_holds_no_credential() {
    let secrets: Vec<String> = [KEY_ID_VAR, SECRET_VAR]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .filter(|value| !value.is_empty())
        .collect();
    let mut files: u32 = 0;
    for name in every_data_scenario() {
        let recorded = data_scenario(&name);
        assert_eq!(recorded.exchanges.len(), 1, "{name} is one read");
        for exchange in &recorded.exchanges {
            let path = exchange.path_and_query.as_str();
            let latest = (path.starts_with("/v2/stocks/")
                && path.ends_with("/quotes/latest?feed=iex"))
                || path.starts_with("/v1beta3/crypto/us/latest/quotes?symbols=");
            assert!(latest, "{name} names `{path}`, which is not a latest-quote read");
            assert_eq!(exchange.method, mandate_alpaca::Method::Get, "{name}");
            assert_eq!(exchange.body, None, "{name} sends no body");
            assert!(!is_paper_trading_path(path), "{name} is not a trading path");
            serde_json::from_slice::<serde_json::Value>(&exchange.response)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        let dir = data_fixtures_dir().join(&name);
        for entry in fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.expect("a readable entry").path();
            let text =
                fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let lower = text.to_ascii_lowercase();
            assert!(
                !lower.contains("apca-api") && !lower.contains("secret-key"),
                "{} names an authorisation header",
                path.display()
            );
            assert!(
                !text
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .any(looks_like_key_id),
                "{} holds something shaped like a key ID",
                path.display()
            );
            assert!(
                secrets.iter().all(|s| !text.contains(s.as_str())),
                "{} holds a credential value",
                path.display()
            );
            assert!(
                !text.contains("//api.alpaca.markets"),
                "{} names a live host (ES-23)",
                path.display()
            );
            files = files.saturating_add(1);
        }
    }
    assert!(files >= 12, "the scan must see every data file, saw {files}");
}

#[test]
fn recorded_fixtures_contain_no_credential() {
    let secrets: Vec<String> = [KEY_ID_VAR, SECRET_VAR]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .filter(|value| !value.is_empty())
        .collect();
    let mut files: u32 = 0;
    for name in every_scenario() {
        let dir = fixtures_dir().join(&name);
        for entry in fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.expect("a readable entry").path();
            let text =
                fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let lower = text.to_ascii_lowercase();
            assert!(
                !lower.contains("apca-api") && !lower.contains("secret-key"),
                "{} names an authorisation header",
                path.display()
            );
            assert!(
                !text
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .any(looks_like_key_id),
                "{} holds something shaped like a key ID",
                path.display()
            );
            assert!(
                secrets.iter().all(|s| !text.contains(s.as_str())),
                "{} holds a credential value",
                path.display()
            );
            files = files.saturating_add(1);
        }
    }
    assert!(
        files >= 60,
        "the scan must see every recorded file, saw {files}"
    );
}

#[test]
fn recorded_fixtures_hold_no_account_number_and_no_account_id() {
    for name in every_scenario() {
        let dir = fixtures_dir().join(&name);
        for entry in fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.expect("a readable entry").path();
            let text =
                fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let Some(object) = value.as_object() else {
                continue;
            };
            if let Some(number) = object.get("account_number").and_then(|v| v.as_str()) {
                assert!(
                    number.starts_with("pii:"),
                    "{} holds a broker account number (journal §6.4): the recorder replaces it \
                     by an opaque reference before the bytes are saved",
                    path.display()
                );
                let id = object
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                assert!(
                    id.starts_with("pii:"),
                    "{} holds the broker's account id beside a redacted number",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn every_fixture_is_well_formed_and_its_requests_line_up_with_its_responses() {
    for name in every_scenario() {
        let recorded = scenario(&name);
        assert!(
            !recorded.exchanges.is_empty(),
            "{name} records no exchange, so it pins nothing"
        );
        for (index, exchange) in recorded.exchanges.iter().enumerate() {
            assert!(
                mandate_alpaca::is_paper_trading_path(&exchange.path_and_query),
                "{name} exchange {index} names `{}`, which is not one of the allowed endpoints",
                exchange.path_and_query
            );
            assert!(
                (100..600).contains(&exchange.status),
                "{name} exchange {index} has status {}",
                exchange.status
            );
            serde_json::from_slice::<serde_json::Value>(&exchange.response)
                .unwrap_or_else(|e| panic!("{name} response-{}: {e}", index.saturating_add(1)));
            if let Some(body) = &exchange.body {
                serde_json::from_str::<serde_json::Value>(body).unwrap_or_else(|e| {
                    panic!("{name} request {index} body is not canonical JSON: {e}")
                });
                assert!(
                    !body.contains('\n'),
                    "{name} request {index} body must fit on one line"
                );
            }
        }
    }
}

#[test]
fn every_submission_fixture_carries_our_own_client_order_id() {
    for name in every_scenario()
        .into_iter()
        .filter(|n| n.starts_with("submit_"))
    {
        let recorded = scenario(&name);
        let Some(body) = recorded.exchanges.first().and_then(|e| e.body.clone()) else {
            continue;
        };
        let value: serde_json::Value =
            serde_json::from_str(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
        let id = value
            .get("client_order_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("{name} submits without a client_order_id"));
        assert!(
            id.starts_with(mandate_executor::PREFIX),
            "{name} submits `{id}`, which this platform could not have derived (E7-2)"
        );
    }
}

#[test]
fn no_fixture_names_a_live_host_or_a_funding_endpoint() {
    for name in every_scenario() {
        let recorded = scenario(&name);
        for exchange in &recorded.exchanges {
            assert!(
                !exchange.path_and_query.contains("//api.alpaca.markets"),
                "{name} names a live host (ES-23)"
            );
            for forbidden in ["transfer", "journal", "ach", "withdraw", "deposit"] {
                assert!(
                    !exchange.path_and_query.contains(forbidden),
                    "{name} names `{forbidden}`: there is no fund movement in this stream \
                     (AGENTS.md rule 8)"
                );
            }
        }
    }
}
