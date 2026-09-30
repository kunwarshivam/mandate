//! Trading-domain spec §3.2 item 7's "USD pairs only" (E6-10, DEC-254): a crypto opening in a pair
//! not quoted in USD is denied at check 2, a USD pair passes the floor, and check 2 is whole for
//! crypto. A crypto exit in any pair and a US equity whatever its quote currency are never judged
//! by the rule.
//!
//! Until E6-10's implementation lands, check 2 is still owed for crypto and the gate refuses every
//! crypto opening its other checks would allow with `GateError::Unimplemented("evaluate",
//! "E6-10")` (DEC-129 items 29 and 34), which is where each pending test here stops. The exit, the
//! equity and the check-1 tests are not pending: they hold today and must go on holding once the
//! rule exists.

mod common;

use common::{INSTRUMENT_3, Scenario, asset, proposal, qty, usd};
use mandate_risk::{
    AssetClass, Check, CheckOutcome, Origin, Purpose, QuoteCurrency, ReasonCode, Side, Verdict,
    evaluate,
};
use proptest::prelude::*;

/// Every quote currency a snapshot can state, and the unstated one.
const QUOTES: [Option<QuoteCurrency>; 3] =
    [Some(QuoteCurrency::Usd), Some(QuoteCurrency::Other), None];

/// The two that must never admit a crypto opening: any other code, and none stated.
const NOT_USD: [Option<QuoteCurrency>; 2] = [Some(QuoteCurrency::Other), None];

/// Every origin a sell within the position can come from, with the purpose the brief's table
/// gives it, written out rather than asked of `assign_purpose`.
const EXITS: [(Origin, Purpose); 10] = [
    (Origin::OrderBuilder, Purpose::DiscretionaryExit),
    (Origin::GoalCompletion, Purpose::DiscretionaryExit),
    (Origin::RemovedInstrument, Purpose::DiscretionaryExit),
    (Origin::RiskEngine, Purpose::RiskExit),
    (Origin::TrimToTarget, Purpose::RiskExit),
    (Origin::StopWatchdog, Purpose::RiskExit),
    (Origin::AutomatedKillSwitch, Purpose::RiskExit),
    (Origin::OwnerClose, Purpose::OwnerExit),
    (Origin::OwnerKillSwitch, Purpose::OwnerExit),
    (Origin::ProtectiveLeg, Purpose::Protective),
];

/// §3.2 item 7's pair rule is reported under `not_in_working_universe` until the founder registers
/// a code of its own (DEC-254 item 3).
const PAIR_CODE: ReasonCode = ReasonCode::NotInWorkingUniverse;

/// A crypto pair that every other check passes: tradable, in the universe, 90 M of 30-day median
/// dollar volume against the 1 M floor, an active crypto account, and a continuous session.
fn crypto(quote: Option<QuoteCurrency>) -> Scenario {
    let mut s = Scenario::allowing();
    s.instrument.asset_class = AssetClass::Crypto;
    s.instrument.exchange = None;
    s.instrument.median_dollar_volume_30d = Some(usd("90000000"));
    s.instrument.quote_currency = quote;
    s
}

/// `s` holding `held` units of the instrument at 100, in the agent's sub-ledger and the account.
fn holding(mut s: Scenario, held: u32) -> Scenario {
    let units = held.to_string();
    let value = usd(&(held * 100).to_string());
    s.agent.positions.insert(asset(INSTRUMENT_3), qty(&units));
    s.agent.market_values.insert(asset(INSTRUMENT_3), value);
    s.account.positions.insert(asset(INSTRUMENT_3), qty(&units));
    s.account.market_values.insert(asset(INSTRUMENT_3), value);
    s
}

/// A buy of one unit at 100: an opening from flat, an increase from a holding of 2.
fn buying(s: Scenario, increase: bool) -> Scenario {
    let mut s = if increase { holding(s, 2) } else { s };
    s.proposed = proposal(INSTRUMENT_3, Side::Buy, "1", "100", Origin::OrderBuilder);
    s
}

