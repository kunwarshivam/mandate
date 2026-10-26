//! E21-30 X1b-3 G2b-2 (DEC-2380 item 2): the broker's facts the binding gate reads, over the
//! fixture broker and over Alpaca's paper host's recorded answers. Each fact is read when asked,
//! each one that cannot be read is `None`, which holds the opening (`AGENTS.md` rule 3), and the
//! account's rules are the connector's declaration over the broker's figures (trading spec §7.2,
//! §9.2; DEC-840). A property test checks [`account_rules`] against figures in whole cents.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use mandate_accounting::{AccountType, InstrumentId};
use mandate_alpaca::{
    AccountRules as DeclaredRules, DataClient, DeclaredRegime, HttpRequest, Response,
    TradingTransport, TransportError, alpaca_account_rules,
};
use mandate_executor::BrokerAccount;
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::credential::{AlpacaQuotes, paper_quote_bounds};
use mandate_executord::deployment::BrokerFacts;
use mandate_executord::facts::{
    BrokerRead, NoStatusFeed, account_rules, alpaca_facts, fixture_facts,
};
use mandate_executord::gate::{AccountRules, AssetRecord};
use mandate_executord::paper::{ASSETS, FixturePaper};
use mandate_executord::quotes::QuoteFeed;
use mandate_num::{Price, Usd};
use mandate_risk::DayTradeRegime;
use mandate_time::UtcNanos;
use proptest::prelude::*;

type Error = Box<dyn std::error::Error>;

fn at(text: &str) -> Result<UtcNanos, Error> {
    Ok(UtcNanos::parse_rfc3339(text)?)
}

fn usd(text: &str) -> Result<Usd, Error> {
    Ok(Usd::parse(text)?)
}

const NOON: &str = "2026-10-26T16:00:00Z";

/// Every one of the fixture's assets is active, tradable and fractionable, neither an IPO nor a
/// PTP, and not halted; any other symbol is unread, so an opening in it is held.
#[test]
fn the_fixture_states_each_of_its_assets_and_nothing_else() -> Result<(), Error> {
    let broker = AlpacaBroker::new(FixturePaper::filling(at(NOON)?))?;
    let facts = fixture_facts(&broker)?;
    let listed = AssetRecord {
        active: true,
        tradable: true,
        fractionable: true,
        ipo: false,
        ptp_no_exception: false,
    };
    for asset in ASSETS {
        assert_eq!(facts.asset(asset.symbol), Some(listed), "{}", asset.symbol);
        assert_eq!(facts.halted(asset.symbol), Some(false), "{}", asset.symbol);
    }
    assert_eq!(ASSETS.map(|a| a.symbol), ["SPY", "QQQ"]);
    for unlisted in ["AAPL", "spy", "BTC/USD", ""] {
        assert_eq!(facts.asset(unlisted), None, "{unlisted:?}");
        assert_eq!(facts.halted(unlisted), None, "{unlisted:?}");
    }
    Ok(())
}

/// The fixture's account under Alpaca's declaration: a margin account under `intraday_margin`
/// with its whole equity as excess (no maintenance requirement), and its prior close the
/// fixture's opening cash.
#[test]
fn the_fixtures_rules_are_alpacas_declaration_over_its_account() -> Result<(), Error> {
    let broker = AlpacaBroker::new(FixturePaper::filling(at(NOON)?))?;
    let facts = fixture_facts(&broker)?;
    assert_eq!(
        facts.rules(),
        Some(AccountRules {
            account_type: AccountType::Margin,
            regime: DayTradeRegime::IntradayMargin {
                maintenance_excess: usd("100000")?,
            },
            prior_close_equity: usd("100000")?,
        })
    );
    Ok(())
}

