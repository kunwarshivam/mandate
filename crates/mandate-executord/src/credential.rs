//! The broker an executor runs over and its paper credential (DEC-2306 items 1 to 3).
//!
//! The run configuration chooses the broker: [`BrokerChoice::Fixture`], the default, or
//! [`BrokerChoice::AlpacaPaper`], Alpaca's paper trading host through `mandate-alpaca`'s
//! transport. The paper credential is the OAuth access token the vault leases to this account's
//! connection, in lane X's token entry (DEC-2624), and only that: the connection is the one the
//! account stream's latest connect check passed for, and a lease the vault refused or an entry
//! that does not decode refuses the start, before the process takes the account stream
//! (`AGENTS.md` rules 3 and 7). The token reaches the paper host only as a bearer header.
//!
//! The start in `alpaca-paper` runs in this order: [`paper_settings`] from the flags,
//! [`vault_config`] with the vault's CA from the mount, [`vault_login`] by Kubernetes auth with
//! the pod's ServiceAccount token (the coordinator's row 23, V2's option B), then
//! [`paper_broker`], which takes the lease and builds the broker, all before the process takes the
//! account stream. Each pass [`RunLease::pass`] ticks the lease, and [`lease_verdict`] says
//! whether the run goes on: a lost credential, and a rotated one, which the supervisor's restart
//! leases afresh, drain the process and stop it.
//!
//! [`AlpacaQuotes`] reads the latest quote of an instrument whose exit waits on a price from
//! Alpaca's market-data host over the same transport and credential (DEC-2359 items 2 and 5).

use std::path::PathBuf;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_alpaca::{
    AlpacaPaperHttp, Credentials, DataClient, DataTransport, Pause, RetryPolicy, TokioPause,
    TradingClient, TradingTransport,
};
use mandate_canon::{Value, parse};
use mandate_connections::account_entry::BrokerAccountId;
use mandate_connections::record::ConnectionId;
use mandate_connections::token_entry::decode;
use mandate_connections::vault::AccessToken;
use mandate_executor::{AccountScope, MarketObservation, RiskClock};
use mandate_journal::StreamId;
use mandate_num::{Adverse, Fraction};
use mandate_store::Store;
use mandate_time::UtcNanos;
use mandate_vault::SourceError;
use mandate_vault::kubernetes::service_account_name;
use mandate_vault::lease::{Lease, LeaseKeeper};
use mandate_vault::mounted::read_mounted_ca;
use mandate_vault::openbao::OpenBaoConfig;
use mandate_vault::policy::{Identity, Mounts};
use mandate_vault::source::SecretSource;
use secrecy::{ExposeSecret, SecretString};

use crate::Wiring;
use crate::alpaca::AlpacaBroker;
use crate::fixture;
use crate::lease::{Held, Leased, runtime};
use crate::quotes::{QuoteBounds, QuoteFeed};

/// Which broker the process runs over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerChoice {
    /// The in-process fixture paper broker (DEC-2007 item 1).
    Fixture,
    /// Alpaca's paper trading host (DEC-2306 item 1).
    AlpacaPaper,
}

/// The broker `flag`, the value of `--broker`, names: none is the fixture, and anything but
/// `fixture` or `alpaca-paper` is `broker_unknown`.
///
/// # Errors
/// `broker_unknown`.
pub fn broker_choice(flag: Option<&str>) -> Result<BrokerChoice, &'static str> {
    match flag {
        None | Some("fixture") => Ok(BrokerChoice::Fixture),
        Some("alpaca-paper") => Ok(BrokerChoice::AlpacaPaper),
        Some(_) => Err("broker_unknown"),
    }
}