/// A sell of the whole holding of 10 at 100, from `origin`.
fn selling(s: Scenario, origin: Origin) -> Scenario {
    let mut s = holding(s, 10);
    s.proposed = proposal(INSTRUMENT_3, Side::Sell, "10", "100", origin);
    s
}

/// Check 2 failed with `code` and the six checks after it were not reached, check 1 having passed.
fn denied_at_check_2(code: ReasonCode) -> Vec<CheckOutcome> {
    let mut checks = vec![
        CheckOutcome::Passed(Check::AccountAndMode),
        CheckOutcome::Failed(Check::UniverseAndLimits, code),
    ];
    checks.extend(
        [
            Check::SessionAndHalt,
            Check::OrderConstraints,
            Check::MarkAndCollar,
            Check::ConductControls,
            Check::BuyingPowerAndExposure,
            Check::DayTradeBudget,
        ]
        .map(CheckOutcome::NotReached),
    );
    checks
}

/// All eight checks passed, in §9.1's order.
fn all_passed() -> Vec<CheckOutcome> {
    [
        Check::AccountAndMode,
        Check::UniverseAndLimits,
        Check::SessionAndHalt,
        Check::OrderConstraints,
        Check::MarkAndCollar,
        Check::ConductControls,
        Check::BuyingPowerAndExposure,
        Check::DayTradeBudget,
    ]
    .map(CheckOutcome::Passed)
    .to_vec()
}

/// A crypto opening or increase in a pair quoted in anything but USD, or in no stated currency,
/// is denied at check 2, and §3.2's list order puts item 7's "USD pairs only" after items 1 to 3
/// and ahead of item 7's own 30-day volume floor.
#[test]
#[ignore = "pending E6-10"]
fn a_crypto_opening_in_a_pair_not_quoted_in_usd_is_denied_at_check_2() {
    for quote in NOT_USD {
        for (increase, purpose) in [(false, Purpose::Open), (true, Purpose::Increase)] {
            let d = evaluate(&buying(crypto(quote), increase).input()).expect("the gate decides");
            assert_eq!(
                (d.verdict, d.reason, d.purpose, d.checks),
                (
                    Verdict::Deny,
                    Some(PAIR_CODE),
                    purpose,
                    denied_at_check_2(PAIR_CODE)
                ),
                "a crypto {purpose:?} quoted in {quote:?} is not a USD pair, so §3.2 item 7 denies \
                 it at check 2 and no later check is reached"
            );
        }

        let mut illiquid = crypto(quote);
        illiquid.instrument.median_dollar_volume_30d = Some(usd("1"));
        let d = evaluate(&buying(illiquid, false).input()).expect("the gate decides");
        assert_eq!(
            (d.verdict, d.reason),
            (Verdict::Deny, Some(PAIR_CODE)),
            "item 7 names the USD pair before the 30-day volume, so a {quote:?} pair at 1 of \
             volume against the 1 M floor reports the pair, not `below_liquidity_floor`"
        );

        let mut ipo = crypto(quote);
        ipo.instrument.ipo = true;
        let d = evaluate(&buying(ipo, false).input()).expect("the gate decides");
        assert_eq!(
            (d.verdict, d.reason),
            (Verdict::Deny, Some(ReasonCode::IpoNotTradable)),
            "item 3 comes before item 7, so an `ipo` {quote:?} pair reports `ipo_not_tradable`"
        );
    }
}

