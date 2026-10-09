//! Protection reads the broker's capability profile (E7-23 B2a; DEC-531 item 2, DEC-630, DEC-838;
//! trading-domain spec §5.1, §5.2, §5.4; first-live-trade brief LT-2, LT-14).
//!
//! The oracle is written here from the spec's order of strength (§5.1: OCO legs, and on a profile
//! with neither, one GTC stop-limit; §5.4: a bracket belongs to an entry) over the generated
//! table. It never calls the code under test and never reads the profile it built.

mod common;

use std::collections::BTreeMap;

use common::{fraction, price, qty};
use mandate_domain::{
    AssetClass, CapabilityProfile, Cell, Idempotency, MarketSession, OrderType as Kind,
    ProtectionForm as Form, QuantityForm as Qf, Retry, Row, TimeInForce as Tif,
};
use mandate_executor::{
    ExecutorError, OcoLegs, OrderType, ProtectionPrices, ProtectiveShape, TimeInForce,
    protective_shape,
};
use mandate_num::NumError;
use proptest::prelude::*;

const STOP: &str = "54000";
const TAKE: &str = "66000";
const OFFSET: &str = "0.005";
const OFFSET_LIMIT: &str = "53730";

type Table = BTreeMap<(Kind, Qf), (Vec<Tif>, Vec<Form>)>;

fn table(cells: &[(Kind, Qf, &[Tif], &[Form])]) -> Table {
    cells
        .iter()
        .map(|&(kind, qf, tifs, forms)| ((kind, qf), (tifs.to_vec(), forms.to_vec())))
        .collect()
}

fn profile(rows: &[(AssetClass, MarketSession, &Table)]) -> CapabilityProfile {
    let rows = rows
        .iter()
        .map(|&(asset_class, session, cells)| Row {
            asset_class,
            session,
            cells: cells
                .iter()
                .map(|(&(order_type, quantity_form), (tifs, forms))| Cell {
                    order_type,
                    quantity_form,
                    times_in_force: tifs.iter().copied().collect(),
                    protection_forms: forms.iter().copied().collect(),
                })
                .collect(),
        })
        .collect();
    let idempotency = Idempotency {
        client_order_id: true,
        retry: Retry::Idempotent,
        query_by_client_order_id: true,
    };
    CapabilityProfile::new(1, rows, idempotency).expect("a valid profile")
}

const DAY_GTC: [Tif; 2] = [Tif::Day, Tif::Gtc];

fn alpaca_equity() -> Table {
    table(&[
        (Kind::Market, Qf::Whole, &DAY_GTC, &[Form::Bracket]),
        (
            Kind::Limit,
            Qf::Whole,
            &DAY_GTC,
            &[Form::Bracket, Form::Oco],
        ),
        (Kind::StopLimit, Qf::Whole, &DAY_GTC, &[]),
        (Kind::Limit, Qf::Fractional, &[Tif::Day], &[]),
    ])
}

fn simple_stop_limit(qf: Qf) -> Table {
    table(&[(Kind::StopLimit, qf, &[Tif::Gtc], &[Form::StopLimit])])
}

fn prices(take_profit: bool) -> ProtectionPrices {
    ProtectionPrices {
        stop: price(STOP),
        take_profit: take_profit.then(|| price(TAKE)),
    }
}

fn shape(
    profile: &CapabilityProfile,
    (class, session): (AssetClass, MarketSession),
    quantity: &str,
    take_profit: bool,
    offset: bool,
) -> Option<ProtectiveShape> {
    protective_shape(
        profile,
        class,
        session,
        qty(quantity),
        prices(take_profit),
        offset.then(|| fraction(OFFSET)),
    )
    .expect("the profile read")
}

fn regular(table: &Table) -> CapabilityProfile {
    profile(&[(AssetClass::UsEquity, MarketSession::Regular, table)])
}

fn equity_shape(
    table: &Table,
    quantity: &str,
    take_profit: bool,
    offset: bool,
) -> Option<ProtectiveShape> {
    let class = (AssetClass::UsEquity, MarketSession::Regular);
    shape(&regular(table), class, quantity, take_profit, offset)
}

