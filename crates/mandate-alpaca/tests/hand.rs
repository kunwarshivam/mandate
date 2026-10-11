//! Hand-calculated cases for the Alpaca paper trading connector
//! ([task brief](../../../docs/project/tasks/M6-K-executor-and-connector.md), backlog E7-2,
//! E7-3, E7-4).
//!
//! Every case drives the client through the **recorded** scenario it names, with no network
//! (ADR-0001 ES-19), and computes its expectation from the spec rather than from the code. Cases
//! whose subject is the submission or the status mapping are `pending E7-2`; the reconciliation
//! sources are `pending E7-3`; the protective shapes are `pending E7-4`.

mod common;

use std::sync::{Arc, Mutex};

use common::{FakeClock, FakeTransport, scenario};
use mandate_accounting::Side;
use mandate_alpaca::client::{RetryPolicy, TradingClient};
use mandate_alpaca::error::{TransportError, WireError};
use mandate_alpaca::http::{
    Credentials, HttpRequest, Method, Response, TradingTransport, is_paper_trading_path,
};
use mandate_alpaca::{KEY_ID_VAR, PAPER_HOST, Pause, SECRET_VAR, TokioPause, record, wire};
use mandate_executor::{BrokerOutcome, BrokerUnknown};

fn client(name: &str) -> (TradingClient<FakeTransport, FakeClock>, FakeTransport) {
    let recorded = scenario(name);
    let transport = FakeTransport::replaying(&recorded);
    (
        TradingClient::new(
            transport.clone(),
            FakeClock::default(),
            RetryPolicy::default(),
        ),
        transport,
    )
}

fn body_of(name: &str, index: usize) -> Vec<u8> {
    let recorded = scenario(name);
    recorded
        .exchanges
        .get(index)
        .map(|exchange| exchange.response.clone())
        .unwrap_or_else(|| panic!("{name} has no exchange {index}"))
}

#[test]
fn there_is_no_live_base_url_in_the_crate() {
    assert_eq!(PAPER_HOST, "https://paper-api.alpaca.markets");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut scanned = Vec::new();
    for entry in std::fs::read_dir(root.join("src")).expect("the crate has a src directory") {
        let path = entry.expect("a readable entry").path();
        let source =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for live in ["//api.alpaca.markets", "\"live\""] {
            assert!(
                !source.contains(live),
                "{} names `{live}`: ES-23 compiles in the paper trading host only",
                path.display()
            );
        }
        scanned.push(path);
    }
    let manifest =
        std::fs::read_to_string(root.join("Cargo.toml")).expect("the crate has a manifest");
    assert!(
        !manifest.contains("[features]") && !manifest.contains("live"),
        "and the manifest declares no `live` feature (ES-23)"
    );
    assert!(
        scanned.len() >= 6,
        "every source file is scanned, not one: {scanned:?}"
    );
}

#[test]
fn a_request_is_one_of_the_endpoints_by_method_and_path_together() {
    for (method, path) in [
        (Method::Post, "/v2/orders"),
        (Method::Get, "/v2/orders?status=open&limit=50&direction=asc"),
        (
            Method::Get,
            "/v2/orders:by_client_order_id?client_order_id=md-abc",
        ),
        (
            Method::Get,
            "/v2/orders/e02fc2d2-0ff3-444f-a0ab-6253613302fe",
        ),
        (
            Method::Delete,
            "/v2/orders/e02fc2d2-0ff3-444f-a0ab-6253613302fe",
        ),
        (Method::Get, "/v2/positions"),
        (Method::Get, "/v2/positions/AAPL"),
        (Method::Get, "/v2/account"),
        (Method::Get, "/v2/account/activities?activity_types=FILL"),
    ] {
        let request = HttpRequest::new(method, path, None)
            .unwrap_or_else(|e| panic!("{} {path} is one of ours: {e}", method.as_str()));
        assert_eq!(request.method(), method);
        assert_eq!(request.path_and_query(), path);
        assert_eq!(request.body(), None);
        assert_eq!(
            request.url(),
            format!("https://paper-api.alpaca.markets{path}"),
            "the URL is the paper host and the path, and nothing a caller chose"
        );
    }
    let body = HttpRequest::new(
        Method::Post,
        "/v2/orders",
        Some("{\"client_order_id\":\"md-1\"}".to_owned()),
    )
    .expect("a submission");
    assert_eq!(body.body(), Some("{\"client_order_id\":\"md-1\"}"));

    for (method, path) in [
        (Method::Post, "/v2/account"),
        (Method::Get, "/v2/transfers"),
        (
            Method::Post,
            "/v2/orders/e02fc2d2-0ff3-444f-a0ab-6253613302fe",
        ),
        (Method::Delete, "/v2/account"),
        (Method::Delete, "/v2/positions"),
    ] {
        assert_eq!(
            HttpRequest::new(method, path, None).err(),
            Some(mandate_alpaca::TransportError::RefusedPath),
            "{} {path} pairs a method with a path the table does not pair it with",
            method.as_str()
        );
    }
}

#[test]
fn the_account_wide_endpoints_cannot_be_built_without_an_account_wide_scope() {
    for path in [
        "/v2/orders",
        "/v2/positions/AAPL",
        "/v2/positions/AAPL?percentage=100",
    ] {
        assert_eq!(
            HttpRequest::new(Method::Delete, path, None).err(),
            Some(mandate_alpaca::TransportError::RefusedPath),
            "DELETE {path} is account-wide: only `HttpRequest::cancel_all` and \
             `HttpRequest::close_position`, which take the executor's `AccountWideScope`, build it \
             (AGENTS.md rule 13), so no request exists for a transport to send"
        );
        let endpoint = mandate_alpaca::endpoint_for(Method::Delete, path)
            .unwrap_or_else(|| panic!("DELETE {path} is in the table"));
        assert!(endpoint.account_wide, "and the table marks it account-wide");
    }
    let listing = mandate_alpaca::endpoint_for(Method::Get, "/v2/orders").expect("listed");
    assert!(
        !listing.account_wide,
        "the same path under GET only lists the open orders"
    );
    let cancel_one =
        mandate_alpaca::endpoint_for(Method::Delete, "/v2/orders/abc").expect("cancel one");
    assert!(
        !cancel_one.account_wide,
        "and DELETE of one order is ordinary"
    );
    assert!(
        mandate_alpaca::endpoint_for(Method::Delete, "/v2/positions").is_none(),
        "closing every position at once is not in the table at all"
    );
    assert_eq!(
        mandate_alpaca::ENDPOINTS
            .iter()
            .filter(|endpoint| endpoint.account_wide)
            .count(),
        2,
        "exactly the two account-wide endpoints trading-domain spec §5.5 names"
    );
}

#[test]
fn every_method_has_its_wire_name() {
    assert_eq!(Method::Get.as_str(), "GET");
    assert_eq!(Method::Post.as_str(), "POST");
    assert_eq!(Method::Delete.as_str(), "DELETE");
}

#[test]
fn the_endpoint_allowlist_is_the_seven_paths_this_stream_needs() {
    for path in [
        "/v2/orders",
        "/v2/orders?status=open&limit=50",
        "/v2/orders/e02fc2d2-0ff3-444f-a0ab-6253613302fe",
        "/v2/orders:by_client_order_id?client_order_id=md-abc",
        "/v2/positions",
        "/v2/positions/AAPL?percentage=100",
        "/v2/account",
        "/v2/account/activities?activity_types=FILL&page_size=50",
    ] {
        assert!(is_paper_trading_path(path), "{path} is one of ours");
    }
}

#[test]
fn a_path_that_is_not_a_paper_trading_endpoint_is_refused() {
    for path in [
        "/v2/account/configurations",
        "/v2/transfers",
        "/v2/journals",
        "/v1/corporate-actions?symbols=AAPL",
        "//data.alpaca.markets/v2/account",
        "/v2/orders/../../v2/transfers",
        "https://api.alpaca.markets/v2/account",
        "/v2/account\nX-Injected: 1",
    ] {
        assert!(
            !is_paper_trading_path(path),
            "{path} must never be sent: there is no funding or transfer endpoint in this crate \
             (AGENTS.md rule 8)"
        );
    }
}

/// A dot segment is refused whatever it is spelled as, because a URL parser removes it: each row
/// is a path `url` 2.5.8 sends somewhere else (measured on #174). `DELETE /v2/orders/%2e` would
/// reach `DELETE /v2/orders/`, the account-wide cancel-all (DEC-133 item 18a, AGENTS.md rule 13).
fn assert_refused_and_never_built(method: Method, path: &str, sent_to: &str) {
    assert!(
        !is_paper_trading_path(path),
        "{path} is not one of ours: a parser sends it to {sent_to}"
    );
    assert_eq!(
        mandate_alpaca::endpoint_for(method, path),
        None,
        "{} {path} matches no endpoint, since a parser sends it to {sent_to}",
        method.as_str()
    );
    assert_eq!(
        HttpRequest::new(method, path, None).err(),
        Some(mandate_alpaca::TransportError::RefusedPath),
        "{} {path} is refused, so no request exists for a transport to send to {sent_to}",
        method.as_str()
    );
}

#[test]
fn a_single_dot_order_id_is_refused_and_never_sent() {
    assert_refused_and_never_built(Method::Delete, "/v2/orders/.", "/v2/orders/");
    assert_refused_and_never_built(Method::Get, "/v2/orders/.", "/v2/orders/");
}

#[test]
fn a_percent_encoded_dot_order_id_is_refused_and_never_sent() {
    for path in ["/v2/orders/%2e", "/v2/orders/%2E"] {
        assert_refused_and_never_built(Method::Delete, path, "/v2/orders/");
        assert_refused_and_never_built(Method::Get, path, "/v2/orders/");
    }
}

#[test]
fn a_double_dot_order_id_in_any_spelling_is_refused_and_never_sent() {
    for path in [
        "/v2/orders/..",
        "/v2/orders/%2e%2e",
        "/v2/orders/.%2e",
        "/v2/orders/%2e.",
        "/v2/orders/%2E%2e",
    ] {
        assert_refused_and_never_built(Method::Delete, path, "/v2/");
        assert_refused_and_never_built(Method::Get, path, "/v2/");
    }
}

#[test]
fn a_dot_segment_symbol_is_refused_and_never_sent() {
    for path in [
        "/v2/positions/.",
        "/v2/positions/%2e",
        "/v2/positions/%2E%2e",
    ] {
        assert_refused_and_never_built(Method::Get, path, "/v2/positions/ or /v2/");
    }
}

#[test]
fn a_symbol_with_two_dots_inside_it_is_allowed_and_sent_unchanged() {
    for path in ["/v2/orders/A..B", "/v2/positions/A..B"] {
        assert!(
            is_paper_trading_path(path),
            "{path} has no dot segment; `A..B` is one ordinary segment"
        );
        let request = HttpRequest::new(Method::Get, path, None)
            .unwrap_or_else(|e| panic!("GET {path} is one of ours: {e}"));
        assert_eq!(
            request.url(),
            format!("https://paper-api.alpaca.markets{path}"),
            "and it is sent exactly as it was built"
        );
    }
}

#[tokio::test]
async fn the_client_refuses_a_path_that_is_not_a_paper_trading_endpoint() {
    let (client, transport) = client("submit_limit_accepted");
    let error = client
        .order_by_client_id("../../v2/transfers")
        .await
        .expect_err("a refused path is never a broker fact");
    assert_eq!(
        error.code(),
        "refused_path",
        "the refusal happens before anything is sent: {error}"
    );
    assert!(
        error.as_unknown().is_none(),
        "and a path that never left the process is not an unknown broker outcome"
    );
    assert_eq!(
        error.to_connector().code(),
        "not_sent",
        "the connector reports it as never sent, not as an unknown outcome or a rejection"
    );
    assert!(
        transport.sent().is_empty(),
        "and nothing left the process: {:?}",
        transport.sent()
    );
}

