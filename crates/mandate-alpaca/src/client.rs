//! The paper trading client: one method per [`BrokerRequest`] variant, each answering with a
//! [`BrokerOutcome`] the executor folds.
//!
//! The transport and the clock are injected, so no test touches a network and no test waits
//! (ADR-0001 ES-19, the `mandate-marketdata` shape). The client holds no state a restart would
//! lose: every fact it produces is journaled by the executor before it is acted on.

use std::future::Future;
use std::time::Duration;

use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{
    ActivityCursor, BrokerConnector, BrokerOrder, BrokerOutcome, BrokerRequest, BrokerUnknown,
    ConnectorError,
};
use mandate_time::UtcNanos;

use mandate_accounting::InstrumentId;

use crate::error::{ClientError, ReadError, TransportError, WireError};
use crate::http::{HttpRequest, Method, Response, TradingTransport};
use crate::read::{self, AssetSnapshot};
use crate::wire;

/// Tells the time and waits between retries. The client paces against `now`, so a `pause` must
/// move `now` forward by at least its duration.
pub trait Pause {
    fn now(&self) -> UtcNanos;
    fn pause(&self, duration: Duration) -> impl Future<Output = ()>;
}

/// The system clock and the tokio timer.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioPause;

impl Pause for TokioPause {
    #[allow(
        clippy::disallowed_methods,
        reason = "the adapter's edge reads the wall clock to pace retries; the core never does (ES-05, ES-21)"
    )]
    fn now(&self) -> UtcNanos {
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .ok()
            .and_then(|since| {
                let secs = i64::try_from(since.as_secs()).ok()?;
                UtcNanos::from_parts(secs, since.subsec_nanos()).ok()
            })
            .unwrap_or(UtcNanos::EPOCH)
    }

    async fn pause(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

/// How many times a request whose outcome is **settled** may be retried, and how long the waits
/// are. A request whose outcome is **unknown** is never retried here: it answers
/// [`ConnectorError::Unknown`], and the executor resolves it by querying on the client order id,
/// never by resubmitting (trading-domain spec §5.7, task brief interpretation 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub first_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            first_delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(4),
        }
    }
}

/// The client. One method per request variant; nothing else reaches the host.
///
/// It never retries on its own. A request whose outcome is unknown answers
/// [`ConnectorError::Unknown`] through [`BrokerConnector::call`], and
/// the executor resolves it by querying on the client order id from its own timer, confirming an
/// absence over a window before anything is sent again (trading-domain spec §5.7, task brief
/// interpretation 9). A retry here would be a second, unjournaled path to the broker. The pause
/// and the policy are carried for the shell, which paces those lookups.
#[derive(Debug)]
pub struct TradingClient<T, P> {
    transport: T,
    pause: P,
    _retry: RetryPolicy,
}

impl<T: TradingTransport, P: Pause> TradingClient<T, P> {
    pub fn new(transport: T, pause: P, retry: RetryPolicy) -> Self {
        Self {
            transport,
            pause,
            _retry: retry,
        }
    }

    /// The transport this client sends through, for the tests that assert what was sent.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Submits one order, with **our** `client_order_id` on the wire, and answers what the broker
    /// said. A duplicate `client_order_id` is [`BrokerOutcome::DuplicateClientOrderId`], not a
    /// failure: the broker refusing our own id means the order is already there (E7-2 step 6).
    pub async fn submit(&self, request: &BrokerRequest) -> Result<BrokerOutcome, ClientError> {
        let BrokerRequest::Submit(order) = request else {
            return Err(WireError::NotInterpreted {
                field: "request",
                story: "E7-2",
            }
            .into());
        };
        let body = wire::submission_body(order)?;
        let response = self
            .send(Method::Post, "/v2/orders".to_owned(), Some(body))
            .await?;
        settle(
            response,
            order.client_order_id.as_str(),
            BrokerOutcome::Submitted,
        )
    }