/// The connection the account stream's latest connect check passed for: the
/// `ConnectionChecked` record last appended there, every check of which passed.
///
/// # Errors
/// `connection_unchecked` when there is none, or the latest failed; a journal error's code when
/// the stream cannot be read.
pub fn checked_connection<S: Store>(
    store: &S,
    scope: &AccountScope,
) -> Result<ConnectionId, &'static str> {
    let stream = StreamId::parse(&format!("acct:{}:{}", scope.workspace.0, scope.account.0))
        .ok_or("stream_unavailable")?;
    let rows = store.rows(&stream).ok_or("stream_unavailable")?;
    let latest = rows
        .iter()
        .rev()
        .find(|row| row.event_type == "ConnectionChecked")
        .ok_or("connection_unchecked")?;
    let payload = parse(&latest.body)
        .ok()
        .and_then(|body| body.get("payload").cloned())
        .ok_or("connection_record_unreadable")?;
    let passed = payload
        .get("results")
        .and_then(Value::as_array)
        .is_some_and(|results| {
            !results.is_empty()
                && results
                    .iter()
                    .all(|result| result.get("result").and_then(Value::as_str) == Some("passed"))
        });
    if !passed {
        return Err("connection_unchecked");
    }
    payload
        .get("connection_id")
        .and_then(Value::as_str)
        .and_then(|id| ConnectionId::new(id).ok())
        .ok_or("connection_record_unreadable")
}

/// [`checked_connection`], when it is `own`, the connection the executor runs as: a run never
/// leases a connection whose check another connection's executor wrote.
///
/// # Errors
/// [`checked_connection`]'s codes, and `connection_mismatch` for a checked connection that is
/// not `own`.
pub fn own_checked_connection<S: Store>(
    store: &S,
    scope: &AccountScope,
    own: &ConnectionId,
) -> Result<ConnectionId, &'static str> {
    let checked = checked_connection(store, scope)?;
    if checked == *own {
        Ok(checked)
    } else {
        Err("connection_mismatch")
    }
}

/// The access token a leased secret holds: DEC-2624's token entry, decoded by its one reader.
///
/// # Errors
/// `credential_malformed` for any other bytes; it names nothing of them.
pub fn read_token(secret: &[u8]) -> Result<AccessToken, &'static str> {
    decode(secret)
        .map(|entry| entry.token)
        .map_err(|_| "credential_malformed")
}

/// The access token `lease` holds, with the keeper that renews it.
///
/// # Errors
/// `credential_unavailable` when the vault refused the lease, and [`read_token`]'s refusal when
/// its secret is not a token entry. Neither names the secret.
pub fn paper_token(
    lease: Result<Lease, SourceError>,
) -> Result<(AccessToken, LeaseKeeper), &'static str> {
    let lease = lease.map_err(|_| "credential_unavailable")?;
    let token = read_token(lease.secret.expose_secret())?;
    Ok((token, LeaseKeeper::new(lease)))
}

/// The transport to Alpaca's paper trading host that sends `token` as its bearer credential.
///
/// # Errors
/// `transport_unavailable` when the HTTPS client cannot be built.
pub fn paper_transport(token: AccessToken) -> Result<AlpacaPaperHttp, &'static str> {
    AlpacaPaperHttp::new(Credentials::bearer(token.0)).map_err(|_| "transport_unavailable")
}

/// The broker's own id of the account `transport` reaches, by `mandate-alpaca`'s one read of it
/// (`GET /v2/account`'s `id`), for the exchanged executor's connect check (connections spec §5.2
/// step 4, DEC-2339).
///
/// # Errors
/// `account_unreadable` when the host does not answer it, or `runtime_unavailable`; neither
/// names the id.
pub fn paper_account<T: TradingTransport>(transport: T) -> Result<BrokerAccountId, &'static str> {
    let client = TradingClient::new(transport, TokioPause, RetryPolicy::default());
    runtime()?
        .block_on(client.account_id())
        .map_err(|_| "account_unreadable")
}

/// Where the vault is and where its CA is mounted: `--vault-address` and `--vault-dir`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultMount {
    pub address: String,
    pub dir: PathBuf,
}

/// The vault mount a run on Alpaca's paper host needs, from `flag` (the value after a flag):
/// `None` for the fixture broker, which needs none.
///
/// # Errors
/// [`broker_choice`]'s refusal; `broker_conflict` for `alpaca-paper` with `--fixture`, whose
/// journal forgets everything at exit; `vault_address_missing` and `vault_dir_missing`.
pub fn paper_settings(
    flag: &dyn Fn(&str) -> Option<String>,
    fixture: bool,
) -> Result<Option<VaultMount>, &'static str> {
    if broker_choice(flag("--broker").as_deref())? == BrokerChoice::Fixture {
        return Ok(None);
    }
    if fixture {
        return Err("broker_conflict");
    }
    let address = flag("--vault-address").ok_or("vault_address_missing")?;
    let dir = flag("--vault-dir").ok_or("vault_dir_missing")?;
    Ok(Some(VaultMount {
        address,
        dir: PathBuf::from(dir),
    }))
}