#[test]
fn credentials_are_redacted_in_debug_output() {
    let credentials = Credentials::from_lookup(|name| match name {
        KEY_ID_VAR => Some(common::SENTINEL_KEY_ID.to_owned()),
        SECRET_VAR => Some(common::SENTINEL_SECRET.to_owned()),
        _ => None,
    })
    .expect("both variables are set");
    let rendered = format!("{credentials:?}");
    assert!(
        !rendered.contains(common::SENTINEL_KEY_ID) && !rendered.contains(common::SENTINEL_SECRET),
        "`Debug` prints neither value (AGENTS.md rule 7, ES-09): {rendered}"
    );
    assert!(
        rendered.contains("redacted"),
        "and says so rather than printing an empty struct: {rendered}"
    );
}

#[test]
fn a_missing_credential_names_the_variable_and_nothing_else() {
    let error = Credentials::from_lookup(|_| None).expect_err("nothing is set");
    assert_eq!(error.code(), "missing");
    let rendered = format!("{error}");
    assert!(
        rendered.contains(KEY_ID_VAR),
        "the error names the variable so an operator can fix it: {rendered}"
    );
}

#[test]
fn a_transport_error_names_no_url_and_no_header() {
    for error in [
        mandate_alpaca::TransportError::Timeout,
        mandate_alpaca::TransportError::Connect,
        mandate_alpaca::TransportError::Request,
        mandate_alpaca::TransportError::RefusedPath,
    ] {
        let rendered = format!("{error}");
        assert!(
            !rendered.contains("alpaca")
                && !rendered.contains("APCA")
                && !rendered.contains("http"),
            "a transport error carries no URL, header, or body (ES-09): {rendered}"
        );
        assert!(!error.code().is_empty(), "every variant has a stable code");
    }
}

#[test]
fn a_broker_decimal_with_nine_places_parses_exactly() {
    let raw = serde_json::json!("0.123456789");
    let parsed = wire::decimal_text(&raw, "qty").expect("nine places is the quantity scale");
    assert_eq!(parsed, "0.123456789", "read as text, never through an f64");
}

#[test]
fn a_broker_number_in_exponent_form_is_rejected_with_its_code() {
    let raw = serde_json::json!("1e3");
    let error = wire::decimal_text(&raw, "limit_price").expect_err("an exponent is not canonical");
    assert_eq!(error.code(), "exponent_form", "{error}");
}

#[test]
fn a_json_number_is_rejected_because_it_has_already_been_through_a_float() {
    let raw = serde_json::json!(150.25);
    let error = wire::decimal_text(&raw, "price").expect_err("a JSON number is not raw text");
    assert_eq!(error.code(), "float_number", "{error}");
}

#[test]
fn a_quantity_with_more_places_than_the_increment_is_rejected() {
    let raw = serde_json::json!("0.1234567891");
    let error = wire::decimal_text(&raw, "qty").expect_err("ten places is past the scale");
    assert_eq!(error.code(), "too_many_places", "{error}");
}

#[test]
fn every_broker_status_in_the_table_maps() {
    use mandate_executor::{OrderState, StatusMapping};
    let becomes = StatusMapping::Becomes;
    for (status, expected) in [
        ("new", becomes(OrderState::Accepted)),
        ("accepted", becomes(OrderState::Accepted)),
        ("pending_new", becomes(OrderState::Accepted)),
        ("accepted_for_bidding", becomes(OrderState::Accepted)),
        ("held", becomes(OrderState::Accepted)),
        ("partially_filled", becomes(OrderState::PartiallyFilled)),
        ("filled", becomes(OrderState::Filled)),
        ("done_for_day", StatusMapping::Unchanged),
        ("stopped", StatusMapping::Unchanged),
        ("calculated", StatusMapping::Unchanged),
        ("pending_cancel", becomes(OrderState::PendingCancel)),
        ("canceled", becomes(OrderState::Canceled)),
        ("expired", becomes(OrderState::Expired)),
        ("rejected", becomes(OrderState::Rejected)),
        ("suspended", StatusMapping::AcceptedFlaggedRestricted),
        ("pending_replace", becomes(OrderState::PendingReplace)),
        ("replaced", StatusMapping::ReplacedPair),
    ] {
        let mapped = wire::status(status)
            .unwrap_or_else(|e| panic!("§5.7's table names `{status}` and it did not map: {e}"));
        assert_eq!(mapped, expected, "§5.7's row for `{status}`");
    }
}

#[test]
fn a_status_outside_the_table_is_a_typed_error_not_a_guess() {
    let error = wire::status("quantum_superposition").expect_err("§5.7's last row");
    assert_eq!(error.code(), "unknown_status", "{error}");
    let recorded = body_of("status_unrecognised", 0);
    assert!(
        String::from_utf8_lossy(&recorded).contains("quantum_superposition"),
        "and the recorded scenario is what the executor folds into a pause and an alert"
    );
}

#[test]
fn a_recorded_accepted_order_parses_into_the_executor_vocabulary() {
    let parsed = wire::order(&body_of("submit_limit_accepted", 0)).expect("the recording parses");
    assert_eq!(
        parsed.client_order_id.as_deref(),
        Some("md-e144b97773a6f87c1978cc2831"),
        "**our** client order id is what came back on the wire (E7-2)"
    );
    assert_eq!(parsed.status, "accepted");
    assert_eq!(parsed.qty.to_string(), "1");
    assert_eq!(parsed.filled_qty.to_string(), "0");
}

#[test]
fn the_parser_accepts_unknown_extra_fields() {
    let mut body: serde_json::Value =
        serde_json::from_slice(&body_of("submit_limit_accepted", 0)).expect("valid JSON");
    if let Some(object) = body.as_object_mut() {
        object.insert(
            "a_field_alpaca_added_later".to_owned(),
            serde_json::json!("x"),
        );
    }
    let text = serde_json::to_vec(&body).expect("re-serialises");
    let parsed = wire::order(&text).expect(
        "a later recording carries more than a hand-built body, and the permissiveness is \
         exactly where it is safe: an unknown **status** still fails loudly",
    );
    assert_eq!(
        parsed.broker_order_id, "e02fc2d2-0ff3-444f-a0ab-6253613302fe",
        "the extra field is ignored and the recording's own fields still arrive"
    );
    assert_eq!(parsed.qty.to_string(), "1");
    assert_eq!(parsed.status, "accepted");
}

/// Defence in depth behind the allowlist: a broker order id is a UUID, so an `id` outside
/// `[A-Za-z0-9-]+` is an answer this crate cannot read, and it never reaches the
/// `/v2/orders/{id}` the cancel builds from it (DEC-133 item 18a, #174).
#[test]
fn a_broker_order_id_outside_a_uuids_alphabet_is_unreadable() {
    for id in [
        ".",
        "..",
        "%2e",
        "%2E%2e",
        "a/b",
        "",
        "A..B",
        "e02fc2d2 0ff3",
    ] {
        let mut body: serde_json::Value =
            serde_json::from_slice(&body_of("submit_limit_accepted", 0)).expect("valid JSON");
        if let Some(object) = body.as_object_mut() {
            object.insert("id".to_owned(), serde_json::json!(id));
        }
        let text = serde_json::to_vec(&body).expect("re-serialises");
        let error = wire::order(&text).expect_err("a hostile broker id is not an order");
        assert_eq!(
            (error.code(), error.to_string()),
            ("wrong_type", "field id has the wrong type".to_owned()),
            "the id {id:?} is not a UUID's alphabet, so it is refused before it can reach a path"
        );
        assert_eq!(
            mandate_alpaca::ClientError::from(error)
                .to_connector()
                .code(),
            "unreadable",
            "and the shell stops and alerts on it rather than sending anything (DEC-85)"
        );
        let mut legged: serde_json::Value =
            serde_json::from_slice(&body_of("submit_limit_accepted", 0)).expect("valid JSON");
        let leg_with = |leg_id: &str| {
            let mut leg = legged.clone();
            if let Some(object) = leg.as_object_mut() {
                object.insert("id".to_owned(), serde_json::json!(leg_id));
            }
            leg
        };
        let legs = serde_json::json!([
            leg_with("e02fc2d2-0ff3-444f-a0ab-6253613302ff"),
            leg_with(id)
        ]);
        if let Some(object) = legged.as_object_mut() {
            object.insert("legs".to_owned(), legs);
        }
        let text = serde_json::to_vec(&legged).expect("re-serialises");
        let error = wire::order(&text).expect_err("a hostile leg id is not an order's leg");
        assert_eq!(
            (error.code(), error.to_string()),
            ("wrong_type", "field id has the wrong type".to_owned()),
            "a leg id {id:?} is a broker order id a cancel is built from too, so it is refused \
             the same way"
        );
        let mut replacing: serde_json::Value =
            serde_json::from_slice(&body_of("submit_limit_accepted", 0)).expect("valid JSON");
        if let Some(object) = replacing.as_object_mut() {
            object.insert("replaced_by".to_owned(), serde_json::json!(id));
        }
        let text = serde_json::to_vec(&replacing).expect("re-serialises");
        let error = wire::order(&text).expect_err("a hostile replacement id is not an order's");
        assert_eq!(
            (error.code(), error.to_string()),
            (
                "wrong_type",
                "field replaced_by has the wrong type".to_owned()
            ),
            "a replacement id {id:?} names a broker order a cancel is built from too"
        );
        assert_refused_activity_id(id);
    }
    for injected in [
        "20260926234500000::x&y",
        "20260926234500000::x=y",
        "20260926234500000::x%26y",
        "20260926234500000::x/y",
    ] {
        assert_refused_activity_id(injected);
    }
    let fills = wire::activities(&body_of("partial_then_filled", 2)).expect("the fixture parses");
    assert!(
        fills.iter().all(|f| f.fill_id.0.contains("::")),
        "and a real activity id, a timestamp, `::` and a UUID, still reads"
    );
}

/// An activity id becomes the cursor the next activities read sends as `page_token`, so one
/// outside a timestamp-and-UUID alphabet is refused before it can reach a query.
fn assert_refused_activity_id(id: &str) {
    let mut body: serde_json::Value =
        serde_json::from_slice(&body_of("partial_then_filled", 2)).expect("valid JSON");
    if let Some(object) = body
        .as_array_mut()
        .and_then(|all| all.first_mut())
        .and_then(serde_json::Value::as_object_mut)
    {
        object.insert("id".to_owned(), serde_json::json!(id));
    }
    let text = serde_json::to_vec(&body).expect("re-serialises");
    let error = wire::activities(&text).expect_err("a hostile activity id is not a fill's");
    assert_eq!(
        (error.code(), error.to_string()),
        ("wrong_type", "field id has the wrong type".to_owned()),
        "the activity id {id:?} is refused before it can become a page_token"
    );
}

/// A position's symbol reaches `/v2/positions/{symbol}`, so it is held to a symbol's alphabet:
/// one or two `/`-separated segments of letters, digits and `.`, each starting with a letter or a
/// digit (#191 review, round 3).
#[test]
fn a_position_symbol_outside_a_symbols_alphabet_is_unreadable() {
    let row = |symbol: &str| {
        serde_json::to_vec(&serde_json::json!([{
            "asset_class": "us_equity",
            "avg_entry_price": "150",
            "qty": "1",
            "side": "long",
            "symbol": symbol,
        }]))
        .expect("serialises")
    };
    for symbol in ["AAPL", "BRK.B", "BTC/USD", "FRAC1"] {
        let positions =
            wire::positions(&row(symbol)).unwrap_or_else(|e| panic!("{symbol} is a symbol: {e}"));
        assert_eq!(
            positions.first().map(|p| p.instrument.as_str()),
            Some(symbol),
            "{symbol} reads as itself"
        );
    }
    for symbol in [
        ".", "..", ".A", "%2e", "AAPL/..", "A/B/C", "AAPL?x=1", "", "/AAPL", "AAPL/", "A B", "A-B",
        "A%2FB",
    ] {
        let error = wire::positions(&row(symbol)).expect_err("not a symbol");
        assert_eq!(
            (error.code(), error.to_string()),
            ("wrong_type", "field symbol has the wrong type".to_owned()),
            "{symbol:?} is refused before it can reach /v2/positions/{{symbol}}"
        );
    }
}