fn equity_form(table: &Table, quantity: &str, take_profit: bool, offset: bool) -> Option<Form> {
    equity_shape(table, quantity, take_profit, offset).map(|sent| sent.form)
}

fn oco_of(quantity: &str) -> OcoLegs {
    OcoLegs {
        take_profit: price(TAKE),
        stop: price(STOP),
        qty: qty(quantity),
    }
}

const EQUITY: (AssetClass, MarketSession) = (AssetClass::UsEquity, MarketSession::Regular);
const SESSIONS: [MarketSession; 5] = [
    MarketSession::Overnight,
    MarketSession::PreMarket,
    MarketSession::Regular,
    MarketSession::AfterHours,
    MarketSession::Crypto,
];
const CRYPTO: (AssetClass, MarketSession) = (AssetClass::Crypto, MarketSession::Crypto);

#[test]
fn an_alpaca_equity_with_a_take_profit_is_covered_by_a_gtc_oco() {
    let sent = equity_shape(&alpaca_equity(), "100", true, true)
        .expect("Alpaca's limit cell offers an OCO (§5.2, DEC-630 item 1)");
    assert_eq!(
        (sent.form, sent.order_type, sent.tif),
        (Form::Oco, OrderType::Limit, TimeInForce::Gtc)
    );
    assert_eq!(
        sent.oco,
        Some(oco_of("100")),
        "the legs are the take-profit and the stop"
    );
    assert_eq!(
        (sent.limit_price, sent.stop_price),
        (None, None),
        "legs carry prices"
    );
}

#[test]
fn an_alpaca_crypto_position_is_covered_by_one_gtc_stop_limit_below_its_stop() {
    let row = simple_stop_limit(Qf::Fractional);
    let crypto = profile(&[(AssetClass::Crypto, MarketSession::Crypto, &row)]);
    for take_profit in [false, true] {
        let sent = shape(&crypto, CRYPTO, "0.5", take_profit, true)
            .expect("one resting stop-limit (§5.4, DEC-36)");
        assert_eq!(
            (sent.form, sent.order_type, sent.tif),
            (Form::StopLimit, OrderType::StopLimit, TimeInForce::Gtc)
        );
        assert_eq!(
            sent.limit_price,
            Some(price(OFFSET_LIMIT)),
            "54000 x (1 - 0.005), DEC-539"
        );
        assert_eq!(
            (sent.stop_price, sent.oco),
            (Some(price(STOP)), None),
            "simple orders only"
        );
    }
}

#[test]
fn a_profile_with_no_oco_covers_an_equity_with_one_stop_limit_and_no_take_profit_leg() {
    let sent = equity_shape(&simple_stop_limit(Qf::Whole), "100", true, true)
        .expect("the stop-limit is the strongest form this profile offers (§5.1, §5.4)");
    assert_eq!(
        (sent.form, sent.order_type, sent.oco),
        (Form::StopLimit, OrderType::StopLimit, None)
    );
    assert_eq!(
        (sent.limit_price, sent.tif),
        (Some(price(OFFSET_LIMIT)), TimeInForce::Gtc)
    );
}

#[test]
fn an_oco_outranks_the_stop_limit_and_a_bracket_is_never_placed_on_a_position() {
    let mut both = alpaca_equity();
    both.insert(
        (Kind::StopLimit, Qf::Whole),
        (vec![Tif::Gtc], vec![Form::StopLimit]),
    );
    assert_eq!(
        equity_form(&both, "100", true, true),
        Some(Form::Oco),
        "§5.1: the OCO is stronger"
    );
    assert_eq!(
        equity_form(&both, "100", false, true),
        Some(Form::StopLimit),
        "an OCO needs a take-profit"
    );
    let bracket = table(&[(Kind::Limit, Qf::Whole, &[Tif::Gtc], &[Form::Bracket])]);
    assert_eq!(
        equity_form(&bracket, "100", true, true),
        None,
        "§5.4: a bracket's legs wait for their entry"
    );
}