/// The vault's configuration: `mount`'s address, the CA read whole from the mount's `ca.crt`, and
/// the cell's mounts.
///
/// # Errors
/// `vault_ca_unreadable` when the CA is not a mounted file the reader accepts.
pub fn vault_config(mount: &VaultMount) -> Result<OpenBaoConfig, &'static str> {
    let ca = read_mounted_ca(&mount.dir).map_err(|_| "vault_ca_unreadable")?;
    Ok(OpenBaoConfig {
        address: mount.address.clone(),
        ca_pem: ca.into_bytes(),
        mounts: Mounts::default(),
    })
}

/// The vault session of `connection`'s executor: `service_account_token` reads the pod's token
/// first, and only then does `login` dial the vault as [`Identity::Executor`] of `connection`
/// with it. Both are the caller's: `mandate-vault`'s reader and Kubernetes login in the process,
/// scripted ones in a test.
///
/// # Errors
/// `vault_credentials_unreadable` when the token cannot be read, and `vault_login_failed` when the
/// vault cannot be reached or refuses it.
pub fn vault_login<V>(
    service_account_token: impl FnOnce() -> Result<SecretString, SourceError>,
    login: impl FnOnce(Identity, SecretString) -> Result<V, SourceError>,
    connection: &ConnectionId,
) -> Result<V, &'static str> {
    let token = service_account_token().map_err(|_| "vault_credentials_unreadable")?;
    login(Identity::Executor(connection.clone()), token).map_err(|_| "vault_login_failed")
}

/// The variable the deployment manager sets to the ServiceAccount its pod runs as (DEC-2336).
pub const SUBJECT_ACCOUNT_ENV: &str = "MANDATE_SUBJECT_ACCOUNT";

/// Whether `configured`, the pod's [`SUBJECT_ACCOUNT_ENV`], is exactly the ServiceAccount
/// `connection`'s executor runs as, its id's bytes in lowercase hex, as
/// [`mandate_vault::kubernetes::service_account_name`] builds it (DEC-2060 item 2). Admission pins
/// the name's form but cannot compute the hex, so the executor checks it before it logs in, as
/// the token exchange does (DEC-1177 item 6, DEC-2336).
///
/// # Errors
/// `executor_subject_account` when `configured` is absent, empty or any other name.
pub fn subject_account(
    configured: Option<String>,
    connection: &ConnectionId,
) -> Result<(), &'static str> {
    let own = service_account_name(&Identity::Executor(connection.clone()))
        .map_err(|_| "executor_subject_account")?;
    if configured.as_deref() == Some(own.as_str()) {
        Ok(())
    } else {
        Err("executor_subject_account")
    }
}

/// The connection whose executor the pod runs as: `configured`, the pod's [`SUBJECT_ACCOUNT_ENV`],
/// read back from its lowercase hex, and kept only when [`subject_account`] then holds for it, so
/// an upper-case digit, an odd length or bytes that are no connection id are refused (DEC-2336,
/// DEC-2339). A manager-started executor names no connection otherwise, so this is its own.
///
/// # Errors
/// `executor_subject_account` for anything but one connection's own ServiceAccount name.
pub fn subject_connection(configured: Option<String>) -> Result<ConnectionId, &'static str> {
    let refused = "executor_subject_account";
    let text = configured.ok_or(refused)?;
    let bytes = text
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|digits| u8::from_str_radix(digits, 16).ok())
        })
        .collect::<Option<Vec<u8>>>()
        .ok_or(refused)?;
    let connection = String::from_utf8(bytes)
        .ok()
        .and_then(|id| ConnectionId::new(&id).ok())
        .ok_or(refused)?;
    subject_account(Some(text), &connection)?;
    Ok(connection)
}

/// The variable the workspace chart sets to the namespace the deployment manager creates the
/// executor's pod in (lane D1, K2).
pub const WORKSPACE_NAMESPACE_ENV: &str = "MANDATE_WORKSPACE_NAMESPACE";