/// An order placed by notional amount names no share quantity (`qty: null`). One on the open-orders
/// page is external activity the owner can create on their own account, so it is ingested with
/// its filled quantity as the only quantity known, rather than failing the whole page and with it
/// the reconciliation (§11, #191 review, round 1).
#[test]
fn an_external_notional_order_on_the_open_orders_page_is_ingested() {
    let mut page: serde_json::Value =
        serde_json::from_slice(&body_of("open_orders_page", 0)).expect("valid JSON");
    let recorded = page.as_array().map_or(0, Vec::len);
    let mut notional = page
        .as_array()
        .and_then(|all| all.first())
        .cloned()
        .expect("the page records at least one order");
    if let Some(object) = notional.as_object_mut() {
        object.insert(
            "id".to_owned(),
            serde_json::json!("0f0f0f0f-0000-4000-8000-00000000abcd"),
        );
        object.insert(
            "client_order_id".to_owned(),
            serde_json::json!("owner-app-1"),
        );
        object.insert("qty".to_owned(), serde_json::Value::Null);
        object.insert("notional".to_owned(), serde_json::json!("100"));
        object.insert("filled_qty".to_owned(), serde_json::json!("0"));
    }
    if let Some(all) = page.as_array_mut() {
        all.push(notional);
    }
    let text = serde_json::to_vec(&page).expect("re-serialises");
    let orders = wire::open_orders(&text).expect("a notional order does not fail the page");
    assert_eq!(
        orders.len(),
        recorded + 1,
        "every order on the page is read"
    );
    let external = orders.last().expect("the notional order");
    assert_eq!(external.client_order_id.as_deref(), Some("owner-app-1"));
    assert_eq!(
        external.qty, external.filled_qty,
        "with no share quantity, its filled quantity is the only one known"
    );
    let mut missing: serde_json::Value =
        serde_json::from_slice(&body_of("open_orders_page", 0)).expect("valid JSON");
    if let Some(object) = missing
        .as_array_mut()
        .and_then(|all| all.first_mut())
        .and_then(serde_json::Value::as_object_mut)
    {
        object.remove("qty");
    }
    let text = serde_json::to_vec(&missing).expect("re-serialises");
    assert_eq!(
        wire::open_orders(&text).err().map(|e| e.code()),
        Some("missing_field"),
        "an order that omits qty altogether is still unreadable"
    );
}

#[tokio::test]
async fn a_submission_carries_our_client_order_id_on_the_wire() {
    let recorded = scenario("submit_limit_accepted");
    let sent = recorded
        .exchanges
        .first()
        .and_then(|exchange| exchange.body.clone())
        .expect("the recording carries the request body");
    assert!(
        sent.contains("\"client_order_id\":\"md-"),
        "the broker's own idempotency key is ours (E7-2 step 6): {sent}"
    );
    assert_eq!(
        recorded.exchanges.first().map(|e| e.method),
        Some(Method::Post)
    );
    let requested: serde_json::Value = serde_json::from_str(&sent).expect("canonical JSON");
    let answered = wire::order(&body_of("submit_limit_accepted", 0)).expect("the recording parses");
    assert_eq!(
        answered.client_order_id.as_deref(),
        requested.get("client_order_id").and_then(|v| v.as_str()),
        "the id the broker echoes is the id we sent, which is the whole of E7-2 step 6"
    );
}

#[tokio::test]
async fn the_broker_refusing_our_own_id_is_folded_as_already_submitted() {
    let (client, _transport) = client("submit_duplicate_client_order_id");
    let recorded = scenario("submit_duplicate_client_order_id");
    assert_eq!(
        recorded.exchanges.first().map(|e| e.status),
        Some(422),
        "the recording is what Alpaca actually answers for a repeated client_order_id"
    );
    let outcome = client
        .order_by_client_id("md-e144b97773a6f87c1978cc2831")
        .await
        .expect("a duplicate is an answer, not a failure");
    assert!(
        matches!(
            outcome,
            BrokerOutcome::DuplicateClientOrderId { .. } | BrokerOutcome::Order(_)
        ),
        "it means the order is already there (E7-2 step 6): {outcome:?}"
    );
    let _ = BrokerUnknown::Timeout;
}

#[tokio::test]
async fn an_absent_order_is_an_absence_and_never_an_error() {
    let (client, _transport) = client("order_by_client_id_absent");
    let outcome = client
        .order_by_client_id("md-never-submitted")
        .await
        .expect("a 404 is an answer");
    assert!(
        matches!(outcome, BrokerOutcome::Absent { .. }),
        "one absence is a fact the executor counts, not a failure: {outcome:?}"
    );
}

#[tokio::test]
async fn a_transport_failure_is_an_unknown_outcome_and_never_a_rejection() {
    let transport = FakeTransport::serving([Err(mandate_alpaca::TransportError::Timeout)]);
    let client = TradingClient::new(transport, FakeClock::default(), RetryPolicy::default());
    let error = common::answered(
        client
            .order_by_client_id("md-e144b97773a6f87c1978cc2831")
            .await,
    )
    .expect_err("a timeout is not an answer");
    assert_eq!(
        error.as_unknown(),
        Some(BrokerUnknown::Timeout),
        "an unknown outcome makes the executor query, never resubmit (interpretation 10)"
    );
}

#[test]
fn the_open_orders_page_parses_into_the_order_set() {
    let orders = wire::open_orders(&body_of("open_orders_page", 0)).expect("the recording parses");
    assert!(
        !orders.is_empty(),
        "the recording was taken with a resting order, so the page is not empty"
    );
    assert!(
        orders.iter().all(|order| order.client_order_id.is_some()),
        "every order on the page names a client order id, which is how §11 compares the set"
    );
}

#[test]
fn the_positions_source_parses() {
    let positions = wire::positions(&body_of("positions", 0)).expect("the recording parses");
    assert!(
        positions.is_empty(),
        "the recording was taken with a flat account, which is the honest empty case"
    );
    let held = br#"[{"asset_class":"us_equity","avg_entry_price":"150.10","qty":"-5","side":"short","symbol":"AAPL"},{"asset_class":"us_equity","avg_entry_price":"20","qty":"12.500000000","side":"long","symbol":"FRAC"}]"#;
    let positions = wire::positions(held).expect("two positions parse");
    assert_eq!(positions.len(), 2, "one per row, in the broker's order");
    let short = positions.first().expect("the first row");
    assert_eq!(short.instrument.as_str(), "AAPL");
    assert_eq!(
        short.qty,
        mandate_num::SignedQty::parse("-5").expect("canonical"),
        "a negative position is a signed quantity, never an absolute value"
    );
    assert_eq!(
        short.avg_entry_price,
        mandate_num::Price::parse("150.1").expect("canonical"),
        "the broker's `150.10` is the exact price 150.1: trailing zeros are text, not precision"
    );
    let fractional = positions.get(1).expect("the second row");
    assert_eq!(
        fractional.qty,
        mandate_num::SignedQty::parse("12.5").expect("canonical"),
        "`12.500000000` is 12.5 exactly"
    );
}

#[test]
fn broker_decimals_with_trailing_zeros_parse_into_the_exact_values() {
    let mut order: serde_json::Value =
        serde_json::from_slice(&body_of("submit_limit_accepted", 0)).expect("valid JSON");
    if let Some(object) = order.as_object_mut() {
        object.insert("limit_price".to_owned(), serde_json::json!("0.80"));
        object.insert("qty".to_owned(), serde_json::json!("10.00"));
        object.insert("filled_qty".to_owned(), serde_json::json!("0.0"));
    }
    let parsed = wire::order(&serde_json::to_vec(&order).expect("re-serialises"))
        .expect("an order with trailing zeros parses");
    assert_eq!(
        parsed.limit_price,
        Some(mandate_num::Price::parse("0.8").expect("canonical")),
        "the recording's `0.80` is the exact price 0.8"
    );
    assert_eq!(
        parsed.qty,
        mandate_num::Qty::parse("10").expect("canonical")
    );
    assert_eq!(parsed.filled_qty, mandate_num::Qty::ZERO);

    let fills = br#"[{"activity_type":"FILL","client_order_id":"md-bb11cc22dd33ee44ff5566aa77","cum_qty":"60","id":"20260926233000000::11111111-1111-4111-8111-111111111111","leaves_qty":"40","order_id":"e1111111-1111-4111-8111-111111111111","price":"150.10","qty":"60.0","side":"buy","symbol":"AAPL","transaction_time":"2026-09-26T23:22:30.000000000Z","type":"partial_fill"}]"#;
    let parsed = wire::activities(fills).expect("a fill with trailing zeros parses");
    let fill = parsed.first().expect("one fill");
    assert_eq!(
        fill.price,
        mandate_num::Price::parse("150.1").expect("canonical"),
        "`150.10` is 150.1 exactly"
    );
    assert_eq!(fill.qty, mandate_num::Qty::parse("60").expect("canonical"));

    let mut account: serde_json::Value =
        serde_json::from_slice(&body_of("account_active", 0)).expect("valid JSON");
    if let Some(object) = account.as_object_mut() {
        object.insert("cash".to_owned(), serde_json::json!("20000.50"));
        object.insert("buying_power".to_owned(), serde_json::json!("12000.00"));
    }
    let parsed = wire::account(&serde_json::to_vec(&account).expect("re-serialises"))
        .expect("an account with trailing zeros parses");
    assert_eq!(
        parsed.cash,
        mandate_num::Usd::parse("20000.5").expect("canonical")
    );
    assert_eq!(
        parsed.buying_power,
        mandate_num::Usd::parse("12000").expect("canonical")
    );
}

#[test]
fn the_account_source_parses_and_carries_no_account_number() {
    let account = wire::account(&body_of("account_active", 0)).expect("the recording parses");
    assert_eq!(account.status, "ACTIVE");
    assert!(!account.trading_blocked && !account.account_blocked);
    let rendered = format!("{account:?}");
    assert!(
        !rendered.contains("account_number") && !rendered.contains("pii:"),
        "the executor's account type has nowhere to put either (journal §6.4): {rendered}"
    );
}

#[test]
fn a_blocked_account_parses_into_the_restriction_signals() {
    let account = wire::account(&body_of("account_blocked", 0)).expect("the recording parses");
    assert!(
        account.trading_blocked && account.account_blocked,
        "§7.3's first row reads these flags, not a message"
    );
}

#[test]
fn the_activities_source_parses_fills_by_fill_id() {
    let fills = wire::activities(&body_of("partial_then_filled", 2)).expect("the fixture parses");
    assert_eq!(fills.len(), 2, "a partial and its completion");
    let ids: std::collections::BTreeSet<_> = fills.iter().map(|f| f.fill_id.0.clone()).collect();
    assert_eq!(
        ids.len(),
        2,
        "each fill has its own id, which is what makes a re-ingest idempotent (§11 step 2)"
    );
    let total: Vec<String> = fills.iter().map(|f| f.qty.to_string()).collect();
    assert_eq!(
        total,
        vec!["60".to_owned(), "40".to_owned()],
        "60 then 40 of 100"
    );
}

#[test]
fn a_reject_parses_into_the_restriction_table_signals() {
    let reject = wire::reject(422, &body_of("submit_rejected", 0)).expect("the recording parses");
    assert_eq!(reject.http_status, 422);
    assert!(
        !reject.message.is_empty(),
        "§7.3's table matches against the broker's own message"
    );
}

#[test]
fn a_bracket_shares_one_tif_and_carries_no_extended_hours() {
    let recorded = scenario("submit_bracket_accepted");
    let sent = recorded
        .exchanges
        .first()
        .and_then(|exchange| exchange.body.clone())
        .expect("the recording carries the request body");
    assert!(sent.contains("\"order_class\":\"bracket\""), "{sent}");
    assert!(
        sent.contains("\"time_in_force\":\"gtc\""),
        "one shared TIF (§5.2): {sent}"
    );
    assert!(
        !sent.contains("\"extended_hours\":true"),
        "OCO and bracket orders take no extended hours (§5.2): {sent}"
    );
    let parsed = wire::order(&body_of("submit_bracket_accepted", 0)).expect("the recording parses");
    assert_eq!(parsed.legs.len(), 2, "a take-profit and a stop leg");
}