    /// Reads one order back by client order id, which is how an unacknowledged submission is
    /// resolved (journal spec §5.2). A `404` is [`BrokerOutcome::Absent`]: one absence is a fact
    /// the executor counts, never a reason to resubmit.
    pub async fn order_by_client_id(
        &self,
        client_order_id: &str,
    ) -> Result<BrokerOutcome, ClientError> {
        let path = format!("/v2/orders:by_client_order_id?client_order_id={client_order_id}");
        let response = self.send(Method::Get, path, None).await?;
        if response.status == 404 {
            return Ok(BrokerOutcome::Absent {
                client_order_id: client_order_id.to_owned(),
            });
        }
        settle(response, client_order_id, BrokerOutcome::Order)
    }

    /// One request of any variant, the shape [`BrokerConnector`] wraps.
    pub async fn call_one(&self, request: &BrokerRequest) -> Result<BrokerOutcome, ClientError> {
        match request {
            BrokerRequest::Submit(_) => self.submit(request).await,
            BrokerRequest::Cancel { client_order_id } => {
                self.cancel(client_order_id.as_str()).await
            }
            BrokerRequest::AcknowledgeReplace { replaced: id }
            | BrokerRequest::GetOrderByClientId(id) => self.order_by_client_id(id.as_str()).await,
            BrokerRequest::ListOpenOrders => {
                let orders = wire::open_orders(&self.read(OPEN_ORDERS.to_owned()).await?.body)?;
                if orders.len() >= OPEN_ORDERS_PAGE {
                    return Err(WireError::NotInterpreted {
                        field: "open_orders_page",
                        story: "E7-3",
                    }
                    .into());
                }
                Ok(BrokerOutcome::OpenOrders(orders))
            }
            BrokerRequest::ListPositions => {
                let body = self.read("/v2/positions".to_owned()).await?.body;
                Ok(BrokerOutcome::Positions(wire::positions(&body)?))
            }
            BrokerRequest::GetAccount => {
                let body = self.read("/v2/account".to_owned()).await?.body;
                Ok(BrokerOutcome::Account(wire::account(&body)?))
            }
            BrokerRequest::ListActivities { since } => {
                let path = match since.0.as_str() {
                    "" => ACTIVITIES.to_owned(),
                    cursor => format!("{ACTIVITIES}&page_token={}", query_value(cursor)),
                };
                let fills = wire::activities(&self.read(path).await?.body)?;
                let cursor = fills.last().map_or_else(
                    || since.clone(),
                    |fill| ActivityCursor(fill.fill_id.0.clone()),
                );
                Ok(BrokerOutcome::Activities { fills, cursor })
            }
            BrokerRequest::CancelAll(scope) => {
                self.account_wide(HttpRequest::cancel_all(scope)).await
            }
            BrokerRequest::ClosePosition(scope, instrument) => {
                self.account_wide(HttpRequest::close_position(scope, instrument))
                    .await
            }
            BrokerRequest::ListOrders(_) => Err(ClientError::Unimplemented { story: "E7-23" }),
        }
    }

    /// One instrument's asset record, `GET /v2/assets/{symbol}` (trading-domain spec §3.1, E7-8),
    /// stamped with the clock's now so its reader can refuse it once it is too old. It is a read
    /// of the broker's reference data, not a [`BrokerRequest`]: the executor's vocabulary is
    /// stream K's and this read does not change it.
    pub async fn asset(&self, instrument: &InstrumentId) -> Result<AssetSnapshot, ReadError> {
        let response = self
            .transport
            .send(&HttpRequest::asset(instrument)?)
            .await?;
        reference_status(response.status)?;
        Ok(AssetSnapshot {
            asset: read::asset(instrument, &response.body)?,
            loaded_at: self.pause.now(),
        })
    }

    /// Cancels one of our orders. Alpaca cancels by **its** order id, so the order is looked up by
    /// our client order id first, and an order that is not there answers what the lookup said.
    ///
    /// An accepted `DELETE` is [`BrokerOutcome::CancelAccepted`], which is **not** a confirmation:
    /// the executor waits for the order's own `canceled` state (trading-domain spec §5.4). A
    /// refused `DELETE` — the order filled first — is answered with the order's own state, read
    /// back at once, because that is the fact the executor folds (§5.7's
    /// `PendingCancel --> Filled`).
    async fn cancel(&self, client_order_id: &str) -> Result<BrokerOutcome, ClientError> {
        let order = match self.order_by_client_id(client_order_id).await? {
            BrokerOutcome::Order(order) => order,
            other => return Ok(other),
        };
        let path = format!("/v2/orders/{}", order.broker_order_id);
        let response = self.send(Method::Delete, path, None).await?;
        match response.status {
            200..=299 => Ok(BrokerOutcome::CancelAccepted {
                client_order_id: client_order_id.to_owned(),
            }),
            429 | 500..=599 => Err(BrokerUnknown::Ambiguous.into()),
            _ => self.order_by_client_id(client_order_id).await,
        }
    }