/// The namespace the executor's Kubernetes login must be in: `configured`, the chart's
/// [`WORKSPACE_NAMESPACE_ENV`], checked before `projected` reads the pod's own namespace from the
/// downward API, and only when the two are the same. So the vault's answer is held to what the
/// chart configured, never to what the pod alone says it is.
///
/// # Errors
/// `workspace_namespace_missing` when `configured` is absent or empty, before `projected` is
/// read; `vault_credentials_unreadable` when the pod's namespace cannot be read;
/// `workspace_namespace_mismatch` when the two differ.
pub fn workspace_namespace(
    configured: Option<String>,
    projected: impl FnOnce() -> Result<String, SourceError>,
) -> Result<String, &'static str> {
    let configured = configured
        .filter(|name| !name.is_empty())
        .ok_or("workspace_namespace_missing")?;
    let projected = projected().map_err(|_| "vault_credentials_unreadable")?;
    if configured == projected {
        Ok(configured)
    } else {
        Err("workspace_namespace_mismatch")
    }
}

/// How a renewed secret is read: as the access token.
pub type TokenReader = fn(&[u8]) -> Result<AccessToken, &'static str>;

/// The broker on Alpaca's paper host over the token `vault` leases to `connection`, and the
/// lease the run holds it under. A lease whose secret does not decode is given back before the
/// start is refused.
///
/// # Errors
/// [`paper_token`]'s refusals and [`paper_transport`]'s; `runtime_unavailable`.
pub fn paper_broker<V: SecretSource>(
    vault: V,
    connection: &ConnectionId,
    now: UtcNanos,
) -> Result<(AlpacaBroker<AlpacaPaperHttp>, RunLease<V>), &'static str> {
    let runtime = runtime()?;
    let lease = runtime
        .block_on(vault.lease(connection, now))
        .map_err(|_| "credential_unavailable")?;
    let built = read_token(lease.secret.expose_secret())
        .and_then(paper_transport)
        .and_then(AlpacaBroker::new);
    match built {
        Ok(broker) => {
            let read: TokenReader = read_token;
            let leased = Leased::new(vault, LeaseKeeper::new(lease), read);
            Ok((broker, RunLease::new(leased)))
        }
        Err(refused) => {
            let _expires_unless_given_back = runtime.block_on(vault.revoke(lease));
            Err(refused)
        }
    }
}

/// Whether the run goes on after a lease tick: only while the credential is unchanged.
///
/// # Errors
/// `credential_rotated`, `credential_lost`, or the tick's own refusal: each drains the process and
/// stops it for its supervisor.
pub fn lease_verdict(tick: Result<Held<AccessToken>, &'static str>) -> Result<(), &'static str> {
    match tick? {
        Held::Unchanged => Ok(()),
        Held::Rotated(_) => Err("credential_rotated"),
        Held::Lost => Err(CREDENTIAL_LOST),
    }
}

/// The stop code of a run whose lease lost its credential: the lease expired, the vault refused a
/// renewal, or the renewed secret does not read. The run restricts the account before it drains
/// (DEC-2839).
pub const CREDENTIAL_LOST: &str = "credential_lost";

/// The lease a run holds its broker under, ticked once a pass until a tick stops the run.
pub struct RunLease<V> {
    leased: Leased<V, TokenReader>,
    stopped: Option<&'static str>,
}

impl<V: SecretSource> RunLease<V> {
    /// The run's hold on `leased`, which nothing has stopped yet.
    pub fn new(leased: Leased<V, TokenReader>) -> Self {
        Self {
            leased,
            stopped: None,
        }
    }

    /// The code that stops the run, or `None` while it goes on: the first tick [`lease_verdict`]
    /// refuses gives it, and every later pass answers it again without asking the vault.
    pub fn pass(&mut self, now: UtcNanos) -> Option<&'static str> {
        if self.stopped.is_none() {
            self.stopped = lease_verdict(self.leased.tick(now)).err();
        }
        self.stopped
    }

    /// Gives the lease back to the vault when the run stops.
    ///
    /// # Errors
    /// `revoke_failed` when the vault does not take it back; it then expires.
    pub fn give_back(self) -> Result<(), &'static str> {
        self.leased.revoke()
    }
}

