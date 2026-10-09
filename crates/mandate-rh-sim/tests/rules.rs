//! The place and review rules of the published contract, one example each (E7-25;
//! robinhood-contract.md, "The equity order tools").

mod common;

use common::{AGENTIC, DAY_TRADER, NOT_AGENTIC, account, limit, price, qty, sim};
use mandate_rh_sim::SimError::{QuantityForm, SessionNeedsLimit, Unreadable};
use mandate_rh_sim::{Alert, Event, OrderRequest, Session, Sim, SimError};

type Outcome = Result<(), SimError>;

/// A two-share limit buy at 501 with these fields replaced; an empty value removes the field.
fn req(edits: &[(&str, &str)]) -> OrderRequest {
    let mut r = limit("buy", "2", "501", 1);
    for &(field, value) in edits {
        let v = (!value.is_empty()).then(|| value.to_owned());
        match field {
            "account" => r.account_number = value.to_owned(),
            "symbol" => r.symbol = value.to_owned(),
            "side" => r.side = value.to_owned(),
            "type" => r.order_type = value.to_owned(),
            "quantity" => r.quantity = v,
            "dollar_amount" => r.dollar_amount = v,
            "limit_price" => r.limit_price = v,
            "stop_price" => r.stop_price = v,
            "time_in_force" => r.time_in_force = v,
            "market_hours" => r.market_hours = v,
            _ => r.ref_id = v,
        }
    }
    r
}

const MARKET: [(&str, &str); 2] = [("type", "market"), ("limit_price", "")];
const STOP_LIMIT: [(&str, &str); 2] = [("type", "stop_limit"), ("stop_price", "499")];

#[test]
fn quantity_forms_sessions_and_text_follow_the_contract() -> Outcome {
    let cases: Vec<(OrderRequest, Result<(), SimError>)> = vec![
        (req(&[]), Ok(())),
        (req(&[("quantity", "1.5")]), Err(QuantityForm)),
        (req(&[MARKET[0], MARKET[1], ("quantity", "1.5")]), Ok(())),
        (
            req(&[MARKET[0], MARKET[1], ("market_hours", "extended_hours")]),
            Err(SessionNeedsLimit),
        ),
        (
            req(&[
                STOP_LIMIT[0],
                STOP_LIMIT[1],
                ("market_hours", "all_day_hours"),
            ]),
            Err(SessionNeedsLimit),
        ),
        (
            req(&[STOP_LIMIT[0], STOP_LIMIT[1], ("time_in_force", "gtc")]),
            Ok(()),
        ),
        (req(&[("market_hours", "extended_hours")]), Ok(())),
        (
            req(&[("market_hours", "all_day_hours"), ("time_in_force", "gtc")]),
            Ok(()),
        ),
        (
            req(&[("quantity", ""), ("dollar_amount", "100")]),
            Err(QuantityForm),
        ),
        (
            req(&[MARKET[0], MARKET[1], ("dollar_amount", "100")]),
            Err(Unreadable("quantity")),
        ),
        (req(&[("quantity", "")]), Err(Unreadable("quantity"))),
        (req(&[("quantity", "0")]), Err(Unreadable("quantity"))),
        (req(&[("quantity", "2.0")]), Err(Unreadable("quantity"))),
        (req(&[("limit_price", "")]), Err(Unreadable("limit_price"))),
        (
            req(&[("limit_price", "501.10")]),
            Err(Unreadable("limit_price")),
        ),
        (
            req(&[("type", "stop_limit")]),
            Err(Unreadable("stop_price")),
        ),
        (req(&[("type", "trailing_stop")]), Err(Unreadable("type"))),
        (req(&[("side", "short")]), Err(Unreadable("side"))),
        (
            req(&[("time_in_force", "ioc")]),
            Err(Unreadable("time_in_force")),
        ),
        (
            req(&[("market_hours", "overnight")]),
            Err(Unreadable("market_hours")),
        ),
        (req(&[("ref_id", "retry-1")]), Err(Unreadable("ref_id"))),
        (
            req(&[("quantity", "1.5"), ("market_hours", "extended_hours")]),
            Err(QuantityForm),
        ),
        (
            req(&[STOP_LIMIT[0], STOP_LIMIT[1], ("quantity", "1.5")]),
            Err(QuantityForm),
        ),
        (
            req(&[
                ("type", "stop_market"),
                ("limit_price", ""),
                ("stop_price", "499"),
                ("quantity", "1.5"),
            ]),
            Err(QuantityForm),
        ),
        (
            req(&[
                ("type", "stop_market"),
                ("limit_price", ""),
                ("stop_price", "499"),
                ("quantity", ""),
                ("dollar_amount", "100"),
            ]),
            Err(QuantityForm),
        ),
        (
            req(&[("ref_id", "zzzzzzzz-0000-4000-8000-000000000001")]),
            Err(Unreadable("ref_id")),
        ),
        (
            req(&[
                ("type", "stop_market"),
                ("limit_price", ""),
                ("stop_price", "499"),
            ]),
            Ok(()),
        ),
        (req(&[("symbol", "QQQ")]), Ok(())),
    ];
    for (i, (request, expected)) in cases.into_iter().enumerate() {
        assert_eq!(
            sim()?.place(&request).map(|_| ()),
            expected,
            "case {i}: {request:?}"
        );
    }
    let dollars = req(&[
        MARKET[0],
        MARKET[1],
        ("quantity", ""),
        ("dollar_amount", "100"),
    ]);
    assert_eq!(
        sim()?.place(&dollars)?.quantity,
        qty("0.2"),
        "100 USD at the 500 quote"
    );
    let unquoted = req(&[MARKET[0], MARKET[1], ("symbol", "QQQ")]);
    assert_eq!(sim()?.place(&unquoted), Err(SimError::NoQuote));
    Ok(())
}

