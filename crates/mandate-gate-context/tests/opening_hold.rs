//! Why every opening of an agent's is held (DEC-2143): the one answer the binding gate's source and
//! the workspace API's agent read share, judged against an oracle that reads the rows its own way.

use mandate_canon::{Key, Value};
use mandate_gate_context::{Journaled, OpeningHold, Row};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const ACCOUNT: &str = "acct:ws1:alpaca:paper1";
const OTHER_ACCOUNT: &str = "acct:ws1:alpaca:other";
const CLOCK: &str = "clock:ws1";
const AGENT: &str = "agent-1";
const ASSET: &str = "us_equity:AAPL";
const START: i64 = 1_790_000_000;

fn object(members: &[(&str, &str)]) -> Value {
    Value::Object(
        members
            .iter()
            .map(|(key, value)| (Key::new(key).unwrap(), Value::Str((*value).to_owned())))
            .collect(),
    )
}

fn clock() -> String {
    UtcNanos::from_parts(START, 0).unwrap().to_string()
}

/// A row kind the fold reads, or one it does not, on this account's stream or another's.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Request { other_agent: bool },
    Submit,
    Fill { own: bool },
    Unrelated,
    OnOtherAccount,
}

fn row_of(kind: Kind, n: usize) -> (&'static str, &'static str, Value) {
    let clock = clock();
    let order = format!("o{n}");
    let fill_id = format!("f{n}");
    match kind {
        Kind::Request { other_agent } => (
            ACCOUNT,
            "OrderRequestRecorded",
            object(&[
                ("agent_id", if other_agent { "agent-2" } else { AGENT }),
                ("purpose", "open"),
                ("risk_clock", &clock),
            ]),
        ),
        Kind::Submit => (
            ACCOUNT,
            "OrderSubmitted",
            object(&[
                ("client_order_id", &order),
                ("instrument_id", ASSET),
                ("side", "buy"),
            ]),
        ),
        Kind::Fill { own } => (
            ACCOUNT,
            "FillApplied",
            object(&[
                ("fill_id", &fill_id),
                ("client_order_id", if own { &order } else { "external" }),
                ("instrument_id", ASSET),
                ("side", "buy"),
                ("qty_gross", "1"),
                ("price", "190"),
                ("risk_clock", &clock),
            ]),
        ),
        Kind::Unrelated => (ACCOUNT, "AgentNoted", object(&[("note", "x")])),
        Kind::OnOtherAccount => (
            OTHER_ACCOUNT,
            "OrderRequestRecorded",
            object(&[
                ("agent_id", AGENT),
                ("purpose", "open"),
                ("risk_clock", &clock),
            ]),
        ),
    }
}

fn folded(kinds: &[Kind]) -> Journaled {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    for (n, kind) in kinds.iter().enumerate() {
        let (stream, event_type, payload) = row_of(*kind, n);
        state
            .fold(Row {
                stream,
                event_type,
                payload: &payload,
            })
            .expect("every generated row is readable");
    }
    state
}

/// The oracle: the account has traded once its own stream holds an order request, a submission,
/// or a fill, of any agent; rows of other kinds or on another account's stream change nothing.
fn traded(kinds: &[Kind]) -> bool {
    kinds.iter().any(|kind| {
        matches!(
            kind,
            Kind::Request { .. } | Kind::Submit | Kind::Fill { .. }
        )
    })
}

fn kind() -> impl Strategy<Value = Kind> {
    prop_oneof![
        any::<bool>().prop_map(|other_agent| Kind::Request { other_agent }),
        Just(Kind::Submit),
        any::<bool>().prop_map(|own| Kind::Fill { own }),
        Just(Kind::Unrelated),
        Just(Kind::OnOtherAccount),
    ]
}

#[test]
fn an_untraded_account_holds_no_opening_pinned_or_not() {
    let state = folded(&[Kind::Unrelated, Kind::OnOtherAccount]);
    assert_eq!(state.opening_hold(true), None);
    assert_eq!(state.opening_hold(false), None);
}

#[test]
fn once_the_account_traded_a_pinned_agent_waits_on_the_account_risk_view_and_a_research_agent_on_a_multi_instrument_one()
 {
    for first in [
        Kind::Request { other_agent: true },
        Kind::Submit,
        Kind::Fill { own: false },
    ] {
        let state = folded(&[first]);
        assert_eq!(
            state.opening_hold(true),
            Some(OpeningHold::AccountTraded),
            "{first:?}"
        );
        assert_eq!(
            state.opening_hold(false),
            Some(OpeningHold::ResearchAgentTraded),
            "{first:?}"
        );
    }
}

#[test]
fn each_reason_has_the_code_the_agent_read_serves() {
    assert_eq!(OpeningHold::AccountTraded.code(), "account_traded");
    assert_eq!(
        OpeningHold::ResearchAgentTraded.code(),
        "research_agent_traded"
    );
}

proptest! {
    /// Over any sequence of rows, the hold is the gate's own `untraded` test, and agrees with an
    /// oracle that reads the rows by kind alone; whether the mandate is pinned picks only the reason.
    #[test]
    fn the_hold_is_exactly_the_account_having_traded(kinds in proptest::collection::vec(kind(), 0..12), pinned in any::<bool>()) {
        let state = folded(&kinds);
        let hold = state.opening_hold(pinned);
        prop_assert_eq!(hold.is_some(), !state.untraded());
        prop_assert_eq!(hold.is_some(), traded(&kinds));
        let reason = if pinned { OpeningHold::AccountTraded } else { OpeningHold::ResearchAgentTraded };
        prop_assert!(hold.is_none() || hold == Some(reason));
    }
}
