//! The account's own id, read for the connect check alone (connections spec §5.2 step 4,
//! DEC-2339): `GET /v2/account`'s `id`, as a `BrokerAccountId` that prints nothing of it, over the
//! recorded `account_active` scenario, whose `id` the recording replaced with
//! `pii:account_id:1`, and hand-written bodies, with no network (ADR-0001 ES-19).

mod common;

use common::{FakeClock, FakeTransport, body, inline, reply};
use mandate_alpaca::client::{RetryPolicy, TradingClient};
use mandate_alpaca::error::{ReadError, WireError};
use mandate_alpaca::http::Method;
use mandate_alpaca::wire;
use mandate_connections::account_entry::BrokerAccountId;
use mandate_time::UtcNanos;

const RECORDED: &str = "pii:account_id:1";

fn now() -> UtcNanos {
    UtcNanos::parse_rfc3339("2026-09-28T13:00:00Z").unwrap()
}

fn client(
    replies: impl IntoIterator<
        Item = Result<mandate_alpaca::http::Response, mandate_alpaca::error::TransportError>,
    >,
) -> (TradingClient<FakeTransport, FakeClock>, FakeTransport) {
    let transport = FakeTransport::serving(replies);
    let client = TradingClient::new(
        transport.clone(),
        FakeClock::at(now()),
        RetryPolicy::default(),
    );
    (client, transport)
}

/// The recorded account's `id` is read, and only it: a body whose every other member is gone or
/// broken still gives the id.
#[test]
#[ignore = "pending E10-13"]
fn the_account_id_is_the_objects_id_and_nothing_else() {
    assert_eq!(
        wire::account_id(&body("account_active", 0)),
        Ok(BrokerAccountId::new(RECORDED).unwrap())
    );
    for bare in [
        r#"{"id":"904837e3-3b76-47ec-b432-046db621571b"}"#,
        r#"{"id":"904837e3-3b76-47ec-b432-046db621571b","equity":1e9,"status":7}"#,
    ] {
        assert_eq!(
            wire::account_id(bare.as_bytes()),
            Ok(BrokerAccountId::new("904837e3-3b76-47ec-b432-046db621571b").unwrap()),
            "{bare}"
        );
    }
}

/// No id, an id that is not text, an id outside the account entry's form, or no JSON is refused,
/// and no refusal names the value.
#[test]
#[ignore = "pending E10-13"]
fn an_account_id_that_is_missing_or_malformed_is_refused() {
    for (bad, want) in [
        (
            r#"{"account_number":"PA1"}"#,
            WireError::MissingField { field: "id" },
        ),
        (r#"{"id":null}"#, WireError::WrongType { field: "id" }),
        (r#"{"id":12345}"#, WireError::WrongType { field: "id" }),
        (
            r#"{"id":"acct canary 9q"}"#,
            WireError::WrongType { field: "id" },
        ),
        (r#"{"id":""}"#, WireError::WrongType { field: "id" }),
        ("[]", WireError::WrongType { field: "account" }),
        ("not json", WireError::NotJson),
    ] {
        let refused = wire::account_id(bad.as_bytes());
        assert!(!format!("{refused:?}").contains("canary"), "{bad}");
        assert_eq!(refused.map(drop), Err(want), "{bad}");
    }
}

/// The client reads `GET /v2/account` once, through the allowlist, and answers its id; an
/// overloaded broker is `Overloaded`, never an id, and another status is not interpreted.
#[tokio::test]
#[ignore = "pending E10-13"]
async fn the_client_reads_the_account_once_for_its_id() {
    let (trading, transport) = client([reply(200, "account_active", 0)]);
    assert_eq!(
        trading.account_id().await,
        Ok(BrokerAccountId::new(RECORDED).unwrap())
    );
    let sent: Vec<(Method, String)> = transport
        .sent()
        .iter()
        .map(|r| (r.method(), r.path_and_query().to_owned()))
        .collect();
    assert_eq!(sent, [(Method::Get, "/v2/account".to_owned())]);
    for (status, want) in [
        (503_u16, ReadError::Overloaded),
        (429, ReadError::Overloaded),
        (403, ReadError::UnexpectedStatus { status: 403 }),
    ] {
        let (trading, _) = client([inline(status, "{}")]);
        assert_eq!(trading.account_id().await, Err(want), "{status}");
    }
}
