//! Corporate actions as the run's configuration names them (DEC-2245 items 1 and 4): each record
//! is spelt in the digested configuration exactly as written out here, so two runs whose actions
//! differ never share a configuration digest.

mod common;

use common::*;
use mandate_canon::Value;
use mandate_num::{Price, SplitRatio};
use mandate_version_backtest::{RecordedAction, VersionInput, VersionObjects};

const DAYS: usize = 30;

/// The configuration's `corporate_actions` member for a run holding `actions`.
fn spelt(actions: &[RecordedAction]) -> Value {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days, "0.5");
    let bars = flat_bars(DAYS);
    let (mandate, settings, instrument) = (pinned(), settings(), xyz());
    let medians = Median(qty("10000000"));
    let objects = VersionObjects::of(&VersionInput {
        mandate: &mandate,
        settings: &settings,
        instrument: &instrument,
        bars: &bars,
        coverage_start: at("2026-09-01T00:00:00Z"),
        first_bar_volumes: &medians,
        outputs: &outputs,
        theses: &[],
        corporate_actions: actions,
    })
    .unwrap_or_else(|e| panic!("the objects: {e:?}"));
    objects
        .config
        .get("corporate_actions")
        .cloned()
        .unwrap_or_else(|| panic!("the member"))
}

fn canonical(json: &str) -> Value {
    mandate_canon::parse(json.as_bytes()).unwrap_or_else(|e| panic!("{e}"))
}

fn price(text: &str) -> Price {
    Price::parse(text).unwrap_or_else(|e| panic!("{e}"))
}

/// A split with and without a cash-in-lieu price, and a cash dividend, each spelt with every
/// member, in the records' order.
#[test]
#[ignore = "pending E4-4"]
fn each_action_is_spelt_with_every_member() {
    let actions = [
        RecordedAction::Split {
            ex_date: day("2026-08-14"),
            ratio: SplitRatio::new(1, 10).unwrap_or_else(|e| panic!("{e}")),
            cash_in_lieu_price: Some(price("40.5")),
        },
        split_on(day("2026-08-17")),
        RecordedAction::CashDividend {
            ex_date: day("2026-08-18"),
            pay_date: day("2026-08-28"),
            per_share: price("0.25"),
        },
    ];
    let expected = canonical(
        r#"[{"cash_in_lieu_price":"40.5","ex_date":"2026-08-14","kind":"split","ratio":{"new":1,"old":10}},{"cash_in_lieu_price":null,"ex_date":"2026-08-17","kind":"split","ratio":{"new":2,"old":1}},{"ex_date":"2026-08-18","kind":"cash_dividend","pay_date":"2026-08-28","per_share":"0.25"}]"#,
    );
    assert_eq!(spelt(&actions), expected);
}

/// No actions spell an empty list, so a run without any keeps the configuration digest it had
/// when the member held dates.
#[test]
fn no_actions_spell_an_empty_list() {
    assert_eq!(spelt(&[]), Value::Array(Vec::new()));
}