/// A crypto opening or increase in a USD pair passes the floor and every later check, while the
/// pair's own 30-day volume floor still binds it.
#[test]
#[ignore = "pending E6-10"]
fn a_crypto_opening_in_a_usd_pair_passes_the_floor() {
    let usd_pair = Some(QuoteCurrency::Usd);
    for (increase, purpose) in [(false, Purpose::Open), (true, Purpose::Increase)] {
        let d = evaluate(&buying(crypto(usd_pair), increase).input()).expect("the gate decides");
        assert_eq!(
            (d.verdict, d.reason, d.purpose, d.checks.get(1).cloned()),
            (
                Verdict::Allow,
                None,
                purpose,
                Some(CheckOutcome::Passed(Check::UniverseAndLimits))
            ),
            "a USD pair meets item 7's pair rule, and at 90 M of volume against 1 M its floor too"
        );
    }

    let mut illiquid = crypto(usd_pair);
    illiquid.instrument.median_dollar_volume_30d = Some(usd("999999.99"));
    let d = evaluate(&buying(illiquid, false).input()).expect("the gate decides");
    assert_eq!(
        (d.verdict, d.reason),
        (Verdict::Deny, Some(ReasonCode::BelowLiquidityFloor)),
        "a USD pair is not a pass from the rest of item 7: 999,999.99 is below the 1 M floor"
    );
}

/// No part of check 2 is owed for crypto any more: an allowed USD-pair opening lists all eight
/// checks as passed, and a crypto exit in any pair lists check 2 as passed, exactly as a US
/// equity's does, rather than as not reached.
#[test]
#[ignore = "pending E6-10"]
fn check_2_is_whole_for_crypto() {
    let d = evaluate(&buying(crypto(Some(QuoteCurrency::Usd)), false).input())
        .expect("a crypto opening is decided, not refused as owed");
    assert_eq!(
        (d.verdict, d.checks),
        (Verdict::Allow, all_passed()),
        "a USD-pair opening passes every check and none is left not reached"
    );

    for quote in QUOTES {
        for (origin, _) in EXITS {
            let d = evaluate(&selling(crypto(quote), origin).input()).expect("the gate decides");
            assert_eq!(
                d.checks.get(1),
                Some(&CheckOutcome::Passed(Check::UniverseAndLimits)),
                "a crypto sell from {origin:?} quoted in {quote:?}: check 2 is whole, so it is \
                 listed passed, as a US equity's is"
            );
        }
    }
}

/// `AGENTS.md` rule 13 and §3.2's last paragraph: the pair rule is an eligibility item, so a
/// reduction of a crypto position is never denied by it, whatever the pair is quoted in.
#[test]
fn a_crypto_exit_in_any_pair_is_never_denied_by_the_pair_rule() {
    for quote in QUOTES {
        for (origin, purpose) in EXITS {
            let d = evaluate(&selling(crypto(quote), origin).input()).expect("the gate decides");
            assert_eq!(
                (d.verdict, d.reason, d.purpose),
                (Verdict::Allow, None, purpose),
                "a crypto sell from {origin:?} quoted in {quote:?} reduces risk and is allowed"
            );
            assert!(
                !d.checks
                    .iter()
                    .any(|c| matches!(c, CheckOutcome::Failed(..))),
                "no check fails a crypto sell from {origin:?} quoted in {quote:?}: {:?}",
                d.checks
            );
        }
    }
}

/// §3.2 item 7 is crypto's: a US equity's quote currency, stated or not, never decides an opening,
/// and item 2's exchange rule is the equity's instead.
#[test]
fn a_us_equity_is_not_judged_by_its_quote_currency() {
    for quote in QUOTES {
        for increase in [false, true] {
            let mut s = buying(Scenario::allowing(), increase);
            s.instrument.quote_currency = quote;
            let d = evaluate(&s.input()).expect("the gate decides");
            assert_eq!(
                (d.verdict, d.reason, d.checks),
                (Verdict::Allow, None, all_passed()),
                "a US-equity buy (increase: {increase}) with quote {quote:?} passes every check"
            );
        }
    }
}