#[test]
fn a_form_needs_gtc_the_right_quantity_form_and_its_own_cell() {
    let day_only = table(&[
        (Kind::Limit, Qf::Whole, &[Tif::Day], &[Form::Oco]),
        (Kind::StopLimit, Qf::Whole, &[Tif::Gtc], &[Form::StopLimit]),
    ]);
    assert_eq!(
        equity_form(&day_only, "100", true, true),
        Some(Form::StopLimit),
        "§5.1: GTC (DEC-838 item 2)"
    );
    assert_eq!(
        equity_form(&alpaca_equity(), "0.5", true, true),
        None,
        "§5.2: never fractional in an OCO"
    );
    let misplaced = table(&[(Kind::Limit, Qf::Whole, &[Tif::Gtc], &[Form::StopLimit])]);
    assert_eq!(
        equity_form(&misplaced, "100", false, true),
        None,
        "the stop-limit cell lists it (DEC-630)"
    );
}

#[test]
fn a_missing_row_offers_nothing_and_the_offset_is_the_stop_limits_alone() {
    let stop_limit = simple_stop_limit(Qf::Whole);
    let crypto = shape(&regular(&stop_limit), CRYPTO, "100", false, true);
    assert_eq!(
        crypto, None,
        "DEC-630 item 9: an unlisted cell offers nothing"
    );
    assert_eq!(
        equity_form(&stop_limit, "100", false, false),
        None,
        "no offset"
    );
    assert_eq!(
        equity_form(&alpaca_equity(), "100", true, false),
        Some(Form::Oco)
    );
}

const KINDS: [Kind; 4] = [Kind::Market, Kind::Limit, Kind::Stop, Kind::StopLimit];
const FORMS: [Form; 3] = [Form::Bracket, Form::Oco, Form::StopLimit];
const TIFS: [Tif; 3] = [Tif::Day, Tif::Gtc, Tif::Ioc];

fn specs() -> impl Strategy<Value = Vec<(Kind, Qf, u8, u8)>> {
    prop::collection::vec(
        (
            prop::sample::select(KINDS.to_vec()),
            prop::sample::select(vec![Qf::Whole, Qf::Fractional]),
            1u8..8,
            0u8..8,
        ),
        1..9,
    )
}

fn masked<T: Copy>(all: &[T; 3], mask: u8) -> Vec<T> {
    all.iter()
        .enumerate()
        .filter(|(i, _)| mask >> i & 1 == 1)
        .map(|(_, &v)| v)
        .collect()
}

fn table_of(spec: &[(Kind, Qf, u8, u8)]) -> Table {
    spec.iter()
        .map(|&(k, q, t, f)| ((k, q), (masked(&TIFS, t), masked(&FORMS, f))))
        .collect()
}

fn expected(table: &Table, quantity_form: Qf, take_profit: bool, offset: bool) -> Option<Form> {
    let strongest_first = [
        (Form::Oco, Kind::Limit, take_profit),
        (Form::StopLimit, Kind::StopLimit, offset),
    ];
    strongest_first
        .into_iter()
        .find_map(|(form, kind, usable)| {
            let (tifs, forms) = table.get(&(kind, quantity_form))?;
            (usable && forms.contains(&form) && tifs.contains(&Tif::Gtc)).then_some(form)
        })
}