/// DEC-878 item 1, from the recording: Alpaca nests a bracket's two legs under the entry and names
/// each with its own `client_order_id`, a UUID, never one of ours, so the read keeps each leg
/// whole (side, prices, status) for reconciliation to find it through its parent, by side and
/// status (§11), and never by that id.
#[test]
fn a_brackets_legs_are_read_whole_and_named_by_the_broker() {
    let parsed = wire::order(&body_of("submit_bracket_accepted", 0)).expect("the recording parses");
    let legs: Vec<(Side, Option<String>, Option<String>, &str)> = parsed
        .legs
        .iter()
        .map(|leg| {
            (
                leg.side,
                leg.limit_price.map(|p| p.to_string()),
                leg.stop_price.map(|p| p.to_string()),
                leg.status.as_str(),
            )
        })
        .collect();
    assert_eq!(
        legs,
        vec![
            (Side::Sell, Some("999".to_owned()), None, "held"),
            (Side::Sell, None, Some("0.8".to_owned()), "held"),
        ],
        "the take-profit and the stop, each a sell, held until the entry fills (§5.4)"
    );
    let named: Vec<Option<&str>> = parsed
        .legs
        .iter()
        .map(|leg| leg.client_order_id.as_deref())
        .collect();
    assert_eq!(
        named,
        vec![
            Some("86942a5e-a11d-4464-872b-05f671c148ac"),
            Some("94969c96-b018-47e9-a6fc-c3faa85b65af"),
        ],
        "the broker names each leg itself; neither is the entry's id or one of ours"
    );
    assert_eq!(
        parsed.client_order_id.as_deref(),
        Some("md-daea915c1fc0042bcc19a4caed"),
        "the bracket's parent is the entry, under the entry's own id"
    );
}

#[test]
fn an_oco_for_a_filled_quantity_carries_the_brackets_prices() {
    let recorded = scenario("submit_oco_accepted");
    let sent = recorded
        .exchanges
        .first()
        .and_then(|exchange| exchange.body.clone())
        .expect("the fixture carries the request body");
    assert!(sent.contains("\"order_class\":\"oco\""), "{sent}");
    assert!(
        sent.contains("\"qty\":\"60\""),
        "for exactly the filled quantity: {sent}"
    );
    assert!(
        sent.contains("\"limit_price\":\"170\"") && sent.contains("\"stop_price\":\"140\""),
        "at the bracket's prices (§5.4): {sent}"
    );
    let answered = wire::order(&body_of("submit_oco_accepted", 0)).expect("the recording parses");
    assert_eq!(
        answered.qty.to_string(),
        "60",
        "for exactly the filled quantity"
    );
    assert_eq!(
        answered.legs.len(),
        2,
        "an OCO is one order with two legs: a take-profit and a stop"
    );
}

#[test]
fn a_crypto_stop_limit_is_one_simple_gtc_order_with_its_limit_derived() {
    let recorded = scenario("submit_crypto_stop_limit");
    let sent = recorded
        .exchanges
        .first()
        .and_then(|exchange| exchange.body.clone())
        .expect("the fixture carries the request body");
    assert!(
        sent.contains("\"order_class\":\"simple\""),
        "crypto is simple orders only: {sent}"
    );
    assert!(sent.contains("\"type\":\"stop_limit\""), "{sent}");
    assert!(sent.contains("\"time_in_force\":\"gtc\""), "{sent}");
    assert!(
        sent.contains("\"stop_price\":\"54000\"") && sent.contains("\"limit_price\":\"53730\""),
        "limit = stop x (1 - crypto_stop_limit_offset) = 54000 x 0.995 (DEC-36): {sent}"
    );
    let answered =
        wire::order(&body_of("submit_crypto_stop_limit", 0)).expect("the recording parses");
    assert_eq!(
        answered.stop_price.map(|p| p.to_string()),
        Some("54000".to_owned()),
        "and the stop comes back as exact text, never through a float (ES-23)"
    );
    assert_eq!(
        answered.limit_price.map(|p| p.to_string()),
        Some("53730".to_owned())
    );
    assert!(
        answered.legs.is_empty(),
        "crypto takes simple orders only (§5.2)"
    );
}

#[test]
fn a_cancel_is_confirmed_by_the_broker_and_not_by_the_request() {
    let recorded = scenario("cancel_confirmed");
    assert_eq!(
        recorded.exchanges.first().map(|e| e.status),
        Some(204),
        "the DELETE answers 204 with no body: accepting a cancel is not confirming it"
    );
    let confirmed = wire::order(&body_of("cancel_confirmed", 1)).expect("the follow-up parses");
    assert_eq!(
        confirmed.status, "canceled",
        "the confirmation is the order's own state (§5.4, planted bug 5)"
    );
}

#[test]
fn a_cancel_of_an_order_that_filled_first_is_not_a_confirmation() {
    let recorded = scenario("cancel_rejected_already_filled");
    assert_eq!(recorded.exchanges.first().map(|e| e.status), Some(422));
    let after = wire::order(&body_of("cancel_rejected_already_filled", 1)).expect("parses");
    assert_eq!(
        after.status, "filled",
        "§5.7's `PendingCancel --> Filled: filled first` row"
    );
}

#[test]
fn a_broker_initiated_replace_links_the_new_order_to_the_old() {
    let pending = wire::order(&body_of("replace_pending_then_replaced", 0)).expect("parses");
    assert_eq!(
        pending.status, "pending_replace",
        "the old order stays live"
    );
    let replaced = wire::order(&body_of("replace_pending_then_replaced", 1)).expect("parses");
    assert_eq!(replaced.status, "replaced");
    assert!(
        replaced.replaced_by_broker_order_id.is_some(),
        "the new order is linked, so the reservation can pass to it (interpretation 26)"
    );
    let fresh = wire::order(&body_of("replace_pending_then_replaced", 2)).expect("parses");
    assert_eq!(fresh.status, "new");
}

#[test]
fn a_late_fill_names_an_order_that_is_already_terminal() {
    let terminal = wire::order(&body_of("late_fill_after_terminal", 0)).expect("parses");
    assert_eq!(terminal.status, "canceled");
    let fills = wire::activities(&body_of("late_fill_after_terminal", 1)).expect("parses");
    assert_eq!(
        fills.len(),
        1,
        "one fill arrives after the terminal state (§5.7)"
    );
}

#[test]
fn the_account_wide_endpoints_are_the_two_the_spec_names() {
    let cancel_all = scenario("cancel_all_account_scope");
    assert_eq!(
        cancel_all
            .exchanges
            .first()
            .map(|e| e.path_and_query.as_str()),
        Some("/v2/orders"),
        "the broker's cancel-all is a DELETE on the collection (§5.5)"
    );
    assert_eq!(
        cancel_all.exchanges.first().map(|e| e.method),
        Some(Method::Delete)
    );
    let close = scenario("close_position_account_scope");
    assert!(
        close
            .exchanges
            .first()
            .is_some_and(|e| e.path_and_query.starts_with("/v2/positions/")),
        "and close-position is per instrument"
    );
    let after_cancel_all =
        wire::open_orders(&body_of("cancel_all_account_scope", 1)).expect("the fixture parses");
    assert!(
        after_cancel_all.is_empty(),
        "cancel-all leaves no open order, `Unknown` ones included (§5.5)"
    );
    let sell =
        wire::order(&body_of("close_position_account_scope", 0)).expect("the fixture parses");
    assert_eq!(
        sell.side,
        mandate_accounting::Side::Sell,
        "close-position sells the whole position"
    );
}

#[test]
fn a_broker_exchange_is_recorded_with_its_credentials_redacted() {
    let request = HttpRequest::new(
        Method::Post,
        "/v2/orders",
        Some("{\"client_order_id\":\"md-1\"}".to_owned()),
    )
    .expect("a submission is one of the endpoints");
    let recorded = record::request(&request).expect("the request records");
    let rendered = format!("{recorded:?}");
    assert!(
        !rendered.contains("APCA")
            && !rendered.contains(common::SENTINEL_KEY_ID)
            && !rendered.contains(common::SENTINEL_SECRET),
        "the authorisation headers are removed before anything is hashed or stored: {rendered}"
    );
    assert_eq!(recorded.direction, mandate_alpaca::Direction::Request);
    assert_eq!(
        recorded.endpoint, "/v2/orders",
        "the record names the endpoint it went to"
    );
    assert_eq!(
        recorded.body,
        mandate_alpaca::RecordedBody::Inline("{\"client_order_id\":\"md-1\"}".to_owned()),
        "and keeps the body, which carries no personal data, as the audit record trading §13 asks for"
    );
    assert!(recorded.pii_refs.is_empty(), "nothing in it was personal");
}

#[test]
fn an_account_body_is_recorded_with_its_account_number_replaced_by_a_pii_ref() {
    let response = mandate_alpaca::Response {
        status: 200,
        body: br#"{"account_number":"PA3ABCDEFGHI","id":"8f1c5b2a-0000-4000-8000-000000000000","status":"ACTIVE"}"#
            .to_vec(),
    };
    let recorded = record::response("/v2/account", &response).expect("the response records");
    assert!(
        !recorded.pii_refs.is_empty(),
        "one `pii_refs` entry per personal-data field replaced (journal §6.4)"
    );
    let rendered = format!("{:?}", recorded.body);
    assert!(
        !rendered.contains("PA3ABCDEFGHI")
            && !rendered.contains("8f1c5b2a-0000-4000-8000-000000000000"),
        "neither value has ever been in the bytes that reach the journal (planted bug 21): \
         {rendered}"
    );
}

#[test]
fn a_large_exchange_is_recorded_by_artifact_reference() {
    let filler = "x".repeat(mandate_alpaca::INLINE_LIMIT.saturating_add(1));
    let response = mandate_alpaca::Response {
        status: 200,
        body: format!(
            "{{\"account_number\":\"PA3ABCDEFGHI\",\"message\":\"{filler}\",\"unread\":1}}"
        )
        .into_bytes(),
    };
    let recorded = record::response("/v2/orders", &response).expect("the response records");
    let mandate_alpaca::RecordedBody::Artifact { digest, bytes } = &recorded.body else {
        panic!(
            "above the inline limit the payload carries a `sha256:` reference (journal §6.3, \
             DEC-107): {:?}",
            recorded.endpoint
        );
    };
    let hex = digest
        .strip_prefix("sha256:")
        .unwrap_or_else(|| panic!("the reference is a sha256 digest: {digest}"));
    assert!(
        hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "of 64 hex digits: {digest}"
    );
    assert!(
        bytes.len() > mandate_alpaca::INLINE_LIMIT,
        "and the redacted bytes the digest names travel with it, for the shell to store"
    );
    assert_eq!(
        digest,
        &format!("sha256:{}", mandate_canon::Digest::of(bytes).to_hex()),
        "the reference names the redacted bytes, never the body as the broker sent it \
         (interpretation 24, DEC-133 item 24)"
    );
    assert!(
        !String::from_utf8_lossy(bytes).contains("PA3ABCDEFGHI"),
        "and those bytes have never held the account number"
    );
}

#[test]
fn an_account_id_is_personal_data_wherever_it_appears() {
    let response = mandate_alpaca::Response {
        status: 200,
        body: br#"{"account_id":"acct-7f3e","status":"filled"}"#.to_vec(),
    };
    let recorded = record::response("/v2/orders", &response).expect("the response records");
    assert!(
        !format!("{:?}", recorded.body).contains("acct-7f3e"),
        "an `account_id` field is replaced, not kept (DEC-133 item 33): {:?}",
        recorded.body
    );
    assert_eq!(recorded.pii_refs.len(), 1, "{:?}", recorded.pii_refs);
}

#[test]
fn an_account_nested_in_an_array_is_redacted() {
    let response = mandate_alpaca::Response {
        status: 200,
        body: br#"{"legs":[{"account_number":"PA3INARRAY","id":"acct-in-array","symbol":"AAPL"}]}"#
            .to_vec(),
    };
    let recorded = record::response("/v2/orders", &response).expect("the response records");
    let rendered = format!("{:?}", recorded.body);
    assert!(
        !rendered.contains("PA3INARRAY") && !rendered.contains("acct-in-array"),
        "an account inside an array is redacted like one at the top: {rendered}"
    );
    assert_eq!(recorded.pii_refs.len(), 2, "{:?}", recorded.pii_refs);
}

