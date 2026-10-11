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
    ClientOrderId, ConnectorError, OrderState, StatusMapping,
};
use mandate_time::UtcNanos;

use mandate_accounting::{InstrumentId, Side};
use mandate_num::Qty;

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
                Ok(BrokerOutcome::OpenOrders(self.open_orders().await?))
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

    /// Cancels one of our orders. Alpaca cancels by **its** order id, so an order Alpaca holds
    /// by our own name is looked up by that client order id first, and an order that is not
    /// there answers what the lookup said. An id that names an entry (§2.3: everything before
    /// its last `-p`) is protective — the platform's own handle or the id of a protective order
    /// the platform sent — and is resolved through the open-orders snapshot alone
    /// ([`Self::cancel_protective`]), never by asking Alpaca for the id the handle carries.
    ///
    /// An accepted `DELETE` is only a request taken, not a confirmation, so the order is read back
    /// once (DEC-867 item 3). A read-back that shows the order `canceled` answers
    /// [`BrokerOutcome::CancelAccepted`], the confirmation of trading-domain spec §5.7's
    /// `PendingCancel --> Canceled: confirmed`. Any other read-back is answered as read, for
    /// example the order still in `pending_cancel`, which can yet fill, so the executor keeps the
    /// cancel unconfirmed (§5.4). A read-back that fails is that error, an unknown outcome, never
    /// a confirmation. A refused `DELETE` — the order filled first — is answered with the order's
    /// own state, read back at once, because that is the fact the executor folds (§5.7's
    /// `PendingCancel --> Filled`).
    async fn cancel(&self, client_order_id: &str) -> Result<BrokerOutcome, ClientError> {
        match protective_entry(client_order_id) {
            None => self.cancel_named(client_order_id).await,
            Some(handle) => self.cancel_protective(client_order_id, &handle).await,
        }
    }

    /// The cancel of an id Alpaca holds by that very name: looked up by our client order id,
    /// deleted by the broker order id, read back once by our name, answered by what that read
    /// says (DEC-867 items 2 and 3).
    async fn cancel_named(&self, client_order_id: &str) -> Result<BrokerOutcome, ClientError> {
        let order = match self.order_by_client_id(client_order_id).await? {
            BrokerOutcome::Order(order) => order,
            other => return Ok(other),
        };
        let path = format!("/v2/orders/{}", order.broker_order_id);
        let response = self.send(Method::Delete, path, None).await?;
        match response.status {
            200..=299 => match self.order_by_client_id(client_order_id).await? {
                BrokerOutcome::Order(read_back) if read_back.status == "canceled" => {
                    Ok(BrokerOutcome::CancelAccepted {
                        client_order_id: client_order_id.to_owned(),
                    })
                }
                read_back => Ok(read_back),
            },
            429 | 500..=599 => Err(BrokerUnknown::Ambiguous.into()),
            _ => self.order_by_client_id(client_order_id).await,
        }
    }

    /// The cancel of `{entry}-p{record}` — the platform's handle for a protective placement,
    /// which no Alpaca order carries (trading-domain spec §2.3, DEC-878 item 1) — resolved
    /// through the open-orders snapshot alone (`nested=true`), so no request ever names the
    /// handle on its way to a broker that can only answer it with a 404.
    ///
    /// A protective order the platform itself sent, whose `client_order_id` the handle names (a
    /// partly filled bracket's OCO, a re-placed stop-limit; DEC-878 item 5), is cancelled by the
    /// broker order id the snapshot lists it under. Otherwise the entry the handle names must be
    /// listed there, by the entry's own `client_order_id` and `filled`, with exactly two resting
    /// sell legs nested under it — the stop and the take-profit Alpaca named itself (DEC-878
    /// item 2) — and **each leg is cancelled by its own broker order id**, one `DELETE` each,
    /// the broker cancelling a sibling on either `DELETE` being a fact this fold reads through
    /// the read-backs and never a step it relies on ([`bracket_legs`], #1292's contract item 1).
    ///
    /// Every other shape the snapshot cannot vouch for fails closed (DEC-878 item 5, DEC-176):
    /// no `DELETE` is sent, and the answer is neither a confirmation nor an absence — a bracket
    /// the legs of which cannot be shown resting whole stays cancelled by nobody, so the
    /// placement stays held or doubted rather than assumed cancelled (rule 3).
    async fn cancel_protective(
        &self,
        handle: &str,
        entry: &ClientOrderId,
    ) -> Result<BrokerOutcome, ClientError> {
        let snapshot = self.open_orders().await?;
        if let Some(ours) = snapshot
            .iter()
            .find(|open| open.client_order_id.as_deref() == Some(handle))
        {
            return self.cancel_resolved(handle, ours).await;
        }
        let Some(parent) = snapshot
            .iter()
            .find(|open| open.client_order_id.as_deref() == Some(entry.as_str()))
        else {
            return Err(WireError::NotInterpreted {
                field: "bracket_legs",
                story: "E7-4",
            }
            .into());
        };
        let (stop, take_profit) = match bracket_legs(parent) {
            Some(legs) => legs,
            None => {
                return Err(WireError::NotInterpreted {
                    field: "bracket_legs",
                    story: "E7-4",
                }
                .into());
            }
        };
        for leg in [&stop, &take_profit] {
            let outcome = self.cancel_resolved(handle, leg).await?;
            if !matches!(outcome, BrokerOutcome::CancelAccepted { .. }) {
                return Ok(outcome);
            }
        }
        Ok(BrokerOutcome::CancelAccepted {
            client_order_id: handle.to_owned(),
        })
    }

    /// Cancels one order the snapshot resolved — a protective order of ours the broker holds by
    /// the handle, or one leg of a filled bracket — by **its own** broker order id, then reads it
    /// back once: the read's `canceled` is the only confirmation (DEC-867 items 2 and 3), any
    /// other status the read shows answers the order's own state so the executor keeps the cancel
    /// unconfirmed (§5.7's `PendingCancel --> Filled: filled first`), and a failed read-back is
    /// an unknown outcome, never a confirmation.
    async fn cancel_resolved(
        &self,
        client_order_id: &str,
        order: &BrokerOrder,
    ) -> Result<BrokerOutcome, ClientError> {
        let path = format!("/v2/orders/{}", order.broker_order_id);
        let response = self.send(Method::Delete, path, None).await?;
        match response.status {
            429 | 500..=599 => Err(BrokerUnknown::Ambiguous.into()),
            _ => match self.order_by_broker_id(&order.broker_order_id).await? {
                read if read.status == "canceled" => Ok(BrokerOutcome::CancelAccepted {
                    client_order_id: client_order_id.to_owned(),
                }),
                read => Ok(BrokerOutcome::Order(read)),
            },
        }
    }

    /// Reads one order back by the broker's own order id, the read-back of a cancel the snapshot
    /// resolved (DEC-867 item 3): a success parses, and any other answer says nothing about the
    /// order, so it is an unknown outcome rather than a confirmation or an absence.
    async fn order_by_broker_id(&self, broker_order_id: &str) -> Result<BrokerOrder, ClientError> {
        let path = format!("/v2/orders/{broker_order_id}");
        let response = self.send(Method::Get, path, None).await?;
        match response.status {
            200..=299 => Ok(wire::order(&response.body)?),
            _ => Err(BrokerUnknown::Ambiguous.into()),
        }
    }

    /// The open-orders read (`nested=true`), whole and in one page: every open order, oldest
    /// first, with bracket and OCO legs nested under their parent. Both the
    /// [`BrokerRequest::ListOpenOrders`] answer and the resolution of a protective placement's
    /// cancel read this page, the only shape DEC-878 item 2's rules and this cancel path run
    /// on. A full page may be a truncated one, and a cancel that missed an order would act on
    /// half a bracket, so a full page fails loudly instead (DEC-85).
    async fn open_orders(&self) -> Result<Vec<BrokerOrder>, ClientError> {
        let orders = wire::open_orders(&self.read(OPEN_ORDERS.to_owned()).await?.body)?;
        if orders.len() >= OPEN_ORDERS_PAGE {
            return Err(WireError::NotInterpreted {
                field: "open_orders_page",
                story: "E7-3",
            }
            .into());
        }
        Ok(orders)
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

/// The entry a protective id names — everything before its last `-p`, per trading-domain spec
/// §2.3's grammar — when that reads back as an id of ours ([`ClientOrderId::parse`],
/// `protected_entry`); `None` for any other id, which Alpaca holds by that very name. The entry
/// a placement handle names is the only id this cancel path may ask Alpaca for on its behalf:
/// the handle itself is the platform's and never a broker id, and nothing derives a leg's broker
/// id from it (DEC-878 item 1).
fn protective_entry(raw: &str) -> Option<ClientOrderId> {
    ClientOrderId::parse(raw)
        .ok()
        .and_then(|id| id.protected_entry())
}

/// The two legs a filled entry of ours holds nested under it, exactly as DEC-878 item 2 vouches
/// them: the entry itself `filled`, exactly two legs nested under it, both resting sells —
/// nothing of either filled, §5.7's table mapping each leg's status to `Accepted` — one with its
/// `stop_price` set (the stop) and one sell limit with no stop price and its `limit_price` set
/// (the take-profit), each sized to the entry's own quantity. That quantity is the placement's
/// at the moment the entry is `filled` (§5.4's tranche model: a bracket's legs cover the fill),
/// which is this read's judgeable half of item 2(c)'s sizes-match-the-record rule; the recorded
/// prices are beyond what any connector can judge from the handle alone, and the executor vouches
/// them where the record lives, in `bracket_rests` and the in-doubt settlement
/// (`mandate-executor`'s `reconcile`), before this path is ever asked (#1292's contract item 2).
/// Any other shape answers `None`: none of it is cancelled, never `Absent`, never confirmed
/// (rule 3, DEC-878 item 5).
fn bracket_legs(parent: &BrokerOrder) -> Option<(BrokerOrder, BrokerOrder)> {
    if parent.status != "filled" || parent.legs.len() != 2 {
        return None;
    }
    let resting = |leg: &BrokerOrder| {
        leg.side == Side::Sell
            && leg.filled_qty == Qty::ZERO
            && leg.qty == parent.qty
            && matches!(
                wire::status(&leg.status),
                Ok(StatusMapping::Becomes(OrderState::Accepted))
            )
    };
    let stop = parent
        .legs
        .iter()
        .find(|leg| resting(leg) && leg.stop_price.is_some())?;
    let take_profit = parent
        .legs
        .iter()
        .find(|leg| resting(leg) && leg.stop_price.is_none() && leg.limit_price.is_some())?;
    Some((stop.clone(), take_profit.clone()))
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
    use std::collections::VecDeque;

    use mandate_accounting::InstrumentId;
    use mandate_executor::{
        ActivityCursor, BrokerOutcome, BrokerRequest, BrokerUnknown, ClientOrderId, ConnectorError,
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

    /// A host that answers every request with the one open-orders page it was built from and
    /// records each request as `METHOD path`: the read a protective placement's cancel resolves
    /// through, answered with whatever shape the page builds.
    struct Listing {
        page: Vec<u8>,
        sent: RefCell<Vec<String>>,
    }

    impl Listing {
        fn new(page: &str) -> TradingClient<Self, TokioPause> {
            let transport = Self {
                page: page.as_bytes().to_vec(),
                sent: RefCell::new(Vec::new()),
            };
            TradingClient::new(transport, TokioPause, RetryPolicy::default())
        }
    }

    impl TradingTransport for Listing {
        async fn send(&self, request: &HttpRequest) -> Result<Response, TransportError> {
            self.sent.borrow_mut().push(format!(
                "{} {}",
                request.method().as_str(),
                request.path_and_query()
            ));
            Ok(Response {
                status: 200,
                body: self.page.clone(),
            })
        }
    }

    /// The page of one `nested=true` open-orders read: the recording's entry, by its own
    /// `client_order_id`, with `legs` (already-rendered JSON) nested under it.
    fn entry_with(legs: &str) -> String {
        format!(
            r#"[{{"id":"569fca5f-d21f-461a-9e3f-21311cf912f0","client_order_id":"md-daea915c1fc0042bcc19a4caed","symbol":"AAPL","asset_class":"us_equity","side":"buy","order_type":"limit","time_in_force":"gtc","qty":"1","filled_qty":"1","limit_price":"1","stop_price":null,"status":"filled","created_at":"2026-09-26T23:20:11.63400562Z","replaced_by":null,"legs":{legs}}}]"#
        )
    }

    /// One of the recording's own sell legs: `qty` of it, `filled` so far, `status`, and the
    /// prices a stop or a take-profit carries.
    fn leg(
        id: &str,
        qty: &str,
        filled: &str,
        status: &str,
        stop_price: Option<&str>,
        limit_price: Option<&str>,
    ) -> String {
        let price = |field: &str, value: Option<&str>| match value {
            Some(raw) => format!(r#""{field}":"{raw}""#),
            None => format!(r#""{field}":null"#),
        };
        format!(
            r#"{{"id":"{id}","client_order_id":"{id}-client","symbol":"AAPL","asset_class":"us_equity","side":"sell","order_type":"limit","time_in_force":"gtc","qty":"{qty}","filled_qty":"{filled}",{limit},{stop},"status":"{status}","created_at":"2026-09-26T23:20:11.63400562Z","replaced_by":null,"legs":null}}"#,
            limit = price("limit_price", limit_price),
            stop = price("stop_price", stop_price),
        )
    }

    /// The recorded `submit_bracket_accepted` bracket's two legs, both resting as the recording
    /// holds them, for the quantities and prices given.
    fn resting_legs(
        stop_qty: &str,
        stop_price: &str,
        take_profit_qty: &str,
        limit: &str,
    ) -> String {
        format!(
            "[{},{}]",
            leg(
                "fe22b5be-837b-4684-b222-0090fdd838cc",
                take_profit_qty,
                "0",
                "held",
                None,
                Some(limit),
            ),
            leg(
                "30ab5fda-7d66-4263-82cf-2a1d40bf404f",
                stop_qty,
                "0",
                "held",
                Some(stop_price),
                None,
            ),
        )
    }

    /// #1292's contract item 1's fail-closed half beyond the pinned nine: DEC-878 item 2(c)'s
    /// sizes are judged as far as the snapshot reaches — each leg sized to the entry's own
    /// quantity, the placement's once the entry is `filled` (§5.4) — and a shape resting by
    /// status but partly filled, or carrying no stop and no take-profit between its two legs,
    /// is not one this path vouches for. Each case sends nothing but the open-orders read: no
    /// request names the handle, no `DELETE` goes out, and the answer is neither a confirmation
    /// nor an absence, so the placement stays held or doubted (rule 3, DEC-878 item 5).
    #[tokio::test]
    async fn a_bracket_shape_the_snapshot_cannot_vouch_for_is_never_cancelled() -> Result<(), String>
    {
        let handle = "md-daea915c1fc0042bcc19a4caed-p1";
        let pages = [
            (
                "both legs sized 2 against the entry's 1",
                entry_with(&resting_legs("2", "0.8", "2", "999")),
            ),
            (
                "the take-profit `held` with 0.3 of it already filled",
                entry_with(&format!(
                    "[{},{}]",
                    leg(
                        "fe22b5be-837b-4684-b222-0090fdd838cc",
                        "1",
                        "0.3",
                        "held",
                        None,
                        Some("999"),
                    ),
                    leg(
                        "30ab5fda-7d66-4263-82cf-2a1d40bf404f",
                        "1",
                        "0",
                        "held",
                        Some("0.8"),
                        None,
                    ),
                )),
            ),
            (
                "two stops nested, no take-profit",
                entry_with(&format!(
                    "[{},{}]",
                    leg(
                        "fe22b5be-837b-4684-b222-0090fdd838cc",
                        "1",
                        "0",
                        "held",
                        Some("999"),
                        None,
                    ),
                    leg(
                        "30ab5fda-7d66-4263-82cf-2a1d40bf404f",
                        "1",
                        "0",
                        "held",
                        Some("0.8"),
                        None,
                    ),
                )),
            ),
            ("no legs nested under the filled entry", entry_with("[]")),
        ];
        for (case, page) in pages {
            let client = Listing::new(&page);
            let answer = client
                .call_one(&BrokerRequest::Cancel {
                    client_order_id: ClientOrderId::parse(handle)
                        .map_err(|error| format!("{error:?}"))?,
                })
                .await;
            let refused = answer.map_err(|error| error.to_connector());
            assert_eq!(
                refused,
                Err(ConnectorError::Unreadable {
                    code: "not_interpreted"
                }),
                "{case}: a bracket this path cannot vouch for is not cancelled, and the refusal \
                 is loud"
            );
            assert_eq!(
                client.transport.sent.replace(Vec::new()),
                vec!["GET /v2/orders?status=open&limit=500&direction=asc&nested=true".to_owned()],
                "{case}: the open-orders read is the only request a protective placement's cancel \
                 resolves through, no request names the handle, and no leg is deleted"
            );
        }
        Ok(())
    }

    /// A scripted transport: one `METHOD path` line recorded per request, and the answers given
    /// in order after the open-orders page — a cancel this path resolves sends nothing else.
    struct Scripted {
        page: Vec<u8>,
        lines: RefCell<Vec<String>>,
        answers: RefCell<VecDeque<(u16, Vec<u8>)>>,
    }

    impl Scripted {
        fn answering(page: &str, answers: &[(u16, String)]) -> TradingClient<Self, TokioPause> {
            let transport = Self {
                page: page.as_bytes().to_vec(),
                lines: RefCell::new(Vec::new()),
                answers: RefCell::new(
                    answers
                        .iter()
                        .map(|(status, body)| (*status, body.as_bytes().to_vec()))
                        .collect(),
                ),
            };
            TradingClient::new(transport, TokioPause, RetryPolicy::default())
        }
    }

    impl TradingTransport for Scripted {
        async fn send(&self, request: &HttpRequest) -> Result<Response, TransportError> {
            let path = request.path_and_query();
            self.lines
                .borrow_mut()
                .push(format!("{} {}", request.method().as_str(), path));
            let answered =
                if request.method() == Method::Get && path.starts_with("/v2/orders?status=open") {
                    Some(Response {
                        status: 200,
                        body: self.page.clone(),
                    })
                } else {
                    self.answers
                        .borrow_mut()
                        .pop_front()
                        .map(|(status, body)| Response { status, body })
                };
            answered.ok_or(TransportError::Timeout)
        }
    }
    /// What one leg's resolved path is expected to answer: the leg's own status, a failed
    /// read-back's unknown outcome, or the confirmation both legs' read-backs give the
    /// placement.
    #[derive(Debug, PartialEq, Eq)]
    enum Expected {
        BothLegsCanceled,
        LegShown(&'static str),
        Unknown,
    }

    /// One scripted case: its name, its open-orders page, the answers its non-page requests
    /// get, and what the cancel is expected to answer.
    type Case = (&'static str, String, Vec<(u16, String)>, Expected);

    /// DEC-867 items 2 and 3 through the legs a snapshot resolves (#1292's contract item 1):
    /// each leg's `DELETE` is confirmed by that leg's own read-back alone, so a read-back like
    /// `pending_cancel` — a cancel the broker took but has not shown canceled, which can still
    /// fill — is answered as that leg's order and never as a confirmation; a failed `DELETE` or
    /// a failed read-back is an unknown outcome, never a confirmation; and a refused `DELETE`
    /// is answered by the leg's own read-back, which is also how a sibling-cancellation the
    /// broker already performed (DEC-878's parenthetical) is folded — a fact read off the leg,
    /// never a step relied on. A protective order the platform itself sent, which the snapshot
    /// lists under the handle (a partly filled bracket's OCO, DEC-878 item 5), takes the same
    /// single-order path, resolved by its broker order id alone.
    #[tokio::test]
    async fn a_resolved_cancel_is_written_back_by_the_leg_not_the_request() -> Result<(), String> {
        let handle = "md-daea915c1fc0042bcc19a4caed-p1";
        let take_profit = "fe22b5be-837b-4684-b222-0090fdd838cc";
        let stop = "30ab5fda-7d66-4263-82cf-2a1d40bf404f";
        let read_back = |id: &str, status: &str| {
            format!(
                r#"{{"id":"{id}","client_order_id":"{id}-own","symbol":"AAPL","asset_class":"us_equity","side":"sell","order_type":"limit","time_in_force":"gtc","qty":"1","filled_qty":"0","limit_price":null,"stop_price":null,"status":"{status}","created_at":"2026-09-26T23:20:11.63400562Z","replaced_by":null,"legs":null}}"#
            )
        };
        let empty = String::new();
        let canceled = |id: &str| read_back(id, "canceled");
        let bracket = entry_with(&resting_legs("1", "0.8", "1", "999"));
        let oco = format!(
            r#"[{{"id":"6f88a9b0-a7bd-4d0e-8acc-2ba0fedc1111","client_order_id":"{handle}","symbol":"AAPL","asset_class":"us_equity","side":"sell","order_type":"limit","time_in_force":"gtc","qty":"1","filled_qty":"0","limit_price":"999","stop_price":null,"status":"accepted","created_at":"2026-09-26T23:20:11.63400562Z","replaced_by":null,"legs":null}}]"#
        );
        let cases: Vec<Case> = vec![
            (
                "both legs taken and read back canceled",
                bracket.clone(),
                vec![
                    (204, empty.clone()),
                    (200, canceled(stop)),
                    (204, empty.clone()),
                    (200, canceled(take_profit)),
                ],
                Expected::BothLegsCanceled,
            ),
            (
                "the stop's read-back still pending_cancel",
                bracket.clone(),
                vec![
                    (204, empty.clone()),
                    (200, read_back(stop, "pending_cancel")),
                ],
                Expected::LegShown("pending_cancel"),
            ),
            (
                "the stop's DELETE refused, the sibling already canceled it",
                bracket.clone(),
                vec![
                    (422, "{\"message\":\"unable to cancel\"}".to_owned()),
                    (200, canceled(stop)),
                    (204, empty.clone()),
                    (200, canceled(take_profit)),
                ],
                Expected::BothLegsCanceled,
            ),
            (
                "the stop's DELETE met a failing broker",
                bracket.clone(),
                vec![(503, "{\"message\":\"unavailable\"}".to_owned())],
                Expected::Unknown,
            ),
            (
                "the stop's read-back refused: unknown, never a confirmation",
                bracket,
                vec![
                    (204, empty.clone()),
                    (404, "{\"message\":\"gone\"}".to_owned()),
                ],
                Expected::Unknown,
            ),
            (
                "a sent OCO listed under the handle, resolved by its broker id",
                oco,
                vec![
                    (204, empty.clone()),
                    (200, canceled("6f88a9b0-a7bd-4d0e-8acc-2ba0fedc1111")),
                ],
                Expected::BothLegsCanceled,
            ),
        ];
        for (case, page, answers, expected) in cases {
            let client = Scripted::answering(&page, &answers);
            let answer = client
                .call_one(&BrokerRequest::Cancel {
                    client_order_id: ClientOrderId::parse(handle)
                        .map_err(|error| format!("{error:?}"))?,
                })
                .await;
            match expected {
                Expected::BothLegsCanceled => assert_eq!(
                    answer,
                    Ok(BrokerOutcome::CancelAccepted {
                        client_order_id: handle.to_owned()
                    }),
                    "{case}: every leg read back canceled, so the placement's cancel is the \
                     confirmation both give (DEC-867 item 3)"
                ),
                Expected::LegShown(status) => {
                    let shown = match &answer {
                        Ok(BrokerOutcome::Order(read)) => Some(read.status.clone()),
                        _ => None,
                    };
                    assert_eq!(
                        shown,
                        Some(status.to_owned()),
                        "{case}: the leg's own read-back answers the cancel, never a confirmation \
                         (DEC-867 item 2): {answer:?}"
                    );
                }
                Expected::Unknown => {
                    let refused = answer.map_err(|error| error.to_connector());
                    assert_eq!(
                        refused,
                        Err(ConnectorError::Unknown(BrokerUnknown::Ambiguous)),
                        "{case}: a failed DELETE or read-back is an unknown outcome, never a \
                         confirmation and never an absence (DEC-867 item 3)"
                    );
                }
            }
            let sent = client.transport.lines.replace(Vec::new());
            assert!(
                sent.iter().all(|line| !line.contains(handle)),
                "{case}: no request names the handle (DEC-878 item 1): {sent:?}"
            );
        }
        Ok(())
    }
}
