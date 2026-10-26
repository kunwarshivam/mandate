//! Mandate spec v0.10 §8.3 step 2's first branch (MI-34, DEC-2391): an instrument whose thesis
//! ended sells its whole position at the bid, whatever the signals and with no output read; an
//! equity inside the close window goes out marketable (DEC-70), crypto never does; nothing held
//! holds. Each case is paired with the step beside it.

mod common;

use common::{account, at, crypto_market, flat_account, quiet_risk, swing_market, two_stock_swing};
use mandate_builder::{Action, HoldReason, OrderShape, Proposal, propose_thesis_exit};
use mandate_domain::Purpose;

fn proposed(market: &mandate_builder::Market, position: &str) -> Proposal {
    propose_thesis_exit(&account(position, "99.9"), market).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn the_whole_position_sells_at_the_bid_as_a_limit_order() {
    let proposal = proposed(&swing_market(), "7");
    let Action::Sell {
        purpose,
        qty,
        limit_price,
        order_usd,
        shape,
    } = proposal.action
    else {
        panic!("a sell, not {:?}", proposal.action);
    };
    assert_eq!(purpose, Purpose::DiscretionaryExit);
    assert_eq!(qty, common::qty("7"));
    assert_eq!(limit_price, common::price("99.9"));
    assert_eq!(order_usd, common::usd("699.3"));
    assert_eq!(shape, OrderShape::Limit);
    assert!(proposal.combined.outputs_used.is_empty(), "no output read");
    assert!(proposal.clipped_by.is_empty());
}

#[test]
fn an_equity_inside_the_close_window_goes_out_marketable_and_crypto_never_does() {
    let mut closing = swing_market();
    closing.in_close_window = true;
    assert!(matches!(
        proposed(&closing, "7").action,
        Action::Sell {
            shape: OrderShape::MarketableLimit,
            ..
        }
    ));
    let mut crypto = crypto_market("60000", "60010", "0.0001");
    crypto.in_close_window = true;
    let proposal =
        propose_thesis_exit(&account("0.5", "60000"), &crypto).unwrap_or_else(|e| panic!("{e}"));
    assert!(matches!(
        proposal.action,
        Action::Sell {
            shape: OrderShape::Limit,
            ..
        }
    ));
}

#[test]
fn nothing_held_holds() {
    let proposal =
        propose_thesis_exit(&flat_account(), &swing_market()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        proposal.action,
        Action::Hold {
            reason: HoldReason::NoPosition
        }
    );
}

#[test]
fn the_signal_builder_with_the_same_facts_would_not_sell() {
    let held = account("7", "99.9");
    let proposal = mandate_builder::propose(
        &two_stock_swing(),
        &held,
        &swing_market(),
        &quiet_risk(),
        &[],
        at("2026-09-22T14:00:00Z"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        matches!(proposal.action, Action::Hold { .. }),
        "with no output the signal path holds; only the ended thesis sells"
    );
}