#[test]
fn an_account_nested_in_an_object_is_redacted() {
    let response = mandate_alpaca::Response {
        status: 200,
        body: br#"{"take_profit":{"account_number":"PA3INOBJECT","id":"acct-in-object"}}"#.to_vec(),
    };
    let recorded = record::response("/v2/orders", &response).expect("the response records");
    let rendered = format!("{:?}", recorded.body);
    assert!(
        !rendered.contains("PA3INOBJECT") && !rendered.contains("acct-in-object"),
        "an account inside an object is redacted like one at the top: {rendered}"
    );
    assert_eq!(recorded.pii_refs.len(), 2, "{:?}", recorded.pii_refs);
}

#[test]
fn the_pii_refs_are_a_sorted_set() {
    let response = mandate_alpaca::Response {
        status: 200,
        body: br#"{"account_number":"PA3SORTED","id":"acct-sorted","legs":[{"account_id":"acct-leg"}]}"#
            .to_vec(),
    };
    let recorded = record::response("/v2/account", &response).expect("the response records");
    let mut sorted = recorded.pii_refs.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        recorded.pii_refs, sorted,
        "journal §3's `pii_refs` is a sorted set, whatever order the fields were met in"
    );
    assert_eq!(recorded.pii_refs.len(), 3);
}

/// A client serving scripted replies, and the transport that records what it sent.
fn serving(
    replies: Vec<Result<mandate_alpaca::Response, mandate_alpaca::TransportError>>,
) -> (TradingClient<FakeTransport, FakeClock>, FakeTransport) {
    let transport = FakeTransport::serving(replies);
    (
        TradingClient::new(
            transport.clone(),
            FakeClock::default(),
            RetryPolicy::default(),
        ),
        transport,
    )
}

/// What was sent, as `METHOD path` lines, so a test pins the request order at a glance.
fn lines(transport: &FakeTransport) -> Vec<String> {
    transport
        .sent()
        .iter()
        .map(|request| format!("{} {}", request.method().as_str(), request.path_and_query()))
        .collect()
}

fn client_order_id(raw: &str) -> mandate_executor::ClientOrderId {
    mandate_executor::ClientOrderId::parse(raw)
        .unwrap_or_else(|e| panic!("{raw} is an id this platform derives: {e}"))
}

fn exact_price(text: &str) -> mandate_num::Price {
    mandate_num::Price::parse(text).unwrap_or_else(|e| panic!("price {text}: {e}"))
}

fn exact_qty(text: &str) -> mandate_num::Qty {
    mandate_num::Qty::parse(text).unwrap_or_else(|e| panic!("qty {text}: {e}"))
}

