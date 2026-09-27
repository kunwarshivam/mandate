//! The golden journal: a run whose fold output is pinned, so a change to what the fold derives
//! has to change the file and bump `FOLD_VERSION` in the same commit (ADR-0001 ES-21).
//!
//! The file is read with `mandate_canon::parse`, the same strict parser the journal uses, so a
//! golden journal that is not canonical JSON fails here rather than silently differing from what
//! a real append would have stored.

use mandate_canon::{Value, parse};
use mandate_executor::{AgentId, ExecutorState, FoldedEvent, Mode, Seq};

use super::ACCOUNT_STREAM;

/// What the file pins about the state the events fold to.
pub struct Expected {
    pub risk_clock: i64,
    pub account_head: u64,
    pub orders: usize,
    pub reservations: usize,
    pub open_unprotected_intervals: usize,
    pub mode: String,
    pub mismatched: usize,
}

impl Expected {
    /// Compares the pinned values against a folded state, one clause at a time so a failure says
    /// which derivation moved.
    pub fn assert_against(&self, state: &ExecutorState) {
        assert_eq!(
            state.risk_clock().map(mandate_executor::RiskClock::secs),
            Some(self.risk_clock),
            "the risk clock the fold ends at"
        );
        assert_eq!(
            state.head(ACCOUNT_STREAM),
            Some(Seq(self.account_head)),
            "the account stream's folded head"
        );
        assert_eq!(state.orders().len(), self.orders, "orders carried");
        assert_eq!(
            state.reservations().len(),
            self.reservations,
            "reservations held"
        );
        assert_eq!(
            state
                .unprotected_intervals()
                .iter()
                .filter(|i| i.ended_at.is_none())
                .count(),
            self.open_unprotected_intervals,
            "unprotected intervals still open"
        );
        let mode = match state.effective_mode(&AgentId(super::AGENT.to_owned())) {
            Mode::Normal => "normal",
            Mode::ExitsOnly => "exits_only",
            Mode::Paused => "paused",
            Mode::Stopped => "stopped",
        };
        assert_eq!(mode, self.mode, "the agent's effective mode");
        assert_eq!(
            state.mismatched().len(),
            self.mismatched,
            "instruments left unexplained"
        );
    }
}

pub struct Golden {
    pub fold_version: u32,
    pub events: Vec<FoldedEvent>,
    pub expected: Expected,
}

/// Reads the golden journal, failing loudly on anything the strict parser refuses.
pub fn parse_golden(source: &str) -> Golden {
    let value = parse(source.as_bytes()).unwrap_or_else(|e| panic!("golden journal: {e:?}"));
    let field = |name: &str| {
        value
            .get(name)
            .unwrap_or_else(|| panic!("golden journal has no `{name}`"))
    };
    let fold_version = u32::try_from(
        field("fold_version")
            .as_int()
            .unwrap_or_else(|| panic!("`fold_version` is an integer")),
    )
    .unwrap_or_else(|e| panic!("`fold_version`: {e}"));
    let events = field("events")
        .as_array()
        .unwrap_or_else(|| panic!("`events` is an array"))
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let seq = event
                .get("seq")
                .and_then(Value::as_int)
                .unwrap_or_else(|| panic!("event {index} has no `seq`"));
            let stream = event
                .get("stream")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("event {index} has no `stream`"))
                .to_owned();
            let event_type = event
                .get("event_type")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("event {index} has no `event_type`"))
                .to_owned();
            FoldedEvent {
                event_id: EventIdOf::of(&stream, seq),
                causation_id: event
                    .get("causation_id")
                    .and_then(Value::as_str)
                    .map(|id| mandate_executor::EventId(id.to_owned())),
                payload: event
                    .get("payload")
                    .cloned()
                    .unwrap_or_else(|| panic!("event {index} has no `payload`")),
                stream,
                seq: Seq(seq),
                event_type,
            }
        })
        .collect();
    let expected = field("expected");
    let number = |name: &str| {
        expected
            .get(name)
            .and_then(Value::as_int)
            .unwrap_or_else(|| panic!("`expected.{name}` is an integer"))
    };
    let count = |name: &str| usize::try_from(number(name)).unwrap_or(usize::MAX);
    Golden {
        fold_version,
        events,
        expected: Expected {
            risk_clock: i64::try_from(number("risk_clock")).unwrap_or(i64::MAX),
            account_head: number("account_head"),
            orders: count("orders"),
            reservations: count("reservations"),
            open_unprotected_intervals: count("open_unprotected_intervals"),
            mode: expected
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("`expected.mode` is a string"))
                .to_owned(),
            mismatched: count("mismatched"),
        },
    }
}

/// The same name the rest of the fixtures give an event, so the golden file need not repeat it.
pub struct EventIdOf;

impl EventIdOf {
    pub fn of(stream: &str, seq: u64) -> mandate_executor::EventId {
        mandate_executor::EventId(format!("{stream}-{seq}"))
    }
}