#[test]
fn only_an_agentic_account_reviews_or_places() -> Outcome {
    let mut sim = sim()?;
    assert_eq!(
        sim.review(&req(&[("account", NOT_AGENTIC)])),
        Err(SimError::NotAgentic)
    );
    assert_eq!(
        sim.place(&req(&[("account", NOT_AGENTIC)])),
        Err(SimError::NotAgentic)
    );
    assert_eq!(
        sim.place(&req(&[("account", "5QR09999")])),
        Err(SimError::UnknownAccount)
    );
    assert_eq!(sim.orders(NOT_AGENTIC)?, vec![]);
    assert_eq!(sim.orders("5QR09999"), Err(SimError::UnknownAccount));
    let twice = vec![
        account(AGENTIC, true, false),
        account(AGENTIC, false, false),
    ];
    assert_eq!(Sim::new(twice).map(|_| ()), Err(SimError::DuplicateAccount));
    Ok(())
}

#[test]
fn review_raises_each_alert_and_place_refuses_it() -> Outcome {
    let mut sim = sim()?;
    let too_big = req(&[("quantity", "20")]);
    assert_eq!(sim.review(&too_big)?.alerts, vec![Alert::BuyingPower]);
    assert_eq!(
        sim.place(&too_big),
        Err(SimError::Alert(Alert::BuyingPower))
    );
    let market = req(&[MARKET[0], MARKET[1], ("quantity", "21")]);
    assert_eq!(
        sim.review(&market)?.alerts,
        vec![Alert::BuyingPower],
        "21 at the 500 quote is over 10,000"
    );
    let exact = req(&[MARKET[0], MARKET[1], ("quantity", "20")]);
    assert_eq!(
        sim.review(&exact)?.alerts,
        vec![],
        "exactly the buying power"
    );
    assert_eq!(sim.review(&req(&[("quantity", "19")]))?.alerts, vec![]);
    let day = |side: &str| OrderRequest {
        account_number: DAY_TRADER.to_owned(),
        ..limit(side, "1", "500", if side == "buy" { 2 } else { 3 })
    };
    let bought = sim.place(&day("buy"))?;
    sim.fill(&bought.id, qty("1"), price("500"))?;
    assert_eq!(
        sim.review(&day("sell"))?.alerts,
        vec![Alert::PatternDayTrading]
    );
    assert_eq!(
        sim.place(&day("sell")),
        Err(SimError::Alert(Alert::PatternDayTrading))
    );
    sim.apply(Event::EndOfDay)?;
    sim.apply(Event::Session(Session::Regular))?;
    assert_eq!(
        sim.review(&day("sell"))?.alerts,
        vec![],
        "a new day is no day trade"
    );
    sim.apply(Event::Halt("SPY".to_owned()))?;
    let review = sim.review(&req(&[]))?;
    assert_eq!(
        (review.quote, review.alerts),
        (Some(price("500")), vec![Alert::Halt])
    );
    assert_eq!(sim.place(&req(&[])), Err(SimError::Alert(Alert::Halt)));
    assert_eq!(
        sim.orders(AGENTIC)?,
        vec![],
        "nothing an alert refused was placed"
    );
    Ok(())
}

#[test]
fn each_refusal_names_what_it_refuses() {
    let field = SimError::Unreadable("ref_id").to_string();
    assert!(field.starts_with("`ref_id` is missing"), "{field}");
    let alert = SimError::Alert(mandate_rh_sim::Alert::Halt).to_string();
    assert_eq!(alert, "refused by a pre-trade alert: Halt");
}