/// The fixture quotes each of its assets at its fixed bid and ask, stamped by the reader's clock,
/// so the feed hands a sane quote observed now; a fixture that does not quote, and any other
/// symbol, hands none.
#[test]
fn the_fixture_quotes_its_assets_on_the_readers_clock() -> Result<(), Error> {
    let now = at(NOON)?;
    let quoting = FixturePaper::new(now).quoting(move || now);
    let feed = AlpacaQuotes::new(DataClient::new(quoting, Fixed(now)), paper_quote_bounds()?)?;
    for asset in ASSETS {
        let quote = feed.observe(&InstrumentId::new(asset.symbol)?, now)?;
        assert_eq!(
            quote.bid,
            Some(Price::parse(asset.bid)?),
            "{}",
            asset.symbol
        );
        assert_eq!(
            quote.ask,
            Some(Price::parse(asset.ask)?),
            "{}",
            asset.symbol
        );
        assert!(quote.sane, "{}", asset.symbol);
        assert_eq!(quote.observed_at.secs(), now.secs(), "{}", asset.symbol);
    }
    assert!(feed.observe(&InstrumentId::new("AAPL")?, now).is_err());
    let silent = AlpacaQuotes::new(
        DataClient::new(FixturePaper::new(now), Fixed(now)),
        paper_quote_bounds()?,
    )?;
    assert!(silent.observe(&InstrumentId::new("SPY")?, now).is_err());
    Ok(())
}

/// A quote stamped by a clock behind the reader's by more than the profile's age is stale, and
/// handed nowhere: the fixture's freshness is the reader's clock's, never assumed.
#[test]
fn a_fixture_quote_older_than_the_profile_allows_is_refused() -> Result<(), Error> {
    let now = at(NOON)?;
    let behind = at("2026-10-26T15:59:49Z")?;
    let feed = AlpacaQuotes::new(
        DataClient::new(FixturePaper::new(now).quoting(move || behind), Fixed(now)),
        paper_quote_bounds()?,
    )?;
    assert!(feed.observe(&InstrumentId::new("SPY")?, now).is_err());
    Ok(())
}

/// A clock that always reads one instant, for the data client.
#[derive(Clone, Copy)]
struct Fixed(UtcNanos);

impl mandate_alpaca::Pause for Fixed {
    async fn pause(&self, _for: Duration) {}

    fn now(&self) -> UtcNanos {
        self.0
    }
}

/// One path's answer: its status and body.
type Answer = (String, u16, Vec<u8>);

/// A transport that answers each path from a fixed table, the bodies Alpaca's paper host was
/// recorded sending or the contract's forms, and counts each request, so a test can see each fact
/// read when it is asked.
#[derive(Clone, Default)]
struct Recorded {
    answers: Rc<RefCell<Vec<Answer>>>,
    asked: Rc<RefCell<Vec<String>>>,
}

impl Recorded {
    fn answer(self, path: &str, status: u16, body: &[u8]) -> Self {
        self.answers
            .borrow_mut()
            .push((path.to_owned(), status, body.to_vec()));
        self
    }

    fn asked(&self) -> Vec<String> {
        self.asked.borrow().clone()
    }
}

impl TradingTransport for Recorded {
    async fn send(&self, request: &HttpRequest) -> Result<Response, TransportError> {
        let path = request.path_and_query().to_owned();
        self.asked.borrow_mut().push(path.clone());
        let answer = self
            .answers
            .borrow()
            .iter()
            .find(|(wanted, _, _)| *wanted == path)
            .cloned();
        match answer {
            Some((_, status, body)) => Ok(Response { status, body }),
            None => Err(TransportError::Connect),
        }
    }
}

/// One recorded body from `mandate-alpaca`'s fixtures.
fn recorded(exchange: &str) -> Result<Vec<u8>, Error> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mandate-alpaca/tests/fixtures/alpaca-trading")
        .join(exchange)
        .join("response-1.json");
    Ok(std::fs::read(path)?)
}

fn over(transport: Recorded) -> Result<BrokerRead<Recorded, NoStatusFeed>, Error> {
    Ok(BrokerRead::new(
        transport,
        alpaca_account_rules(),
        NoStatusFeed,
    )?)
}

/// Alpaca's recorded AAPL record reads as active, tradable and fractionable, with neither the
/// `ipo` nor the `ptp_no_exception` attribute; the recorded `404` for an unknown symbol is unread.
#[test]
fn alpacas_recorded_asset_record_reads_and_its_absence_is_unread() -> Result<(), Error> {
    let transport = Recorded::default()
        .answer("/v2/assets/AAPL", 200, &recorded("asset_equity")?)
        .answer("/v2/assets/XYZQ", 404, &recorded("asset_absent")?);
    let facts = over(transport.clone())?;
    assert_eq!(
        facts.asset("AAPL"),
        Some(AssetRecord {
            active: true,
            tradable: true,
            fractionable: true,
            ipo: false,
            ptp_no_exception: false,
        })
    );
    assert_eq!(facts.asset("XYZQ"), None);
    assert_eq!(transport.asked(), ["/v2/assets/AAPL", "/v2/assets/XYZQ"]);
    Ok(())
}

