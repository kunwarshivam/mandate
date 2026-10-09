#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The Alpaca **paper** trading connector
//! ([task brief](../../../docs/project/tasks/M6-K-executor-and-connector.md), backlog E7-2, E7-3,
//! E7-4): the `BrokerConnector` `mandate-executor` declares, behind an injected transport and an
//! injected clock.
//!
//! The shape is `mandate-marketdata`'s, deliberately: recorded fixtures, no network in any test,
//! and a path allowlist checked before anything is sent (ADR-0001 ES-19, ES-23). It is duplicated
//! rather than shared with market data because two hosts, two endpoint allowlists, and two
//! credential scopes meet here and one of them is on the order path — a shared crate would let a
//! market-data change reach the trading path (task brief Decisions needed 5).
//!
//! # The paper boundary
//!
//! [`http::PAPER_HOST`] is the only host compiled in. There is no live base URL in this crate at
//! all, a `live` cargo feature is forbidden, and there is no deposit, withdrawal, or transfer
//! endpoint in [`http::ENDPOINTS`] and no way to add one at runtime (ES-23, `AGENTS.md` rule 8:
//! no custody of funds). Live credentials would come only from the vault, and that path is M13's.
//!
//! # Reference data (E7-8)
//!
//! [`TradingClient::asset`] reads one instrument's asset record from the paper trading host, and
//! [`DataClient::latest_quote`] reads its latest quote from [`data::DATA_HOST`], the one other
//! host compiled in (ES-23), through a request type and a transport trait of its own so neither
//! host can be sent the other's request (DEC-168). [`DataClient::recent_minute_bars`] reads one
//! equity's complete one-minute IEX bars inside a short window before the clock, one page only,
//! through the same transport trait and its own request type (DEC-471).
//!
//! # Credentials and personal data
//!
//! Credentials are read through an injected lookup, held as `SecretString`, sent only as headers
//! marked sensitive, and printed by no `Debug`. They are never stored, written, journaled, or
//! logged: not in a draft, not in a `BrokerExchangeRecorded` payload, not in an error message
//! (which carries no URL, header, or body), not in a fixture, and not in a file this code writes
//! (`AGENTS.md` rule 7, ES-09).
//!
//! The broker's `account_number` and account `id` are personal data (journal spec §6.4). The
//! redaction pass in [`record`] replaces both with opaque `pii_refs` entries **before** the bytes
//! are hashed or stored, and the executor's own [`mandate_executor::BrokerAccount`] has nowhere
//! to put either.
//!
//! # Numbers
//!
//! Broker numbers never pass through an `f64` (ES-23): every quantity, price, and amount arrives
//! as raw text and is parsed by `mandate-num`. A JSON number, an exponent, or more places than
//! the increment allows is a typed [`error::WireError`] with a stable code.

pub mod account_rules;
pub mod client;
pub mod data;
pub mod error;
pub mod http;
pub mod profile;
pub mod read;
pub mod record;
pub mod wire;

pub use account_rules::{AccountRules, DeclaredRegime, alpaca as alpaca_account_rules};
pub use client::{Pause, RetryPolicy, TokioPause, TradingClient};
pub use data::{BarsRequest, DATA_HOST, DataClient, DataTransport, MAX_BARS_WINDOW, QuoteRequest};
pub use error::{
    ClientError, CredentialsError, HttpSetupError, ReadError, TransportError, WireError,
};
pub use http::{
    AlpacaPaperHttp, Credentials, ENDPOINTS, Endpoint, HttpRequest, KEY_ID_VAR, Method, PAPER_HOST,
    Response, SECRET_VAR, TradingTransport, endpoint_for, is_paper_trading_path,
};
pub use profile::alpaca as alpaca_profile;
pub use read::{Asset, AssetSnapshot, Exchange, Feed, LatestQuote, MinuteBar, MinuteBars};
pub use record::{Direction, INLINE_LIMIT, RecordedBody, RecordedExchange};