/// Item 7's `crypto_status = ACTIVE` is check 1's, the account, so an inactive crypto account
/// reports `crypto_account_inactive` before check 2 looks at the pair, whatever it is quoted in.
#[test]
fn an_inactive_crypto_account_is_reported_before_the_pair() {
    for quote in QUOTES {
        let mut s = buying(crypto(quote), false);
        s.account.crypto_active = false;
        let d = evaluate(&s.input()).expect("the gate decides");
        assert_eq!(
            (d.verdict, d.reason, d.checks.first().cloned()),
            (
                Verdict::Deny,
                Some(ReasonCode::CryptoAccountInactive),
                Some(CheckOutcome::Failed(
                    Check::AccountAndMode,
                    ReasonCode::CryptoAccountInactive
                ))
            ),
            "check 1 decides first for a crypto opening quoted in {quote:?}"
        );
    }
}

/// What a generated case proposes, chosen by the generator so the oracle knows it without asking
/// the gate: a buy from flat, a buy on top of a holding, or a sell of the holding from an origin.
#[derive(Debug, Clone, Copy)]
enum Intent {
    Open,
    Increase,
    Exit(Origin),
}

fn intent() -> impl Strategy<Value = Intent> {
    prop_oneof![
        Just(Intent::Open),
        Just(Intent::Increase),
        prop::sample::select(EXITS.map(|(origin, _)| origin).to_vec()).prop_map(Intent::Exit),
    ]
}

proptest! {
    /// The pair rule denies exactly a crypto opening or increase whose quote currency is not
    /// stated as USD, at check 2 under its code, and nothing else: every other case in a scene
    /// every other check passes is allowed, with check 2 passed for an opening.
    ///
    /// The oracle is the rule's own words over the generator's choices, `opening ∧ crypto ∧
    /// quote ≠ USD`; it never reads the gate's purpose or its check list to decide what to expect.
    #[test]
    #[ignore = "pending E6-10"]
    fn the_pair_rule_denies_exactly_a_crypto_opening_not_quoted_in_usd(
        is_crypto in any::<bool>(),
        quote in prop::sample::select(QUOTES.to_vec()),
        intent in intent(),
        units in 1_u32..5,
    ) {
        let mut s = if is_crypto { crypto(quote) } else { Scenario::allowing() };
        s.instrument.quote_currency = quote;
        let buy = |s: Scenario| {
            let mut s = s;
            s.proposed = proposal(
                INSTRUMENT_3, Side::Buy, &units.to_string(), "100", Origin::OrderBuilder,
            );
            s
        };
        let s = match intent {
            Intent::Open => buy(s),
            Intent::Increase => buy(holding(s, 1)),
            Intent::Exit(origin) => {
                let mut s = holding(s, units);
                s.proposed = proposal(
                    INSTRUMENT_3, Side::Sell, &units.to_string(), "100", origin,
                );
                s
            }
        };

        let opening = matches!(intent, Intent::Open | Intent::Increase);
        let quoted_in_usd = matches!(quote, Some(QuoteCurrency::Usd));
        let denied_by_the_pair_rule = opening && is_crypto && !quoted_in_usd;

        let d = evaluate(&s.input()).expect("the gate decides");
        if denied_by_the_pair_rule {
            prop_assert_eq!(
                (d.verdict, d.reason, d.checks.get(1).cloned()),
                (
                    Verdict::Deny,
                    Some(PAIR_CODE),
                    Some(CheckOutcome::Failed(Check::UniverseAndLimits, PAIR_CODE)),
                ),
                "a crypto opening quoted in {:?} is denied at check 2", quote
            );
        } else {
            prop_assert_eq!(
                (d.verdict, d.reason),
                (Verdict::Allow, None),
                "{:?} in a {} quoted in {:?} is not the pair rule's to deny",
                intent, if is_crypto { "crypto pair" } else { "US equity" }, quote
            );
            if opening {
                prop_assert_eq!(
                    d.checks.get(1).cloned(),
                    Some(CheckOutcome::Passed(Check::UniverseAndLimits)),
                    "an allowed opening passed check 2"
                );
            }
        }
    }
}