/// Each eligibility fact is the record's own: an `ipo` or `ptp_no_exception` attribute, and an
/// inactive, untradable or unfractionable record, each read as such, active and tradable apart;
/// and a record of another symbol, one with no `attributes`, a server error, or no answer at all
/// are unread.
#[test]
fn each_asset_fact_is_the_records_own_and_a_bad_record_is_unread() -> Result<(), Error> {
    let body = |symbol: &str,
                status: &str,
                tradable: bool,
                fractionable: bool,
                attributes: &str| {
        format!(
            r#"{{"attributes":{attributes},"class":"us_equity","exchange":"NYSE","fractionable":{fractionable},"id":"b0b6dd9d-8b9b-48a9-ba46-b9d54906e415","status":"{status}","symbol":"{symbol}","tradable":{tradable}}}"#
        )
        .into_bytes()
    };
    let transport = Recorded::default()
        .answer(
            "/v2/assets/IPOX",
            200,
            &body("IPOX", "active", true, true, r#"["ipo"]"#),
        )
        .answer(
            "/v2/assets/PTPX",
            200,
            &body("PTPX", "active", true, false, r#"["ptp_no_exception"]"#),
        )
        .answer(
            "/v2/assets/GONE",
            200,
            &body("GONE", "inactive", false, true, "[]"),
        )
        .answer(
            "/v2/assets/HALT",
            200,
            &body("HALT", "active", false, true, "[]"),
        )
        .answer(
            "/v2/assets/IDLE",
            200,
            &body("IDLE", "inactive", true, false, "[]"),
        )
        .answer(
            "/v2/assets/ELSE",
            200,
            &body("ZZZZ", "active", true, true, "[]"),
        )
        .answer(
            "/v2/assets/BARE",
            200,
            &body("BARE", "active", true, true, "null"),
        )
        .answer("/v2/assets/DOWN", 503, b"{}");
    let facts = over(transport)?;
    let record = |active, tradable, fractionable, ipo, ptp_no_exception| AssetRecord {
        active,
        tradable,
        fractionable,
        ipo,
        ptp_no_exception,
    };
    assert_eq!(
        facts.asset("IPOX"),
        Some(record(true, true, true, true, false))
    );
    assert_eq!(
        facts.asset("PTPX"),
        Some(record(true, true, false, false, true))
    );
    assert_eq!(
        facts.asset("GONE"),
        Some(record(false, false, true, false, false))
    );
    assert_eq!(
        facts.asset("HALT"),
        Some(record(true, false, true, false, false))
    );
    assert_eq!(
        facts.asset("IDLE"),
        Some(record(false, true, false, false, false))
    );
    for unread in ["ELSE", "BARE", "DOWN", "NONE"] {
        assert_eq!(facts.asset(unread), None, "{unread}");
    }
    Ok(())
}

/// Alpaca's recorded paper account, `multiplier` 4 and no maintenance requirement, reads under
/// Alpaca's declaration as a margin account under `intraday_margin` with its whole equity as
/// excess, its prior close `last_equity`; each read is made when asked; and Alpaca's halts are
/// unread, so its openings hold on them.
#[test]
fn alpacas_recorded_account_reads_under_its_declaration_and_its_halts_are_unread()
-> Result<(), Error> {
    let transport = Recorded::default().answer("/v2/account", 200, &recorded("account_active")?);
    let facts = over(transport.clone())?;
    let rules = Some(AccountRules {
        account_type: AccountType::Margin,
        regime: DayTradeRegime::IntradayMargin {
            maintenance_excess: usd("1000000")?,
        },
        prior_close_equity: usd("1000000")?,
    });
    assert_eq!(facts.rules(), rules);
    assert_eq!(facts.rules(), rules);
    assert_eq!(transport.asked(), ["/v2/account", "/v2/account"]);
    assert_eq!(facts.halted("AAPL"), None);
    let broker = AlpacaBroker::new(transport)?;
    assert_eq!(alpaca_facts(&broker)?.halted("SPY"), None);
    Ok(())
}

/// An account that does not read, a server error, and no answer are unread rules.
#[test]
fn an_account_that_cannot_be_read_gives_no_rules() -> Result<(), Error> {
    let mut missing = String::from_utf8(recorded("account_active")?)?;
    missing = missing.replace(r#""last_equity":"1000000","#, "");
    for transport in [
        Recorded::default().answer("/v2/account", 200, missing.as_bytes()),
        Recorded::default().answer("/v2/account", 500, b"{}"),
        Recorded::default().answer("/v2/account", 200, b"not json"),
        Recorded::default(),
    ] {
        assert_eq!(over(transport)?.rules(), None);
    }
    Ok(())
}

/// The account the property test reads, its figures in whole cents.
fn account(
    equity: i64,
    margin: i64,
    last_equity: i64,
    multiplier: u32,
) -> Result<BrokerAccount, Error> {
    let cents = |c: i64| usd(&cents_text(c));
    Ok(BrokerAccount {
        status: "ACTIVE".to_owned(),
        crypto_status: "ACTIVE".to_owned(),
        trading_blocked: false,
        account_blocked: false,
        trade_suspended_by_user: false,
        multiplier,
        equity: cents(equity)?,
        cash: cents(equity)?,
        buying_power: cents(equity)?,
        non_marginable_buying_power: cents(equity)?,
        accrued_fees: Usd::ZERO,
        last_equity: cents(last_equity)?,
        maintenance_margin: cents(margin)?,
    })
}

/// Whole cents as canonical decimal text: no trailing fractional zero, zero unsigned.
fn cents_text(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    match abs % 100 {
        0 => format!("{sign}{}", abs / 100),
        tenths if tenths % 10 == 0 => format!("{sign}{}.{}", abs / 100, tenths / 10),
        hundredths => format!("{sign}{}.{hundredths:02}", abs / 100),
    }
}

proptest! {
    /// [`account_rules`] against an oracle in whole cents: the type is always the declaration's
    /// and the prior close `last_equity`, whatever the multiplier; under `legacy_pdt` nothing else
    /// is read; under `intraday_margin` the excess is equity less the margin, a negative margin or
    /// a deficit gives no rules, and a zero excess is no deficit.
    #[test]
    fn the_rules_are_the_declaration_over_the_figures(
        equity in -1_000_000_000i64..1_000_000_000,
        margin in -1_000_000i64..1_000_000_000,
        last_equity in -1_000_000_000i64..1_000_000_000,
        multiplier in prop_oneof![Just(1u32), Just(2u32), Just(4u32)],
        cash in any::<bool>(),
        legacy in any::<bool>(),
    ) {
        let declared = DeclaredRules {
            account_type: if cash { AccountType::Cash } else { AccountType::Margin },
            regime: if legacy { DeclaredRegime::LegacyPdt } else { DeclaredRegime::IntradayMargin },
        };
        let got = account_rules(declared, &account(equity, margin, last_equity, multiplier).unwrap());
        let excess = equity - margin;
        let regime = if legacy {
            Some(DayTradeRegime::LegacyPdt)
        } else if margin < 0 || excess < 0 {
            None
        } else {
            Some(DayTradeRegime::IntradayMargin { maintenance_excess: usd(&cents_text(excess)).unwrap() })
        };
        let want = regime.map(|regime| AccountRules {
            account_type: declared.account_type,
            regime,
            prior_close_equity: usd(&cents_text(last_equity)).unwrap(),
        });
        prop_assert_eq!(got, want);
    }
}

/// The boundary cases by name: an excess of exactly zero is no deficit, one cent of deficit holds,
/// and a negative margin holds even with equity to spare.
#[test]
fn a_zero_excess_is_no_deficit_and_a_cent_of_deficit_holds() -> Result<(), Error> {
    let declared = alpaca_account_rules();
    let zero = account_rules(declared, &account(500_000, 500_000, 400_000, 1)?);
    assert_eq!(
        zero.map(|r| r.regime),
        Some(DayTradeRegime::IntradayMargin {
            maintenance_excess: Usd::ZERO
        })
    );
    assert_eq!(
        account_rules(declared, &account(500_000, 500_001, 400_000, 1)?),
        None
    );
    assert_eq!(
        account_rules(declared, &account(500_000, -1, 400_000, 1)?),
        None
    );
    Ok(())
}