    /// A read whose answer must be a success. An overloaded or failing broker is an unknown
    /// outcome; any other answer is not a fact this crate can fold (DEC-85).
    async fn read(&self, path_and_query: String) -> Result<Response, ClientError> {
        let response = self.send(Method::Get, path_and_query, None).await?;
        match response.status {
            200..=299 => Ok(response),
            429 | 500..=599 => Err(BrokerUnknown::Ambiguous.into()),
            _ => Err(WireError::NotInterpreted {
                field: "status",
                story: "E7-3",
            }
            .into()),
        }
    }

    /// An ordinary request: [`HttpRequest::new`] checks the method and the path together against
    /// the allowlist **before** anything is sent, so a crafted id or symbol is refused without a
    /// round trip, and neither account-wide endpoint can be reached from here.
    async fn send(
        &self,
        method: Method,
        path_and_query: String,
        body: Option<String>,
    ) -> Result<Response, ClientError> {
        self.dispatch(&HttpRequest::new(method, &path_and_query, body)?)
            .await
    }

    /// An account-wide request. It exists only for a path the allowlist accepts (see
    /// [`HttpRequest::close_position`]); a refused one never leaves, and the connector answers
    /// `NotSent`. The endpoints answer success or a reject; either way the executor confirms what
    /// happened by reading the order set back (trading-domain spec §5.5). An overloaded or failing
    /// broker is an unknown outcome, never a reject (`AGENTS.md` rule 3, DEC-133 item 10).
    async fn account_wide(
        &self,
        request: Result<HttpRequest, TransportError>,
    ) -> Result<BrokerOutcome, ClientError> {
        let response = self.dispatch(&request?).await?;
        match response.status {
            200..=299 => Ok(BrokerOutcome::AccountWideAccepted),
            429 | 500..=599 => Err(BrokerUnknown::Ambiguous.into()),
            status => Ok(BrokerOutcome::Rejected(wire::reject(
                status,
                &response.body,
            )?)),
        }
    }

    /// The one place a request leaves.
    async fn dispatch(&self, request: &HttpRequest) -> Result<Response, ClientError> {
        Ok(self.transport.send(request).await?)
    }
}

/// The open-orders read: every open order, oldest first, with bracket and OCO legs nested under
/// their parent so a leg's broker-assigned `client_order_id` is never mistaken for external
/// activity (trading-domain spec §11).
const OPEN_ORDERS: &str = "/v2/orders?status=open&limit=500&direction=asc&nested=true";
/// Alpaca's largest page. A full page may be a truncated one, and a reconciliation that missed an
/// order would adopt its absence, so a full page fails loudly instead (DEC-85).
const OPEN_ORDERS_PAGE: usize = 500;
/// The fill activities read, oldest first, resumed from the cursor.
const ACTIVITIES: &str = "/v2/account/activities?activity_types=FILL&direction=asc&page_size=100";
/// What Alpaca answers, with a `422`, for a `client_order_id` it already holds (the recorded
/// `submit_duplicate_client_order_id` scenario).
const DUPLICATE_CLIENT_ORDER_ID: &str = "client_order_id must be unique";

/// The status of a reference-data read (E7-8): a success is read; a `404` is
/// [`ReadError::Absent`]; a `429` or a `5xx` is [`ReadError::Overloaded`]; anything else is
/// [`ReadError::UnexpectedStatus`]. None of them is retried here, and none is a fact about the
/// instrument (DEC-168 item 4).
pub(crate) fn reference_status(status: u16) -> Result<(), ReadError> {
    match status {
        200..=299 => Ok(()),
        404 => Err(ReadError::Absent),
        429 | 500..=599 => Err(ReadError::Overloaded),
        status => Err(ReadError::UnexpectedStatus { status }),
    }
}

