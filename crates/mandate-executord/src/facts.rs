//! The broker's facts the binding gate reads and the journal does not hold (E21-30 X1b-3 G2b-2,
//! DEC-2380 item 2): the instrument's asset record and halt status, and the account's day-trade
//! rules, as [`BrokerFacts`], over `mandate-alpaca`'s client and whatever transport the run's
//! broker has, Alpaca's paper host or the fixture ([`crate::paper::FixturePaper`]).
//!
//! Each answer is read when the gate asks for it, as the quote is ([`crate::quotes`]), so it is as
//! fresh as the opening that reads it; nothing is cached, and nothing read at start answers later.
//! The gate asks only for an opening, never for an exit (`AGENTS.md` rule 13). Every read that
//! fails, every answer that does not read, and every fact no source states is `None`, which holds
//! the opening (rule 3):
//! - the asset record is `GET /v2/assets/{symbol}` (trading spec §3.1), refused as
//!   `mandate_alpaca::read::asset` refuses it;
//! - the account's rules are the connector's declaration (trading spec §7.2, DEC-840 item 1) over
//!   `GET /v2/account`: [`account_rules`];
//! - the halt status is the venue's ([`HaltStatus`]). Alpaca serves no trading-status channel over
//!   REST, so its halts are unread ([`NoStatusFeed`]); the fixture is the venue, and halts nothing.
//!
//! Nothing here logs a symbol, a figure or an account field (`AGENTS.md` rule 7, ADR-0001 ES-20).

use mandate_accounting::InstrumentId;
use mandate_alpaca::{
    AccountRules as DeclaredRules, DeclaredRegime, RetryPolicy, TokioPause, TradingClient,
    TradingTransport, alpaca_account_rules,
};
use mandate_executor::{BrokerAccount, BrokerOutcome, BrokerRequest};
use mandate_risk::DayTradeRegime;
use tokio::runtime::Runtime;

use crate::alpaca::AlpacaBroker;
use crate::deployment::BrokerFacts;
use crate::gate::{AccountRules, AssetRecord};
use crate::paper::FixturePaper;

/// The gate's account rules from the connector's `declared` rules and the broker's `account`
/// (trading spec §7.2, §9.2; DEC-840 items 4 and 5), or `None`, which holds the opening: under
/// `intraday_margin`, a maintenance margin [`BrokerAccount::maintenance_excess`] refuses (a negative
/// one, DEC-524 item 2) or an excess that does not fit, never read as no requirement; and a
/// reported deficit, a negative excess, which DEC-840 item 4 refuses an opening on until §9.2's
/// `exits_only` exists. A zero excess is no deficit.
///
/// The type is the declaration's, never inferred from the multiplier, and the prior close's equity
/// is the broker's `last_equity` as it stands, under either regime (DEC-524 item 4). The
/// multiplier is not read: Alpaca's paper accounts report 4, and §7.2's 1× requirement is met by
/// the gate's own 1× cap on gross exposure (§9.3), which binds whatever the broker would lend.
#[must_use]
pub fn account_rules(declared: DeclaredRules, account: &BrokerAccount) -> Option<AccountRules> {
    let regime = match declared.regime {
        DeclaredRegime::LegacyPdt => DayTradeRegime::LegacyPdt,
        DeclaredRegime::IntradayMargin => {
            let maintenance_excess = account.maintenance_excess().ok()?;
            if maintenance_excess.is_negative() {
                return None;
            }
            DayTradeRegime::IntradayMargin { maintenance_excess }
        }
    };
    Some(AccountRules {
        account_type: declared.account_type,
        regime,
        prior_close_equity: account.last_equity,
    })
}

/// Who says whether an instrument is halted (trading spec §4.4).
pub trait HaltStatus {
    /// `Some(true)` halted, `Some(false)` trading, `None` unread.
    fn halted(&self, symbol: &str) -> Option<bool>;
}

/// No trading-status channel: every halt is unread, so every opening is held.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoStatusFeed;

impl HaltStatus for NoStatusFeed {
    fn halted(&self, _symbol: &str) -> Option<bool> {
        None
    }
}

impl HaltStatus for FixturePaper {
    fn halted(&self, symbol: &str) -> Option<bool> {
        Self::halted(self, symbol)
    }
}

/// The broker's facts read over `transport`, with the connector's declared rules and the venue's
/// halt status.
pub struct BrokerRead<T, H> {
    transport: T,
    declared: DeclaredRules,
    halts: H,
    runtime: Runtime,
}

impl<T: TradingTransport + Clone, H: HaltStatus> BrokerRead<T, H> {
    /// The facts over `transport`, under `declared`, halts from `halts`.
    ///
    /// # Errors
    /// `runtime_unavailable` when the runtime that drives the client cannot be built.
    pub fn new(transport: T, declared: DeclaredRules, halts: H) -> Result<Self, &'static str> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "runtime_unavailable")?;
        Ok(Self {
            transport,
            declared,
            halts,
            runtime,
        })
    }

    fn client(&self) -> TradingClient<T, TokioPause> {
        TradingClient::new(self.transport.clone(), TokioPause, RetryPolicy::default())
    }
}

impl<T: TradingTransport + Clone, H: HaltStatus> BrokerFacts for BrokerRead<T, H> {
    /// `GET /v2/assets/{symbol}` read now: the record of the symbol itself, or `None`.
    fn asset(&self, symbol: &str) -> Option<AssetRecord> {
        let instrument = InstrumentId::new(symbol).ok()?;
        let snapshot = self
            .runtime
            .block_on(self.client().asset(&instrument))
            .ok()?;
        let asset = snapshot.asset;
        Some(AssetRecord {
            active: asset.active,
            tradable: asset.tradable,
            fractionable: asset.fractionable,
            ipo: asset.ipo,
            ptp_no_exception: asset.ptp_no_exception,
        })
    }

    /// `GET /v2/account` read now, under the declaration ([`account_rules`]), or `None`.
    fn rules(&self) -> Option<AccountRules> {
        let outcome = self
            .runtime
            .block_on(self.client().call_one(&BrokerRequest::GetAccount))
            .ok()?;
        match outcome {
            BrokerOutcome::Account(account) => account_rules(self.declared, &account),
            _ => None,
        }
    }

    fn halted(&self, symbol: &str) -> Option<bool> {
        self.halts.halted(symbol)
    }
}

/// Alpaca's paper host's facts over `broker`'s transport and credential: Alpaca's declared rules
/// (margin, `intraday_margin`; DEC-840), and halts unread, since Alpaca serves no status channel
/// over REST.
///
/// # Errors
/// `runtime_unavailable`.
pub fn alpaca_facts<T: TradingTransport + Clone>(
    broker: &AlpacaBroker<T>,
) -> Result<BrokerRead<T, NoStatusFeed>, &'static str> {
    BrokerRead::new(broker.transport(), alpaca_account_rules(), NoStatusFeed)
}

/// The fixture's facts over `broker`'s book: the fixture answers Alpaca's API, so its rules are
/// Alpaca's declaration, and it is the venue, so it states its own halts.
///
/// # Errors
/// `runtime_unavailable`.
pub fn fixture_facts(
    broker: &AlpacaBroker<FixturePaper>,
) -> Result<BrokerRead<FixturePaper, FixturePaper>, &'static str> {
    let transport = broker.transport();
    BrokerRead::new(transport.clone(), alpaca_account_rules(), transport)
}
