//! The facts are a function of the journaled rows alone (DEC-2378 item 2): random sequences of
//! account and scheduler ticks, each stream's clock running on its own, marks, snapshots and
//! mandate records, checked against an oracle that finds each fact by
//! searching the rows from the newest back, rather than folding them forward.

use std::collections::BTreeMap;

use mandate_canon::{Key, Value};
use mandate_gate_context::{Freshness, Held, Journaled, Row};
use mandate_num::Price;
use mandate_time::UtcNanos;
use proptest::prelude::*;

const ACCOUNT: &str = "acct:ws1:alpaca:paper1";
const CLOCK: &str = "clock:ws1";
const AGENT: &str = "agent-1";
const ASSET: &str = "us_equity:AAPL";
const START: i64 = 1_790_000_000;
const FRESH: Freshness = Freshness {
    mark_max_age_s: 8,
    snapshot_max_age_s: 20,
};

#[derive(Debug, Clone)]
enum Step {
    Tick,
    SchedulerTick,
    Mark {
        bid: u32,
        sane: bool,
        session: usize,
    },
    Snapshot,
    Deployed(String),
    Applied {
        version: String,
        applied: bool,
    },
    Stopped,
}

const SESSIONS: [&str; 5] = [
    "regular",
    "crypto",
    "pre_market",
    "after_hours",
    "overnight",
];

fn step() -> impl Strategy<Value = (u8, Step)> {
    let kind = prop_oneof![
        Just(Step::Tick),
        Just(Step::SchedulerTick),
        (1u32..500, any::<bool>(), 0usize..5).prop_map(|(bid, sane, session)| Step::Mark {
            bid,
            sane,
            session
        }),
        Just(Step::Snapshot),
        "v[0-9]".prop_map(Step::Deployed),
        ("v[0-9]", any::<bool>()).prop_map(|(version, applied)| Step::Applied { version, applied }),
        Just(Step::Stopped),
    ];
    (0u8..6, kind)
}

fn object(members: &[(&str, Value)]) -> Value {
    Value::Object(
        members
            .iter()
            .map(|(key, value)| (Key::new(key).unwrap(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

fn row(step: &Step, at: i64) -> (&'static str, &'static str, Value) {
    let clock = text(&UtcNanos::from_parts(at, 0).unwrap().to_string());
    match step {
        Step::Tick => (ACCOUNT, "ClockAdvanced", object(&[("clock", clock)])),
        Step::SchedulerTick => (CLOCK, "ClockAdvanced", object(&[("clock", clock)])),
        Step::Mark { bid, sane, session } => (
            ACCOUNT,
            "MarkUpdated",
            object(&[
                ("instrument_id", text(ASSET)),
                ("price", text("1")),
                ("source", text("quote")),
                ("feed", text("iex")),
                ("bid", text(&bid.to_string())),
                ("sane", Value::Bool(*sane)),
                ("session", text(SESSIONS.get(*session).unwrap())),
                ("risk_clock", clock),
            ]),
        ),
        Step::Snapshot => (
            ACCOUNT,
            "AccountSnapshotRecorded",
            object(&[
                ("status", text("ACTIVE")),
                ("crypto_status", text("ACTIVE")),
                ("trading_blocked", Value::Bool(false)),
                ("account_blocked", Value::Bool(false)),
                ("trade_suspended_by_user", Value::Bool(false)),
                (
                    "multiplier",
                    Value::Int(mandate_canon::Int::new(1).unwrap()),
                ),
                ("equity", text("100")),
                ("cash", text("100")),
                ("buying_power", text("100")),
                ("non_marginable_buying_power", text("100")),
                ("risk_clock", clock),
            ]),
        ),
        Step::Deployed(version) => (
            "ctl:ws1",
            "AgentDeployed",
            object(&[
                ("agent_id", text(AGENT)),
                ("mandate_version", text(version)),
            ]),
        ),
        Step::Applied { version, applied } => (
            ACCOUNT,
            "MandateVersionApplied",
            object(&[
                ("agent_id", text(AGENT)),
                ("new_version", text(version)),
                (
                    "result",
                    text(if *applied { "applied" } else { "rejected" }),
                ),
                ("risk_clock", clock),
            ]),
        ),
        Step::Stopped => (
            "ctl:ws1",
            "AgentStopped",
            object(&[("agent_id", text(AGENT))]),
        ),
    }
}

/// Each fact found by searching back from the newest row: what the forward fold must agree with.
fn oracle(steps: &[(i64, Step)]) -> Result<(String, i64, u32, i64), Held> {
    if !steps
        .iter()
        .any(|(_, step)| matches!(step, Step::SchedulerTick))
    {
        return Err(Held::NoClock);
    }
    let now = steps
        .iter()
        .filter(|(_, step)| !matches!(step, Step::Deployed(_) | Step::Stopped))
        .map(|(at, _)| *at)
        .max()
        .ok_or(Held::NoClock)?;
    let mandate = steps
        .iter()
        .rev()
        .find_map(|(_, step)| match step {
            Step::Deployed(version) => Some(Some(version.clone())),
            Step::Applied {
                version,
                applied: true,
            } => Some(Some(version.clone())),
            Step::Stopped => Some(None),
            _ => None,
        })
        .flatten()
        .ok_or(Held::NoMandate)?;
    let snapshot_at = steps
        .iter()
        .rev()
        .find(|(_, step)| matches!(step, Step::Snapshot))
        .map(|(at, _)| *at)
        .ok_or(Held::NoSnapshot)?;
    if now - snapshot_at >= i64::from(FRESH.snapshot_max_age_s) {
        return Err(Held::StaleSnapshot);
    }
    let (mark_at, bid, sane) = steps
        .iter()
        .rev()
        .find_map(|(at, step)| match step {
            Step::Mark { bid, sane, session } if *session < 2 => Some((*at, *bid, *sane)),
            _ => None,
        })
        .ok_or(Held::NoMark)?;
    if !sane {
        return Err(Held::InsaneMark);
    }
    if now - mark_at >= i64::from(FRESH.mark_max_age_s) {
        return Err(Held::StaleMark);
    }
    Ok((mandate, mark_at, bid, now))
}

proptest! {
    #[test]
    fn facts_are_the_journaled_rows_alone(raw in prop::collection::vec(step(), 0..24)) {
        let (mut account, mut scheduler) = (START, START);
        let steps: Vec<(i64, Step)> = raw
            .into_iter()
            .map(|(gap, step)| {
                let at = if matches!(step, Step::SchedulerTick) {
                    &mut scheduler
                } else {
                    &mut account
                };
                *at += i64::from(gap);
                (*at, step)
            })
            .collect();
        let mut forward = Journaled::new(ACCOUNT, CLOCK, AGENT);
        for (at, step) in &steps {
            let (stream, event_type, payload) = row(step, *at);
            forward.fold(Row { stream, event_type, payload: &payload }).unwrap();
        }
        let answered = forward.facts(ASSET, FRESH).map(|facts| {
            (
                facts.mandate_version,
                facts.mark.at.secs(),
                facts.mark.bid,
                facts.now.secs(),
            )
        });
        let expected = oracle(&steps).map(|(mandate, mark_at, bid, now)| {
            (mandate, mark_at, Price::parse(&bid.to_string()).unwrap(), now)
        });
        prop_assert_eq!(answered, expected);
    }
}