/// The paper equity profile's quote bounds (trading-domain spec §4.2, DEC-2359 item 4): a quote
/// at most the reviewed rule set's IEX age old, ten seconds, and a spread of at most 2 % of the
/// bid, the collar's width for an instrument outside the liquid tier.
///
/// # Errors
/// `quote_bounds_invalid` when the spread bound does not read.
pub fn paper_quote_bounds() -> Result<QuoteBounds, &'static str> {
    Ok(QuoteBounds {
        max_age: Duration::from_secs(10),
        max_spread: Fraction::parse("0.02").map_err(|_| "quote_bounds_invalid")?,
    })
}

/// The paper serve path's wiring (DEC-2359 item 2): the fixture's, with the quote feed over
/// `broker`'s own transport and credential, judged by [`paper_quote_bounds`] on `clock`.
///
/// # Errors
/// The fixture wiring's refusal, `quote_bounds_invalid` when the bounds do not read, and
/// `runtime_unavailable` when the feed's runtime cannot be built.
pub fn paper_wiring<T, P>(
    broker: &AlpacaBroker<T>,
    clock: P,
    scope: &AccountScope,
) -> Result<Wiring, &'static str>
where
    T: TradingTransport + DataTransport + Clone + 'static,
    P: Pause + 'static,
{
    let mut wiring = fixture::wiring(scope)?;
    wiring.quotes = Box::new(AlpacaQuotes::new(
        DataClient::new(broker.transport(), clock),
        paper_quote_bounds()?,
    )?);
    Ok(wiring)
}

/// The latest-quote read of Alpaca's market-data host, over the connection's transport.
pub struct AlpacaQuotes<D, P> {
    client: DataClient<D, P>,
    bounds: QuoteBounds,
    runtime: tokio::runtime::Runtime,
}

impl<D: DataTransport, P: Pause> AlpacaQuotes<D, P> {
    /// The feed over `client`, judged by `bounds`.
    ///
    /// # Errors
    /// `runtime_unavailable` when the feed's runtime cannot be built.
    pub fn new(client: DataClient<D, P>, bounds: QuoteBounds) -> Result<Self, &'static str> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "runtime_unavailable")?;
        Ok(Self {
            client,
            bounds,
            runtime,
        })
    }
}

impl<D: DataTransport, P: Pause> QuoteFeed for AlpacaQuotes<D, P> {
    /// The instrument's latest quote at `now`: its bid, ask and sizes as the host answered them,
    /// observed at the earlier of its own stamp and `now`, so a quote stamped ahead of the clock
    /// never reads younger than it is.
    ///
    /// # Errors
    /// `quote_unread` for a quote the host does not answer, that does not read, or that is older
    /// than [`QuoteBounds::max_age`]. A quote that is not sane as trading spec §8.2 says,
    /// `0 < bid ≤ ask` and the ask no higher than `bid × (1 + max_spread)` rounded down, is
    /// observed with `sane` false, so a mark can restrict the instrument at once (§9.30); the
    /// process hands the core none of those (DEC-2359, DEC-2380 item 3). The code names neither the
    /// instrument nor a price.
    fn observe(
        &self,
        instrument: &InstrumentId,
        now: UtcNanos,
    ) -> Result<MarketObservation, &'static str> {
        let quote = self
            .runtime
            .block_on(self.client.latest_quote(instrument, self.bounds.max_age))
            .map_err(|_| "quote_unread")?;
        let sane = quote.bid <= quote.ask
            && quote
                .bid
                .collar_bound(self.bounds.max_spread, Adverse::Up)
                .is_ok_and(|widest| quote.ask <= widest);
        Ok(MarketObservation {
            instrument: instrument.clone(),
            bid: Some(quote.bid),
            bid_size: Some(quote.bid_size),
            ask: Some(quote.ask),
            last_trade: None,
            mark: None,
            sane,
            observed_at: RiskClock::from_secs(quote.at.min(now).secs()),
        })
    }

    /// Alpaca's IEX feed, the one paper reads (DEC-2359).
    fn feed(&self) -> &'static str {
        "iex"
    }
}
