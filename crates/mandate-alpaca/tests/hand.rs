//! Hand-calculated cases for the Alpaca paper trading connector
//! ([task brief](../../../docs/project/tasks/M6-K-executor-and-connector.md), backlog E7-2,
//! E7-3, E7-4).
//!
//! Every case drives the client through the **recorded** scenario it names, with no network
//! (ADR-0001 ES-19), and computes its expectation from the spec rather than from the code. Cases
//! whose subject is the submission or the status mapping are `pending E7-2`; the reconciliation
//! sources are `pending E7-3`; the protective shapes are `pending E7-4`.

mod common;

use common::{FakeClock, FakeTransport, scenario};
use mandate_alpaca::client::{RetryPolicy, TradingClient};
use mandate_alpaca::error::WireError;
use mandate_alpaca::http::{Credentials, HttpRequest, Method, is_paper_trading_path};
use mandate_alpaca::{KEY_ID_VAR, PAPER_HOST, SECRET_VAR, record, wire};
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
    let source = include_str!("../src/http.rs");
    assert!(
        !source.contains("//api.alpaca.markets"),
        "ES-23: only the paper trading host is compiled in, and a `live` feature is forbidden"
    );
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

#[tokio::test]
#[ignore = "pending E7-2"]
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
    assert!(
        transport
            .sent()
            .iter()
            .all(|request| is_paper_trading_path(&request.path_and_query)),
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
#[ignore = "pending E7-2"]
fn a_broker_decimal_with_nine_places_parses_exactly() {
    let raw = serde_json::json!("0.123456789");
    let parsed = wire::decimal_text(&raw, "qty").expect("nine places is the quantity scale");
    assert_eq!(parsed, "0.123456789", "read as text, never through an f64");
}

#[test]
#[ignore = "pending E7-2"]
fn a_broker_number_in_exponent_form_is_rejected_with_its_code() {
    let raw = serde_json::json!("1e3");
    let error = wire::decimal_text(&raw, "limit_price").expect_err("an exponent is not canonical");
    assert_eq!(error.code(), "exponent_form", "{error}");
}

#[test]
#[ignore = "pending E7-2"]
fn a_json_number_is_rejected_because_it_has_already_been_through_a_float() {
    let raw = serde_json::json!(150.25);
    let error = wire::decimal_text(&raw, "price").expect_err("a JSON number is not raw text");
    assert_eq!(error.code(), "float_number", "{error}");
}

#[test]
#[ignore = "pending E7-2"]
fn a_quantity_with_more_places_than_the_increment_is_rejected() {
    let raw = serde_json::json!("0.1234567891");
    let error = wire::decimal_text(&raw, "qty").expect_err("ten places is past the scale");
    assert_eq!(error.code(), "too_many_places", "{error}");
}

#[test]
#[ignore = "pending E7-2"]
fn every_broker_status_in_the_table_maps() {
    for status in [
        "new",
        "accepted",
        "pending_new",
        "accepted_for_bidding",
        "held",
        "partially_filled",
        "filled",
        "done_for_day",
        "stopped",
        "calculated",
        "pending_cancel",
        "canceled",
        "expired",
        "rejected",
        "suspended",
        "pending_replace",
        "replaced",
    ] {
        wire::status(status)
            .unwrap_or_else(|e| panic!("§5.7's table names `{status}` and it did not map: {e}"));
    }
}

#[test]
#[ignore = "pending E7-2"]
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
#[ignore = "pending E7-2"]
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
#[ignore = "pending E7-2"]
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
    wire::order(&text).expect(
        "a later recording carries more than a hand-built body, and the permissiveness is \
         exactly where it is safe: an unknown **status** still fails loudly",
    );
}

#[tokio::test]
#[ignore = "pending E7-2"]
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
#[ignore = "pending E7-2"]
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
#[ignore = "pending E7-2"]
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
#[ignore = "pending E7-2"]
async fn a_transport_failure_is_an_unknown_outcome_and_never_a_rejection() {
    let transport = FakeTransport::serving([Err(mandate_alpaca::TransportError::Timeout)]);
    let client = TradingClient::new(transport, FakeClock::default(), RetryPolicy::default());
    let error = client
        .order_by_client_id("md-e144b97773a6f87c1978cc2831")
        .await
        .expect_err("a timeout is not an answer");
    assert_eq!(
        error.as_unknown(),
        Some(BrokerUnknown::Timeout),
        "an unknown outcome makes the executor query, never resubmit (interpretation 10)"
    );
}

#[test]
#[ignore = "pending E7-3"]
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
#[ignore = "pending E7-3"]
fn the_positions_source_parses() {
    let positions = wire::positions(&body_of("positions", 0)).expect("the recording parses");
    assert!(
        positions.is_empty(),
        "the recording was taken with a flat account, which is the honest empty case"
    );
}

#[test]
#[ignore = "pending E7-3"]
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
#[ignore = "pending E7-3"]
fn a_blocked_account_parses_into_the_restriction_signals() {
    let account = wire::account(&body_of("account_blocked", 0)).expect("the recording parses");
    assert!(
        account.trading_blocked && account.account_blocked,
        "§7.3's first row reads these flags, not a message"
    );
}

#[test]
#[ignore = "pending E7-3"]
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
#[ignore = "pending E7-3"]
fn a_reject_parses_into_the_restriction_table_signals() {
    let reject = wire::reject(422, &body_of("submit_rejected", 0)).expect("the recording parses");
    assert_eq!(reject.http_status, 422);
    assert!(
        !reject.message.is_empty(),
        "§7.3's table matches against the broker's own message"
    );
}

#[test]
#[ignore = "pending E7-4"]
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

#[test]
#[ignore = "pending E7-4"]
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
#[ignore = "pending E7-4"]
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
#[ignore = "pending E7-2"]
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
#[ignore = "pending E7-2"]
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
#[ignore = "pending E7-2"]
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
#[ignore = "pending E7-2"]
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
#[ignore = "pending E7-4"]
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
#[ignore = "pending E7-2"]
fn a_broker_exchange_is_recorded_with_its_credentials_redacted() {
    let request = HttpRequest {
        method: Method::Post,
        path_and_query: "/v2/orders".to_owned(),
        body: Some("{\"client_order_id\":\"md-1\"}".to_owned()),
    };
    let recorded = record::request(&request).expect("the request records");
    let rendered = format!("{recorded:?}");
    assert!(
        !rendered.contains("APCA")
            && !rendered.contains(common::SENTINEL_KEY_ID)
            && !rendered.contains(common::SENTINEL_SECRET),
        "the authorisation headers are removed before anything is hashed or stored: {rendered}"
    );
}

#[test]
#[ignore = "pending E7-2"]
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
#[ignore = "pending E7-2"]
fn a_large_exchange_is_recorded_by_artifact_reference() {
    let filler = "x".repeat(mandate_alpaca::INLINE_LIMIT.saturating_add(1));
    let response = mandate_alpaca::Response {
        status: 200,
        body: format!("{{\"message\":\"{filler}\"}}").into_bytes(),
    };
    let recorded = record::response("/v2/orders", &response).expect("the response records");
    assert!(
        matches!(recorded.body, mandate_alpaca::RecordedBody::Artifact { .. }),
        "above the inline limit the payload carries a `sha256:` reference (journal §6.3, DEC-107)"
    );
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