proptest! {
    #[test]
    fn every_protective_order_sent_is_one_the_generated_profile_allows(
        spec in specs(),
        take_profit in any::<bool>(),
        offset in any::<bool>(),
        whole in any::<bool>(),
    ) {
        let table = table_of(&spec);
        let profile = regular(&table);
        let (quantity, quantity_form) = if whole { ("100", Qf::Whole) } else { ("0.5", Qf::Fractional) };
        let sent = shape(&profile, EQUITY, quantity, take_profit, offset);
        prop_assert_eq!(sent.as_ref().map(|s| s.form), expected(&table, quantity_form, take_profit, offset));
        if let Some(sent) = sent {
            let (kind, legs) = match sent.form {
                Form::Oco => (Kind::Limit, Some(oco_of(quantity))),
                _ => (Kind::StopLimit, None),
            };
            let (tifs, forms) = &table[&(kind, quantity_form)];
            prop_assert!(forms.contains(&sent.form) && tifs.contains(&Tif::Gtc), "the cell lists the form and gtc");
            prop_assert_eq!(sent.tif, TimeInForce::Gtc);
            prop_assert_eq!(sent.oco, legs);
            prop_assert_eq!(sent.limit_price, (kind == Kind::StopLimit).then(|| price(OFFSET_LIMIT)));
        }
    }

    #[test]
    fn the_shape_never_depends_on_the_asset_class_or_the_clocks_session(
        spec in specs(),
        take_profit in any::<bool>(),
        offset in any::<bool>(),
        whole in any::<bool>(),
    ) {
        let table = table_of(&spec);
        let profile = profile(&[
            (AssetClass::UsEquity, MarketSession::Regular, &table),
            (AssetClass::Crypto, MarketSession::Crypto, &table),
        ]);
        let quantity = if whole { "100" } else { "0.5" };
        let equity = shape(&profile, EQUITY, quantity, take_profit, offset);
        for clock in SESSIONS {
            for class in [AssetClass::UsEquity, AssetClass::Crypto] {
                let read = shape(&profile, (class, clock), quantity, take_profit, offset);
                prop_assert_eq!(&read, &equity, "LT-2: one rule table, whatever the class or the clock");
            }
        }
    }
}

#[test]
fn protection_reads_its_own_session_not_the_clocks() {
    let equity = alpaca_equity();
    let oco = profile(&[(AssetClass::UsEquity, MarketSession::Regular, &equity)]);
    let crypto = simple_stop_limit(Qf::Whole);
    let crypto = profile(&[(AssetClass::Crypto, MarketSession::Crypto, &crypto)]);
    let want_oco = shape(&oco, EQUITY, "100", true, true).expect("the regular-session OCO");
    let want_stop = shape(&crypto, CRYPTO, "100", true, true).expect("crypto's stop-limit");
    assert_eq!(
        (want_oco.form, want_stop.form),
        (Form::Oco, Form::StopLimit),
        "AGENTS rule 13: re-placement at the open or after the close still protects (DEC-838 item 4)"
    );
    for clock in SESSIONS {
        assert_eq!(
            shape(&oco, (AssetClass::UsEquity, clock), "100", true, true),
            Some(want_oco.clone())
        );
        assert_eq!(
            shape(&crypto, (AssetClass::Crypto, clock), "100", true, true),
            Some(want_stop.clone())
        );
    }
}

#[test]
fn re_place_no_longer_reads_the_asset_class_and_asks_the_profile() {
    let source = include_str!("../src/protection.rs");
    let start = source
        .find("fn re_place(")
        .expect("re_place is in protection.rs");
    let body = &source[start..];
    let body = &body[..body.find("\n}\n").expect("re_place ends")];
    let named = body.contains("AssetClass::") || body.contains("Crypto");
    assert!(!named, "LT-2 (DEC-531 item 2): the asset-class branch goes");
    let at = body
        .find("protective_shape(")
        .expect("re_place asks the profile's shape");
    let before = &body[..at];
    let statement = &before[before.rfind([';', '{', '}']).map_or(0, |i| i + 1)..];
    let after = body[at..].split(';').next().unwrap_or_default();
    assert!(!statement.contains("let _"), "the result is not discarded");
    assert!(
        statement.contains("match") || statement.contains("let") || after.ends_with(")?"),
        "the result is matched or propagated, never ignored"
    );
}

#[test]
fn an_offset_of_one_is_refused_and_never_sent_as_a_zero_limit() {
    let sent = protective_shape(
        &regular(&simple_stop_limit(Qf::Whole)),
        AssetClass::UsEquity,
        MarketSession::Regular,
        qty("100"),
        prices(false),
        Some(fraction("1")),
    );
    assert_eq!(
        sent,
        Err(ExecutorError::Num(NumError::NotPositive)),
        "DEC-539: a stop-limit's limit is stop x (1 - offset), and a sell's bound must stay positive"
    );
}