/// Settles one order-bearing answer: a success parses into `found`; a broker that is overloaded
/// or failing is an unknown outcome, never a rejection (interpretation 10); the broker refusing
/// our own `client_order_id` is the order already being there (E7-2 step 6); and anything else is
/// a reject the executor reads §7.3's restriction table from.
fn settle(
    response: Response,
    client_order_id: &str,
    found: fn(BrokerOrder) -> BrokerOutcome,
) -> Result<BrokerOutcome, ClientError> {
    match response.status {
        200..=299 => Ok(found(wire::order(&response.body)?)),
        429 | 500..=599 => Err(BrokerUnknown::Ambiguous.into()),
        status => {
            let mut reject = wire::reject(status, &response.body)?;
            if reject.message.contains(DUPLICATE_CLIENT_ORDER_ID) {
                return Ok(BrokerOutcome::DuplicateClientOrderId {
                    client_order_id: client_order_id.to_owned(),
                });
            }
            reject.client_order_id = Some(client_order_id.to_owned());
            Ok(BrokerOutcome::Rejected(reject))
        }
    }
}

/// A query parameter's value, percent-encoded: letters, digits, `-`, `.`, `_`, `~` and `:` stay as
/// they are (RFC 3986 lets a query carry `:`), and every other byte becomes `%XX`. The activities
/// cursor is already held to `[A-Za-z0-9:-]+` by `wire`, so today nothing changes on the wire;
/// this keeps a cursor from adding a parameter or ending the query if that alphabet ever widens
/// (#191 review, round 3).
fn query_value(raw: &str) -> String {
    raw.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~:".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// The connector the executor declares.
///
/// The mapping is [`ClientError::to_connector`]: only a failure whose outcome is genuinely
/// **unknown** becomes [`ConnectorError::Unknown`], because that is what makes the executor query
/// rather than resubmit (task brief interpretation 10). A body this crate could not read is
/// [`ConnectorError::Unreadable`] and a request that never left the process is
/// [`ConnectorError::NotSent`]: the shell stops and alerts on both (DEC-85), and neither is ever
/// reported as a rejection — treating an unread answer as a rejection is how a duplicate is born.
impl<T: TradingTransport, P: Pause> BrokerConnector for TradingClient<T, P> {
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        crate::profile::alpaca()
    }

    async fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        self.call_one(request)
            .await
            .map_err(|error| error.to_connector())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use mandate_accounting::InstrumentId;
    use mandate_executor::{
        ActivityCursor, BrokerOutcome, BrokerRequest, BrokerUnknown, ConnectorError,
    };

    use super::{RetryPolicy, TokioPause, TradingClient, query_value};
    use crate::error::TransportError;
    use crate::http::{HttpRequest, Method, Response, TradingTransport};

    /// Records every path it is handed and answers an empty page.
    #[derive(Default)]
    struct Recorder {
        sent: RefCell<Vec<String>>,
    }

    impl TradingTransport for Recorder {
        async fn send(&self, request: &HttpRequest) -> Result<Response, TransportError> {
            self.sent
                .borrow_mut()
                .push(request.path_and_query().to_owned());
            Ok(Response {
                status: 200,
                body: b"[]".to_vec(),
            })
        }
    }

    #[tokio::test]
    async fn the_activities_read_sends_its_cursor_encoded() {
        let client = TradingClient::new(Recorder::default(), TokioPause, RetryPolicy::default());
        let answer = client
            .call_one(&BrokerRequest::ListActivities {
                since: ActivityCursor("x&side=sell".to_owned()),
            })
            .await;
        assert!(answer.is_ok(), "an empty page is an answer: {answer:?}");
        let sent = client.transport.sent.borrow().clone();
        assert_eq!(
            sent.last()
                .map(|path| path.ends_with("&page_token=x%26side%3Dsell")),
            Some(true),
            "the cursor reaches the query encoded, never as a second parameter: {sent:?}"
        );
    }

    #[test]
    fn a_cursor_is_encoded_so_it_cannot_add_a_parameter_or_end_the_query() {
        assert_eq!(
            query_value("20260926233000000::2222-aB.c_d~e"),
            "20260926233000000::2222-aB.c_d~e",
            "the activity alphabet and the rest of RFC 3986's unreserved set pass unchanged"
        );
        assert_eq!(
            query_value("x&side=sell"),
            "x%26side%3Dsell",
            "`&` and `=` cannot start another parameter"
        );
        assert_eq!(query_value("a/b%c#d e+f"), "a%2Fb%25c%23d%20e%2Bf");
        assert_eq!(query_value(""), "");
    }

    /// Records every path it is handed and answers one fixed status, with a reject's body.
    struct Answering {
        status: u16,
        sent: RefCell<Vec<String>>,
    }

    impl Answering {
        fn with(status: u16) -> TradingClient<Self, TokioPause> {
            let transport = Self {
                status,
                sent: RefCell::new(Vec::new()),
            };
            TradingClient::new(transport, TokioPause, RetryPolicy::default())
        }
    }

    impl TradingTransport for Answering {
        async fn send(&self, request: &HttpRequest) -> Result<Response, TransportError> {
            self.sent
                .borrow_mut()
                .push(request.path_and_query().to_owned());
            Ok(Response {
                status: self.status,
                body: br#"{"code":40410000,"message":"refused"}"#.to_vec(),
            })
        }
    }

    #[tokio::test]
    async fn a_close_position_the_allowlist_refuses_is_not_sent_and_never_a_reject() {
        let client = Answering::with(404);
        let answer = client
            .account_wide(HttpRequest::account_wide_for_tests(
                Method::Delete,
                "/v2/positions/BTC/USD",
            ))
            .await;
        assert_eq!(
            answer.map_err(|error| error.to_connector()),
            Err(ConnectorError::NotSent {
                code: "refused_path"
            }),
            "a two-segment path is our unbuildable URL, never the broker refusing to reduce \
             risk (rule 13, DEC-133 item 32)"
        );
        assert!(
            client.transport.sent.borrow().is_empty(),
            "and no request left the process"
        );
        let ordinary = client
            .account_wide(HttpRequest::account_wide_for_tests(
                Method::Delete,
                "/v2/orders/b-1",
            ))
            .await;
        assert_eq!(
            ordinary.map_err(|error| error.to_connector()),
            Err(ConnectorError::NotSent {
                code: "refused_path"
            }),
            "nor does the account-wide constructor build an ordinary endpoint's request"
        );
        assert!(client.transport.sent.borrow().is_empty());
    }

    #[tokio::test]
    async fn a_crypto_close_is_sent_to_the_pair_without_its_slash() -> Result<(), String> {
        let client = Answering::with(200);
        let btc = InstrumentId::new("BTC/USD").map_err(|e| format!("{e:?}"))?;
        let answer = client
            .account_wide(HttpRequest::close_position_for_tests(&btc))
            .await;
        assert!(
            matches!(answer, Ok(BrokerOutcome::AccountWideAccepted)),
            "a kill switch closes a crypto position like any other: {answer:?}"
        );
        assert_eq!(
            *client.transport.sent.borrow(),
            vec!["/v2/positions/BTCUSD".to_owned()],
            "once, on the one path Alpaca answers for the pair (#195 finding 1)"
        );
        Ok(())
    }

    #[tokio::test]
    async fn an_overloaded_or_failing_broker_leaves_an_account_wide_call_unknown() {
        for status in [429, 503] {
            let client = Answering::with(status);
            let answer = client
                .account_wide(HttpRequest::account_wide_for_tests(
                    Method::Delete,
                    "/v2/orders",
                ))
                .await;
            assert_eq!(
                answer.map_err(|error| error.to_connector()),
                Err(ConnectorError::Unknown(BrokerUnknown::Ambiguous)),
                "a {status} on the cancel-all is an unknown outcome the executor queries, never \
                 a settled reject (rule 3, DEC-133 item 10)"
            );
            assert_eq!(client.transport.sent.borrow().len(), 1, "it was sent once");
        }
        let client = Answering::with(404);
        let answer = client
            .account_wide(HttpRequest::account_wide_for_tests(
                Method::Delete,
                "/v2/positions/AAPL",
            ))
            .await;
        assert!(
            matches!(answer, Ok(BrokerOutcome::Rejected(ref reject)) if reject.http_status == 404),
            "while a 404 on an allowlisted close is the broker's own refusal: {answer:?}"
        );
    }
}