fn instrument(name: &str) -> mandate_accounting::InstrumentId {
    mandate_accounting::InstrumentId::new(name).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The limit order `submit_limit_accepted` recorded: one share of AAPL, a buy limit far from the
/// market, GTC, regular session only.
fn recorded_limit_order() -> mandate_executor::SubmitOrder {
    mandate_executor::SubmitOrder {
        client_order_id: client_order_id("md-e144b97773a6f87c1978cc2831"),
        instrument: instrument("AAPL"),
        side: mandate_accounting::Side::Buy,
        qty: exact_qty("1"),
        order_type: mandate_executor::OrderType::Limit,
        tif: mandate_executor::TimeInForce::Gtc,
        limit_price: Some(exact_price("1")),
        stop_price: None,
        bracket: None,
        oco: None,
        extended_hours: false,
        purpose: mandate_executor::Purpose::Open,
    }
}

/// The request body a scenario recorded, as JSON with its decimals made canonical.
fn recorded_body(name: &str) -> serde_json::Value {
    let text = scenario(name)
        .exchanges
        .first()
        .and_then(|exchange| exchange.body.clone())
        .unwrap_or_else(|| panic!("{name} records its request body"));
    let value: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
    common::canonical_decimals(&value)
}

fn built_body(order: &mandate_executor::SubmitOrder) -> serde_json::Value {
    let text = wire::submission_body(order).expect("the body builds");
    let value: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON: {e}: {text}"));
    assert_eq!(
        serde_json::to_string(&value).ok().as_deref(),
        Some(text.as_str()),
        "the body is canonical: sorted keys, no whitespace, so a recorded line pins it byte for byte"
    );
    common::canonical_decimals(&value)
}

#[test]
fn a_simple_equity_limit_body_is_the_recorded_one_with_extended_hours_false() {
    let built = built_body(&recorded_limit_order());
    assert_eq!(
        built,
        recorded_body("submit_limit_accepted"),
        "the body Alpaca paper accepted, field for field, with our client order id (E7-2)"
    );
    assert_eq!(
        built.get("extended_hours"),
        Some(&serde_json::json!(false)),
        "a simple equity limit says so: regular session only (§5.1)"
    );
}

#[test]
fn a_bracket_body_is_the_recorded_one_with_no_extended_hours() {
    let order = mandate_executor::SubmitOrder {
        client_order_id: client_order_id("md-daea915c1fc0042bcc19a4caed"),
        bracket: Some(mandate_executor::BracketLegs {
            take_profit: exact_price("999"),
            stop: exact_price("0.8"),
        }),
        ..recorded_limit_order()
    };
    let built = built_body(&order);
    assert_eq!(built, recorded_body("submit_bracket_accepted"));
    assert!(
        built.get("extended_hours").is_none(),
        "a bracket carries no extended_hours at all (§5.2)"
    );
}

#[test]
fn an_oco_body_has_no_top_level_limit_and_no_extended_hours() {
    let order = mandate_executor::SubmitOrder {
        client_order_id: client_order_id(
            "md-e144b97773a6f87c1978cc2831-p01JPROTECTION0000000000001",
        ),
        side: mandate_accounting::Side::Sell,
        qty: exact_qty("60"),
        limit_price: None,
        oco: Some(mandate_executor::OcoLegs {
            take_profit: exact_price("170"),
            stop: exact_price("140"),
            qty: exact_qty("60"),
        }),
        purpose: mandate_executor::Purpose::Protective,
        ..recorded_limit_order()
    };
    let built = built_body(&order);
    assert_eq!(built, recorded_body("submit_oco_accepted"));
    assert!(
        built.get("limit_price").is_none() && built.get("extended_hours").is_none(),
        "an OCO's prices are its legs' only: take_profit.limit_price and stop_loss.stop_price"
    );
    let two_quantities = mandate_executor::SubmitOrder {
        oco: Some(mandate_executor::OcoLegs {
            take_profit: exact_price("170"),
            stop: exact_price("140"),
            qty: exact_qty("40"),
        }),
        ..order.clone()
    };
    assert_eq!(
        wire::submission_body(&two_quantities)
            .err()
            .map(|e| e.code()),
        Some("wrong_type"),
        "an OCO whose legs name 40 while the order names 60 is refused, never sent with either \
         quantity: §5.4's OCO is for the filled quantity, and a guess could over-sell"
    );
    let with_a_limit = mandate_executor::SubmitOrder {
        limit_price: Some(exact_price("170")),
        ..order.clone()
    };
    assert_eq!(
        wire::submission_body(&with_a_limit).err().map(|e| e.code()),
        Some("wrong_type"),
        "an OCO carrying a top-level limit is refused: its prices are its legs' only"
    );
    let with_a_stop = mandate_executor::SubmitOrder {
        stop_price: Some(exact_price("140")),
        ..order
    };
    assert_eq!(
        wire::submission_body(&with_a_stop).err().map(|e| e.code()),
        Some("wrong_type"),
        "an OCO carrying a top-level stop is refused the same way"
    );
}

#[test]
fn a_crypto_stop_limit_body_is_simple_with_no_extended_hours() {
    let order = mandate_executor::SubmitOrder {
        client_order_id: client_order_id("md-c7c0b6a5d2e14f8b9a3c5d7e11"),
        instrument: instrument("BTC/USD"),
        side: mandate_accounting::Side::Sell,
        qty: exact_qty("0.5"),
        order_type: mandate_executor::OrderType::StopLimit,
        limit_price: Some(exact_price("53730")),
        stop_price: Some(exact_price("54000")),
        purpose: mandate_executor::Purpose::Protective,
        ..recorded_limit_order()
    };
    let built = built_body(&order);
    assert_eq!(built, recorded_body("submit_crypto_stop_limit"));
    assert_eq!(built.get("order_class"), Some(&serde_json::json!("simple")));
    assert!(
        built.get("extended_hours").is_none(),
        "crypto trades around the clock and takes no extended_hours field"
    );
}

#[tokio::test]
async fn a_submit_posts_the_body_and_folds_the_acceptance() {
    let (client, transport) = serving(vec![common::reply(200, "submit_limit_accepted", 0)]);
    let order = recorded_limit_order();
    let outcome = client
        .call_one(&mandate_executor::BrokerRequest::Submit(order.clone()))
        .await
        .expect("an accepted order is an answer");
    let sent = transport.sent();
    assert_eq!(lines(&transport), vec!["POST /v2/orders".to_owned()]);
    assert_eq!(
        sent.first()
            .and_then(|request| request.body())
            .map(str::to_owned),
        Some(wire::submission_body(&order).expect("the body builds")),
        "the bytes sent are the submission body, nothing added"
    );
    let mandate_executor::BrokerOutcome::Submitted(answered) = outcome else {
        panic!("a 200 on submit is Submitted: {outcome:?}");
    };
    assert_eq!(
        answered.client_order_id.as_deref(),
        Some("md-e144b97773a6f87c1978cc2831")
    );
    assert_eq!(answered.status, "accepted");
}

#[tokio::test]
async fn a_rejected_submission_is_a_rejection_with_its_status_and_message() {
    let (client, transport) = serving(vec![common::reply(422, "submit_rejected", 0)]);
    let outcome = client
        .call_one(&mandate_executor::BrokerRequest::Submit(
            recorded_limit_order(),
        ))
        .await
        .expect("a rejection is an answer");
    assert_eq!(lines(&transport), vec!["POST /v2/orders".to_owned()]);
    let mandate_executor::BrokerOutcome::Rejected(reject) = outcome else {
        panic!("a 422 that is not a duplicate is Rejected: {outcome:?}");
    };
    assert_eq!(reject.http_status, 422);
    assert_eq!(reject.message, "qty must be > 0");
    assert_eq!(
        reject.client_order_id.as_deref(),
        Some("md-e144b97773a6f87c1978cc2831"),
        "the reject names the order it refused"
    );
}

#[tokio::test]
async fn a_duplicate_client_order_id_on_submit_is_already_submitted() {
    let (client, _transport) = serving(vec![common::reply(
        422,
        "submit_duplicate_client_order_id",
        0,
    )]);
    let outcome = client
        .call_one(&mandate_executor::BrokerRequest::Submit(
            recorded_limit_order(),
        ))
        .await
        .expect("a duplicate is an answer");
    assert_eq!(
        outcome,
        mandate_executor::BrokerOutcome::DuplicateClientOrderId {
            client_order_id: "md-e144b97773a6f87c1978cc2831".to_owned()
        },
        "the broker refusing our own id means the order is already there (E7-2 step 6)"
    );
}

#[tokio::test]
async fn a_5xx_on_submit_is_an_unknown_outcome_never_a_rejection() {
    let (client, transport) = serving(vec![common::inline(503, "{\"message\":\"unavailable\"}")]);
    let error = common::answered(
        client
            .call_one(&mandate_executor::BrokerRequest::Submit(
                recorded_limit_order(),
            ))
            .await,
    )
    .expect_err("a 5xx after the request left is not a settled answer");
    assert_eq!(
        error.as_unknown(),
        Some(BrokerUnknown::Ambiguous),
        "the broker may have accepted it: the executor must query, never resubmit (interpretation 10)"
    );
    assert_eq!(error.to_connector().code(), "unknown_outcome");
    assert_eq!(
        transport.sent().len(),
        1,
        "and the client does not retry a submission on its own: that is the executor's to resolve"
    );
}

/// The three requests of a cancel the broker took: the lookup by our id, the `DELETE` by the
/// broker's, and DEC-867 item 3's read-back by our id.
const CANCEL_THEN_READ_BACK: [&str; 3] = [
    "GET /v2/orders:by_client_order_id?client_order_id=md-e144b97773a6f87c1978cc2831",
    "DELETE /v2/orders/e02fc2d2-0ff3-444f-a0ab-6253613302fe",
    "GET /v2/orders:by_client_order_id?client_order_id=md-e144b97773a6f87c1978cc2831",
];

fn cancel_recorded_order() -> mandate_executor::BrokerRequest {
    mandate_executor::BrokerRequest::Cancel {
        client_order_id: client_order_id("md-e144b97773a6f87c1978cc2831"),
    }
}

/// The recorded `order_by_client_id_found` record, showing `status` with `filled_qty` filled.
fn found_showing(
    status: &str,
    filled_qty: &str,
) -> Result<mandate_alpaca::Response, mandate_alpaca::TransportError> {
    let body = common::body("order_by_client_id_found", 0);
    let mut record: serde_json::Value =
        serde_json::from_slice(&body).unwrap_or_else(|e| panic!("the recorded order parses: {e}"));
    record["status"] = serde_json::json!(status);
    record["filled_qty"] = serde_json::json!(filled_qty);
    common::inline(200, &record.to_string())
}

/// DEC-867 items 1 and 3, trading spec §5.7's `PendingCancel --> Canceled: confirmed`: Alpaca
/// cancels by the broker's order id, which `BrokerRequest::Cancel` does not carry, so the client
/// looks it up by our id first; an accepted `DELETE` is only a request taken, so the client reads
/// the order back once, and the recorded `canceled` it reads is the confirmation.
#[tokio::test]
async fn a_cancel_looks_the_order_up_then_deletes_it_by_the_broker_id() {
    for deleted in [204, 200] {
        let (client, transport) = serving(vec![
            common::reply(200, "order_by_client_id_found", 0),
            common::inline(deleted, ""),
            common::reply(200, "cancel_confirmed", 1),
        ]);
        let outcome = client
            .call_one(&cancel_recorded_order())
            .await
            .unwrap_or_else(|e| panic!("{deleted}: a confirmed cancel is an answer: {e}"));
        assert_eq!(lines(&transport), CANCEL_THEN_READ_BACK, "{deleted}");
        assert_eq!(
            outcome,
            mandate_executor::BrokerOutcome::CancelAccepted {
                client_order_id: "md-e144b97773a6f87c1978cc2831".to_owned()
            },
            "{deleted}"
        );
    }
}

/// DEC-867 items 2 and 3: a `DELETE` Alpaca accepted whose order the read-back does not show
/// `canceled`, with a `204` or a `200`, is answered with that order, in the status the read shows,
/// so the executor keeps the cancel unconfirmed (§5.4) and waits for Alpaca's own `canceled`. An
/// order in `pending_cancel` can still fill.
#[tokio::test]
async fn an_accepted_cancel_whose_order_is_not_yet_canceled_answers_the_order_read_back() {
    let shown = [
        ("pending_cancel", "0"),
        ("accepted", "0"),
        ("new", "0"),
        ("partially_filled", "0.4"),
    ];
    for ((status, filled), deleted) in shown.into_iter().flat_map(|s| [(s, 204), (s, 200)]) {
        let (client, transport) = serving(vec![
            common::reply(200, "order_by_client_id_found", 0),
            common::inline(deleted, ""),
            found_showing(status, filled),
        ]);
        let outcome = client
            .call_one(&cancel_recorded_order())
            .await
            .unwrap_or_else(|e| panic!("{status}: the read-back is an answer: {e}"));
        assert!(
            !matches!(outcome, BrokerOutcome::CancelAccepted { .. }),
            "{status}: an accepted DELETE is not a confirmation: {outcome:?}"
        );
        let BrokerOutcome::Order(order) = outcome else {
            panic!("{status}: {outcome:?}")
        };
        assert_eq!(order.status, status);
        assert_eq!(
            order.broker_order_id,
            "e02fc2d2-0ff3-444f-a0ab-6253613302fe"
        );
        assert_eq!(
            order.client_order_id.as_deref(),
            Some("md-e144b97773a6f87c1978cc2831")
        );
        assert_eq!(
            (order.qty, order.filled_qty),
            (exact_qty("1"), exact_qty(filled)),
            "{status}"
        );
        assert_eq!(lines(&transport), CANCEL_THEN_READ_BACK, "{status}");
    }
}

/// DEC-867 item 3: a read-back after an accepted `DELETE` that times out, is throttled or meets a
/// failing broker says nothing about the order, so the cancel's outcome is unknown, never
/// `CancelAccepted`, and nothing is deleted again.
#[tokio::test]
async fn a_failed_read_back_after_an_accepted_cancel_is_unknown_never_confirmed() {
    let failed = [
        ("timeout", Err(mandate_alpaca::TransportError::Timeout)),
        (
            "429",
            common::inline(429, "{\"message\":\"too many requests\"}"),
        ),
        ("503", common::inline(503, "{\"message\":\"unavailable\"}")),
    ];
    for (name, read_back) in failed {
        let (client, transport) = serving(vec![
            common::reply(200, "order_by_client_id_found", 0),
            common::inline(204, ""),
            read_back,
        ]);
        let error =
            common::answered(client.call_one(&cancel_recorded_order()).await).expect_err(name);
        assert!(
            matches!(
                error.to_connector(),
                mandate_executor::ConnectorError::Unknown(_)
            ),
            "{name}: {error:?}"
        );
        assert_eq!(lines(&transport), CANCEL_THEN_READ_BACK, "{name}");
    }
}

#[tokio::test]
async fn a_cancel_of_an_order_the_broker_does_not_have_is_an_absence() {
    let (client, transport) = serving(vec![common::reply(404, "order_by_client_id_absent", 0)]);
    let outcome = client
        .call_one(&mandate_executor::BrokerRequest::Cancel {
            client_order_id: client_order_id("md-e144b97773a6f87c1978cc2831"),
        })
        .await
        .expect("a 404 is an answer");
    assert_eq!(
        outcome,
        mandate_executor::BrokerOutcome::Absent {
            client_order_id: "md-e144b97773a6f87c1978cc2831".to_owned()
        }
    );
    assert_eq!(
        transport.sent().len(),
        1,
        "and nothing is deleted when there is nothing to delete"
    );
}

#[tokio::test]
async fn a_cancel_refused_as_not_cancelable_reads_the_order_back() {
    let (client, transport) = serving(vec![
        common::reply(200, "order_by_client_id_found", 0),
        common::reply(422, "cancel_rejected_already_filled", 0),
        common::reply(200, "cancel_rejected_already_filled", 1),
    ]);
    let outcome = client
        .call_one(&mandate_executor::BrokerRequest::Cancel {
            client_order_id: client_order_id("md-e144b97773a6f87c1978cc2831"),
        })
        .await
        .expect("a filled-first cancel is an answer");
    assert_eq!(
        lines(&transport),
        vec![
            "GET /v2/orders:by_client_order_id?client_order_id=md-e144b97773a6f87c1978cc2831"
                .to_owned(),
            "DELETE /v2/orders/e02fc2d2-0ff3-444f-a0ab-6253613302fe".to_owned(),
            "GET /v2/orders:by_client_order_id?client_order_id=md-e144b97773a6f87c1978cc2831"
                .to_owned(),
        ]
    );
    let mandate_executor::BrokerOutcome::Order(order) = outcome else {
        panic!("a 422 `not cancelable` is answered by the order's own state: {outcome:?}");
    };
    assert_eq!(
        order.status, "filled",
        "§5.7's `PendingCancel --> Filled: filled first` row, never a confirmation"
    );
}

/// The `submit_bracket_accepted` entry, as Alpaca lists it once the bracket fills completely: the
/// parent under the entry's own `client_order_id`, `filled`, with its two sell legs still nested
/// under it (the recording's own ids kept — each leg's broker `id` and broker-named
/// `client_order_id` — so the listing differs from the recording by the fill alone; DEC-878
/// item 1: no broker order carries the placement's handle).
fn filled_bracket_entry() -> serde_json::Value {
    let mut parent: serde_json::Value =
        serde_json::from_slice(&common::body("submit_bracket_accepted", 0))
            .unwrap_or_else(|e| panic!("the recorded bracket parses: {e}"));
    parent["status"] = serde_json::json!("filled");
    parent["filled_at"] = serde_json::json!("2026-09-26T23:21:11.63400562Z");
    parent["filled_avg_price"] = serde_json::json!("1");
    parent["filled_qty"] = serde_json::json!("1");
    parent
}

/// The recording's entry, under the entry's own `client_order_id`: the id Alpaca holds, which
/// the placement's handle names before its last `-p` (trading-domain spec §2.3, DEC-878 item 1).
const BRACKET_ENTRY: &str = "md-daea915c1fc0042bcc19a4caed";
/// The placement's platform handle, `{entry}-p{record}` (§2.3, DEC-160 item 3): the name no
/// Alpaca order carries, so a request that asks by it is answered by the broker's own 404.
const BRACKET_PLACEMENT: &str = "md-daea915c1fc0042bcc19a4caed-p1";
/// The take-profit leg's broker order id in the recording: the leg the platform names
/// `{placement}-tp` and Alpaca names with its own UUID (DEC-878).
const TAKE_PROFIT_LEG_ID: &str = "fe22b5be-837b-4684-b222-0090fdd838cc";
/// The stop leg's broker order id in the recording, the platform's `{placement}-sl`.
const STOP_LEG_ID: &str = "30ab5fda-7d66-4263-82cf-2a1d40bf404f";

/// The paper host's answers on a filled bracket's cancel path, dispatched by what was asked
/// rather than scripted into a sequence, so a test pins which requests exist, not their order.
///
/// The open-orders read (`nested=true`) answers the page a filled bracket lists; a read by a
/// `client_order_id` Alpaca holds answers that order, and any other name — the placement's
/// handle included, which DEC-878 item 1 keeps the platform's own and never a broker id —
/// answers the recorded 404 (`order_by_client_id_absent`); a `DELETE` by an order id is taken
/// and answered empty, as the recorded `cancel_confirmed` DELETE is; and a read by an order id
/// answers the order `canceled`, the read-back DEC-867 item 3 confirms a cancel by.
#[derive(Clone)]
struct FilledBracketHost {
    open_orders: String,
    entry: Option<String>,
    canceled_leg: String,
    absent: String,
    sent: Arc<Mutex<Vec<String>>>,
}

impl FilledBracketHost {
    /// On a broker whose open-orders page is `listing`; a read by the entry's
    /// `client_order_id` answers it only while the page lists it, as no read can name an order
    /// the page does not hold.
    fn answering(listing: serde_json::Value) -> Self {
        let page = listing
            .as_array()
            .unwrap_or_else(|| panic!("the open-orders page is a list: {listing}"));
        let entry = page
            .iter()
            .find(|open| {
                open.get("client_order_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(BRACKET_ENTRY)
            })
            .cloned();
        let mut canceled = filled_bracket_entry()["legs"][0].clone();
        canceled["status"] = serde_json::json!("canceled");
        Self {
            open_orders: listing.to_string(),
            entry: entry.map(|found| found.to_string()),
            canceled_leg: canceled.to_string(),
            absent: String::from_utf8(common::body("order_by_client_id_absent", 0))
                .unwrap_or_else(|e| panic!("the recorded absence is text: {e}")),
            sent: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Everything the client sent, as `METHOD path` lines, in order.
    fn sent_lines(&self) -> Vec<String> {
        let mut sent = match self.sent.lock() {
            Ok(sent) => sent,
            Err(poisoned) => poisoned.into_inner(),
        };
        std::mem::take(&mut *sent)
    }
}

impl TradingTransport for FilledBracketHost {
    async fn send(&self, request: &HttpRequest) -> Result<Response, TransportError> {
        let path = request.path_and_query();
        {
            let mut sent = match self.sent.lock() {
                Ok(sent) => sent,
                Err(poisoned) => poisoned.into_inner(),
            };
            sent.push(format!("{} {}", request.method().as_str(), path));
        }
        let answer =
            if request.method() == Method::Get && path.starts_with("/v2/orders?status=open") {
                Response {
                    status: 200,
                    body: self.open_orders.as_bytes().to_vec(),
                }
            } else if request.method() == Method::Get
                && path.starts_with("/v2/orders:by_client_order_id?client_order_id=")
            {
                let asked = path.rsplit("client_order_id=").next().unwrap_or_default();
                match (&self.entry, asked == BRACKET_ENTRY) {
                    (Some(answer), true) => Response {
                        status: 200,
                        body: answer.as_bytes().to_vec(),
                    },
                    _ => Response {
                        status: 404,
                        body: self.absent.as_bytes().to_vec(),
                    },
                }
            } else if request.method() == Method::Delete && path.starts_with("/v2/orders/") {
                Response {
                    status: 204,
                    body: Vec::new(),
                }
            } else if request.method() == Method::Get && path.starts_with("/v2/orders/") {
                Response {
                    status: 200,
                    body: self.canceled_leg.as_bytes().to_vec(),
                }
            } else {
                Response {
                    status: 404,
                    body: self.absent.as_bytes().to_vec(),
                }
            };
        Ok(answer)
    }
}

/// One request to cancel a filled bracket's placement, by the platform's handle for it.
fn cancel_bracket_placement() -> mandate_executor::BrokerRequest {
    mandate_executor::BrokerRequest::Cancel {
        client_order_id: client_order_id(BRACKET_PLACEMENT),
    }
}

/// E7-4 (DEC-878's "Not decided here: the cancel of a bracket's placement", #1270's "Not
/// done"), §5.4's marketable exit sequence, DEC-867 item 3: cancelling the placement of a
/// completely filled bracket — the handle `{entry}-p{record}`, which no Alpaca order carries —
/// cancels the two legs the broker holds nested under the filled entry, each by **its own**
/// broker order id read from the open-orders snapshot, never for the handle itself, whose every
/// answer is the broker's 404 (DEC-878 items 1 and 2).
#[ignore = "pending E7-4"]
#[tokio::test]
async fn cancelling_a_brackets_placement_cancels_its_two_legs_by_their_own_broker_ids() {
    let transport = FilledBracketHost::answering(serde_json::json!([filled_bracket_entry()]));
    let client = TradingClient::new(
        transport.clone(),
        FakeClock::default(),
        RetryPolicy::default(),
    );
    let outcome = client
        .call_one(&cancel_bracket_placement())
        .await
        .unwrap_or_else(|e| panic!("cancelling the placement of nested legs is an answer: {e}"));
    assert_eq!(
        outcome,
        BrokerOutcome::CancelAccepted {
            client_order_id: BRACKET_PLACEMENT.to_owned()
        },
        "the placement is cancelled with its legs — the exit sequence's first step (§5.4) — and \
         confirmed by the legs' read-backs (DEC-867 item 3), never by the request alone"
    );
    let sent = transport.sent_lines();
    for leg in [TAKE_PROFIT_LEG_ID, STOP_LEG_ID] {
        assert!(
            sent.contains(&format!("DELETE /v2/orders/{leg}")),
            "both legs the broker holds nested under the filled entry are cancelled, each by its \
             own broker order id (DEC-878 items 1 and 2): {sent:?}"
        );
    }
    assert!(
        sent.iter().all(|line| !line.contains(BRACKET_PLACEMENT)),
        "the handle is the platform's, never a broker id, so no request asks Alpaca for it \
         (DEC-878 item 1): {sent:?}"
    );
}

/// The recordings `filled_bracket_entry` builds, with an entry whose own status is `status` and
/// nothing of it filled: the listing of an entry that does not rest as a filled bracket does.
fn entry_in_status(status: &str) -> serde_json::Value {
    let mut entry = filled_bracket_entry();
    entry["status"] = serde_json::json!(status);
    entry["filled_at"] = serde_json::Value::Null;
    entry["filled_avg_price"] = serde_json::Value::Null;
    entry["filled_qty"] = serde_json::json!("0");
    serde_json::json!([entry])
}

/// The recording's listing with its legs changed by `change`, so a case names the one shape that
/// keeps the placement from resting whole (DEC-878 item 2).
fn legs_changed(change: impl Fn(&mut Vec<serde_json::Value>)) -> serde_json::Value {
    let mut entry = filled_bracket_entry();
    let mut legs = entry["legs"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| panic!("the recording nests its bracket's legs"));
    change(&mut legs);
    entry["legs"] = serde_json::json!(legs);
    serde_json::json!([entry])
}

/// Every listing that leaves a filled bracket's placement short of DEC-878 item 2's whole
/// bracket: the entry listed `filled` with exactly two resting sell legs nested under it, the
/// recorded stop and take-profit (a missing stop never reads as covering, item 9). Each is a
/// real doubt, so a cancel of the placement against it fails closed (rule 3: what cannot be
/// shown resting is never assumed).
fn listings_without_a_resting_bracket() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("nothing listed", serde_json::json!([])),
        (
            "the entry canceled, its legs resting",
            entry_in_status("canceled"),
        ),
        (
            "the entry accepted, not filled",
            entry_in_status("accepted"),
        ),
        (
            "the take-profit resting alone, the stop gone",
            legs_changed(|legs| legs.truncate(1)),
        ),
        (
            "a third leg beside the two, done",
            legs_changed(|legs| {
                let mut done = legs[1].clone();
                done["id"] = serde_json::json!("9b3ba3a5-4e21-4179-9f3f-4ba2c11f70e1");
                done["client_order_id"] = serde_json::json!("1f7c3314-a11d-4464-872b-05f671c148ad");
                done["status"] = serde_json::json!("canceled");
                legs.push(done);
            }),
        ),
        (
            "both legs that buy",
            legs_changed(|legs| {
                for leg in legs {
                    leg["side"] = serde_json::json!("buy");
                }
            }),
        ),
        (
            "the stop partly filled",
            legs_changed(|legs| {
                legs[1]["status"] = serde_json::json!("partially_filled");
                legs[1]["filled_qty"] = serde_json::json!("0.4");
            }),
        ),
        (
            "the stop pending a cancel",
            legs_changed(|legs| {
                legs[1]["status"] = serde_json::json!("pending_cancel");
            }),
        ),
        (
            "the stop for 2 where the take-profit has 1",
            legs_changed(|legs| {
                legs[1]["qty"] = serde_json::json!("2");
            }),
        ),
    ]
}

/// E7-4 (DEC-878 items 2 and 9, rule 3), #1270's "Not done": a cancel of a bracket's placement
/// whose open-orders snapshot cannot show the entry carrying exactly two resting sell legs is
/// never confirmed and never answered as an absence — which is what today's lookup by the
/// handle answers, since Alpaca holds no order by that name. The connector fails closed: no leg
/// is deleted, no request asks for the handle, and the placement stays held or doubted rather
/// than assumed cancelled, so risk never widens.
#[ignore = "pending E7-4"]
#[tokio::test]
async fn a_bracket_placement_without_its_whole_bracket_resting_is_never_assumed_cancelled() {
    for (case, listing) in listings_without_a_resting_bracket() {
        let transport = FilledBracketHost::answering(listing);
        let client = TradingClient::new(
            transport.clone(),
            FakeClock::default(),
            RetryPolicy::default(),
        );
        let outcome = client.call_one(&cancel_bracket_placement()).await;
        let sent = transport.sent_lines();
        assert!(
            sent.iter().all(|line| !line.contains(BRACKET_PLACEMENT)),
            "{case}: no request asks the broker for the handle `{BRACKET_PLACEMENT}`, which no \
             broker order carries (DEC-878 item 1): {sent:?}"
        );
        assert!(
            !sent.iter().any(|line| line.starts_with("DELETE ")),
            "{case}: a bracket the snapshot cannot show resting whole is refused, never \
             cancelled (rule 3, DEC-878 item 2): {sent:?}"
        );
        assert!(
            !matches!(&outcome, Ok(BrokerOutcome::CancelAccepted { .. })),
            "{case}: a cancel nothing confirms is not confirmed (DEC-867 item 3): {outcome:?}"
        );
        assert!(
            !matches!(&outcome, Ok(BrokerOutcome::Absent { .. })),
            "{case}: legs the broker may still hold are never assumed gone by a 404 asked for a \
             name the broker does not hold; the placement stays in doubt rather than assumed \
             cancelled (rule 3, DEC-878 items 1 and 5): {outcome:?}"
        );
    }
}

#[tokio::test]
async fn the_order_query_reads_by_our_client_order_id() {
    let (client, transport) = serving(vec![
        common::reply(200, "order_by_client_id_found", 0),
        common::reply(404, "order_by_client_id_absent", 0),
    ]);
    let request = mandate_executor::BrokerRequest::GetOrderByClientId(client_order_id(
        "md-e144b97773a6f87c1978cc2831",
    ));
    let found = client.call_one(&request).await.expect("found is an answer");
    let absent = client
        .call_one(&request)
        .await
        .expect("absent is an answer");
    assert_eq!(
        lines(&transport),
        vec![
            "GET /v2/orders:by_client_order_id?client_order_id=md-e144b97773a6f87c1978cc2831"
                .to_owned();
            2
        ]
    );
    assert!(
        matches!(&found, mandate_executor::BrokerOutcome::Order(order) if order.status == "accepted"),
        "{found:?}"
    );
    assert_eq!(
        absent,
        mandate_executor::BrokerOutcome::Absent {
            client_order_id: "md-e144b97773a6f87c1978cc2831".to_owned()
        }
    );
}

#[tokio::test]
async fn the_open_orders_are_listed_in_one_page() {
    let (client, transport) = serving(vec![common::reply(200, "open_orders_page", 0)]);
    let outcome = client
        .call_one(&mandate_executor::BrokerRequest::ListOpenOrders)
        .await
        .expect("a page is an answer");
    assert_eq!(
        lines(&transport),
        vec!["GET /v2/orders?status=open&limit=500&direction=asc&nested=true".to_owned()],
        "every open order in one page of Alpaca's largest size, oldest first, with bracket and OCO \
         legs nested under their parent so a leg is never mistaken for external activity"
    );
    let expected = wire::open_orders(&common::body("open_orders_page", 0)).expect("parses");
    assert!(!expected.is_empty(), "the recording holds a resting order");
    assert_eq!(
        outcome,
        mandate_executor::BrokerOutcome::OpenOrders(expected)
    );
}

#[tokio::test]
async fn the_positions_are_listed() {
    let held = r#"[{"asset_class":"us_equity","avg_entry_price":"150","qty":"10","side":"long","symbol":"AAPL"}]"#;
    let (client, transport) = serving(vec![common::inline(200, held)]);
    let outcome = client
        .call_one(&mandate_executor::BrokerRequest::ListPositions)
        .await
        .expect("positions are an answer");
    assert_eq!(lines(&transport), vec!["GET /v2/positions".to_owned()]);
    let mandate_executor::BrokerOutcome::Positions(positions) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(positions.len(), 1);
    assert_eq!(
        positions.first().map(|p| p.qty),
        Some(mandate_num::SignedQty::parse("10").expect("canonical"))
    );
}

#[tokio::test]
async fn the_account_is_read() {
    let (client, transport) = serving(vec![common::reply(200, "account_active", 0)]);
    let outcome = client
        .call_one(&mandate_executor::BrokerRequest::GetAccount)
        .await
        .expect("the account is an answer");
    assert_eq!(lines(&transport), vec!["GET /v2/account".to_owned()]);
    let mandate_executor::BrokerOutcome::Account(account) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(account.status, "ACTIVE");
    assert_eq!(account.multiplier, 4);
    assert_eq!(
        account.buying_power,
        mandate_num::Usd::parse("3999997.98").expect("canonical")
    );
}

#[tokio::test]
async fn the_activities_walk_resumes_from_its_cursor_and_answers_the_next_one() {
    let (client, transport) = serving(vec![
        common::reply(200, "partial_then_filled", 2),
        common::reply(200, "activities_fills", 0),
    ]);
    let first = client
        .call_one(&mandate_executor::BrokerRequest::ListActivities {
            since: mandate_executor::ActivityCursor(String::new()),
        })
        .await
        .expect("a page is an answer");
    let mandate_executor::BrokerOutcome::Activities { fills, cursor } = first else {
        panic!("{first:?}");
    };
    assert_eq!(fills.len(), 2, "a partial and its completion");
    assert_eq!(
        cursor.0, "20260926233000000::22222222-2222-4222-8222-222222222222",
        "the next walk resumes after the last activity this page carried"
    );
    let second = client
        .call_one(&mandate_executor::BrokerRequest::ListActivities {
            since: cursor.clone(),
        })
        .await
        .expect("an empty page is an answer");
    assert_eq!(
        second,
        mandate_executor::BrokerOutcome::Activities {
            fills: Vec::new(),
            cursor: cursor.clone(),
        },
        "an empty page leaves the cursor where it was"
    );
    assert_eq!(
        lines(&transport),
        vec![
            "GET /v2/account/activities?activity_types=FILL&direction=asc&page_size=100".to_owned(),
            format!(
                "GET /v2/account/activities?activity_types=FILL&direction=asc&page_size=100&page_token={}",
                cursor.0
            ),
        ],
        "fills oldest first; an empty cursor is the first page and a cursor becomes Alpaca's \
         page_token"
    );
}

#[tokio::test]
async fn the_connector_hands_the_executor_only_unknown_outcomes() {
    use mandate_executor::BrokerConnector;
    let (mut client, _transport) = serving(vec![
        Err(mandate_alpaca::TransportError::Timeout),
        common::inline(200, "not json"),
    ]);
    let timed_out = client
        .call(&mandate_executor::BrokerRequest::GetAccount)
        .await
        .expect_err("a timeout is not an answer");
    assert_ne!(
        timed_out,
        mandate_executor::ConnectorError::NotSent {
            code: "unimplemented"
        },
        "E7-3 has not been implemented yet"
    );
    assert_eq!(
        timed_out,
        mandate_executor::ConnectorError::Unknown(BrokerUnknown::Timeout),
        "a timeout may have reached the broker, so the executor queries"
    );
    let unreadable = client
        .call(&mandate_executor::BrokerRequest::GetAccount)
        .await
        .expect_err("a body that is not JSON is not an answer");
    assert_eq!(
        unreadable,
        mandate_executor::ConnectorError::Unreadable { code: "not_json" },
        "the broker answered and we could not read it: the shell stops (DEC-85), the executor \
         never queries on it"
    );
}

#[test]
fn a_client_failure_maps_onto_the_connector_honestly() {
    use mandate_alpaca::{ClientError, TransportError};
    use mandate_executor::ConnectorError;
    let cases = [
        (
            ClientError::Unknown(BrokerUnknown::Ambiguous),
            Some(BrokerUnknown::Ambiguous),
            ConnectorError::Unknown(BrokerUnknown::Ambiguous),
            "unknown_outcome",
        ),
        (
            ClientError::Transport(TransportError::Timeout),
            Some(BrokerUnknown::Timeout),
            ConnectorError::Unknown(BrokerUnknown::Timeout),
            "timeout",
        ),
        (
            ClientError::Transport(TransportError::Connect),
            Some(BrokerUnknown::Transport),
            ConnectorError::Unknown(BrokerUnknown::Transport),
            "connect",
        ),
        (
            ClientError::Transport(TransportError::Request),
            Some(BrokerUnknown::Transport),
            ConnectorError::Unknown(BrokerUnknown::Transport),
            "request",
        ),
        (
            ClientError::Transport(TransportError::RefusedPath),
            None,
            ConnectorError::NotSent {
                code: "refused_path",
            },
            "refused_path",
        ),
        (
            ClientError::Wire(WireError::FloatNumber { field: "price" }),
            None,
            ConnectorError::Unreadable {
                code: "float_number",
            },
            "float_number",
        ),
        (
            ClientError::Unimplemented { story: "E7-2" },
            None,
            ConnectorError::NotSent {
                code: "unimplemented",
            },
            "unimplemented",
        ),
    ];
    for (error, unknown, connector, code) in cases {
        assert_eq!(error.code(), code, "{error:?}");
        assert_eq!(
            error.as_unknown(),
            unknown,
            "only a genuinely unknown outcome is one (interpretation 10): {error:?}"
        );
        assert_eq!(
            error.to_connector(),
            connector,
            "a parse failure and a refused path are never unknown outcomes: {error:?}"
        );
    }
}

#[test]
fn every_transport_and_setup_failure_has_its_code_and_its_nature() {
    use mandate_alpaca::TransportError;
    for (error, code, unknown) in [
        (TransportError::Timeout, "timeout", true),
        (TransportError::Connect, "connect", true),
        (TransportError::Request, "request", true),
        (TransportError::RefusedPath, "refused_path", false),
    ] {
        assert_eq!(error.code(), code);
        assert_eq!(
            error.is_unknown_outcome(),
            unknown,
            "a refused path never left the process; every other failure might have reached the broker"
        );
    }
    assert_eq!(mandate_alpaca::HttpSetupError::Client.code(), "client");
    assert_eq!(
        mandate_alpaca::CredentialsError::Missing { variable: "V" }.code(),
        "missing"
    );
}

#[test]
fn the_connector_error_says_which_failures_the_executor_may_fold() {
    use mandate_executor::ConnectorError;
    for (error, code, unknown) in [
        (
            ConnectorError::Unknown(BrokerUnknown::Timeout),
            "unknown_outcome",
            Some(BrokerUnknown::Timeout),
        ),
        (
            ConnectorError::Unreadable { code: "not_json" },
            "unreadable",
            None,
        ),
        (
            ConnectorError::NotSent {
                code: "refused_path",
            },
            "not_sent",
            None,
        ),
    ] {
        assert_eq!(error.code(), code);
        assert_eq!(error.as_unknown(), unknown);
        assert!(!format!("{error}").is_empty());
    }
    for (unknown, code) in [
        (BrokerUnknown::Timeout, "timeout"),
        (BrokerUnknown::Ambiguous, "ambiguous"),
        (BrokerUnknown::Transport, "transport"),
    ] {
        assert_eq!(unknown.code(), code);
    }
}

#[test]
fn an_account_prints_no_account_number_and_no_account_id() {
    let account = wire::WireAccount {
        id: "8f1c5b2a-0000-4000-8000-000000000000".to_owned(),
        account_number: "PA3ABCDEFGHI".to_owned(),
        status: "ACTIVE".to_owned(),
        crypto_status: "ACTIVE".to_owned(),
        currency: "USD".to_owned(),
        trading_blocked: false,
        account_blocked: false,
        trade_suspended_by_user: false,
        multiplier: "4".to_owned(),
        equity: "1000000".to_owned(),
        cash: "1000000".to_owned(),
        buying_power: "3999997.98".to_owned(),
        non_marginable_buying_power: "999998.99".to_owned(),
        accrued_fees: "0".to_owned(),
    };
    let rendered = format!("{account:?}");
    assert!(
        !rendered.contains("PA3ABCDEFGHI")
            && !rendered.contains("8f1c5b2a")
            && !rendered.contains("account_number"),
        "`Debug` prints neither personal-data field (journal §6.4): {rendered}"
    );
    for kept in [
        "ACTIVE",
        "USD",
        "3999997.98",
        "999998.99",
        "multiplier",
        "trading_blocked",
        "account_blocked",
        "trade_suspended_by_user",
        "equity",
        "cash",
        "accrued_fees",
        "crypto_status",
    ] {
        assert!(
            rendered.contains(kept),
            "and still prints what an operator needs: `{kept}` in {rendered}"
        );
    }
}

#[test]
fn every_error_code_is_stable_and_unique() {
    let codes = WireError::CODES;
    assert!(
        codes.contains(&"unimplemented"),
        "the stubs' own code is part of the closed set (DEC-77, DEC-83)"
    );
    let mut seen = std::collections::BTreeSet::new();
    for code in codes {
        assert!(seen.insert(code), "`{code}` appears twice");
    }
    let samples = [
        WireError::Unimplemented { story: "E7-2" },
        WireError::NotJson,
        WireError::MissingField { field: "f" },
        WireError::WrongType { field: "f" },
        WireError::FloatNumber { field: "f" },
        WireError::ExponentForm { field: "f" },
        WireError::TooManyPlaces { field: "f" },
        WireError::UnknownStatus {
            status: "x".to_owned(),
        },
        WireError::NotInterpreted {
            field: "f",
            story: "E7-2",
        },
    ];
    for sample in &samples {
        assert!(
            codes.contains(&sample.code()),
            "`{}` is not in the declared set (ES-09)",
            sample.code()
        );
    }
    assert_eq!(codes.len(), 11, "the set is closed");
    for error in [
        mandate_alpaca::ClientError::Unimplemented { story: "E7-2" },
        mandate_alpaca::ClientError::Wire(WireError::NotJson),
        mandate_alpaca::ClientError::Transport(mandate_alpaca::TransportError::Timeout),
        mandate_alpaca::ClientError::Unknown(BrokerUnknown::Ambiguous),
    ] {
        assert!(
            !error.code().is_empty(),
            "every client failure has a stable code"
        );
    }
    assert!(
        mandate_alpaca::ClientError::Wire(WireError::NotJson)
            .as_unknown()
            .is_none(),
        "a parse failure is an answer this crate could not read, not an unknown outcome (DEC-85)"
    );
}

/// The shell's pause really waits: a retry that did not wait would hit the broker again at once.
/// Measured on the adapter's own clock, the only wall clock this crate reads.
#[tokio::test]
async fn the_tokio_pause_waits_for_the_duration() {
    let before = TokioPause.now();
    TokioPause.pause(std::time::Duration::from_millis(40)).await;
    let after = TokioPause.now();
    let nanos =
        |t: mandate_time::UtcNanos| i128::from(t.secs()) * 1_000_000_000 + i128::from(t.nanos());
    let waited = nanos(after) - nanos(before);
    assert!(waited >= 40_000_000, "the pause returned after {waited} ns");
}

#[tokio::test]
async fn a_5xx_on_the_cancels_delete_is_an_unknown_outcome() {
    let (client, transport) = serving(vec![
        common::reply(200, "order_by_client_id_found", 0),
        common::inline(503, "{\"message\":\"unavailable\"}"),
    ]);
    let error = common::answered(
        client
            .call_one(&mandate_executor::BrokerRequest::Cancel {
                client_order_id: client_order_id("md-e144b97773a6f87c1978cc2831"),
            })
            .await,
    )
    .expect_err("a 5xx after the DELETE left is not a settled answer");
    assert_eq!(
        error.as_unknown(),
        Some(BrokerUnknown::Ambiguous),
        "the broker may have cancelled it: the answer is unknown, never a refusal read back as \
         the order's state (interpretation 10)"
    );
    assert_eq!(
        transport.sent().len(),
        2,
        "the lookup and the DELETE, and nothing read back after an unknown outcome"
    );
}

#[tokio::test]
async fn a_5xx_on_a_reconciliation_read_is_an_unknown_outcome_not_an_unreadable_body() {
    let (client, _transport) = serving(vec![common::inline(503, "{\"message\":\"unavailable\"}")]);
    let error = common::answered(
        client
            .call_one(&mandate_executor::BrokerRequest::ListPositions)
            .await,
    )
    .expect_err("a 5xx is not a positions page");
    assert_eq!(
        error.to_connector().code(),
        "unknown_outcome",
        "an overloaded broker is retried by the shell's reconciliation timer, not stopped on as \
         an unreadable answer (DEC-85 is for bodies this crate cannot read)"
    );
}

#[test]
fn an_exchange_of_exactly_the_inline_limit_stays_inline() {
    let envelope = "{\"message\":\"\"}".len();
    let filler = "x".repeat(mandate_alpaca::INLINE_LIMIT.saturating_sub(envelope));
    let response = mandate_alpaca::Response {
        status: 200,
        body: format!("{{\"message\":\"{filler}\"}}").into_bytes(),
    };
    let recorded = record::response("/v2/orders", &response).expect("the response records");
    let mandate_alpaca::RecordedBody::Inline(text) = &recorded.body else {
        panic!(
            "at exactly the limit the body is still inline (journal §6.3: above it, a reference)"
        );
    };
    assert_eq!(
        text.len(),
        mandate_alpaca::INLINE_LIMIT,
        "the redacted bytes are the limit exactly"
    );
}
