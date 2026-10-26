//! The LT-10 install probes (E7-26's shell row): the in-module pins DEC-77's 2026-09-27 amendment
//! names for a review-required fact in an implementation PR — "A test that a review ruling
//! requires in an implementation PR goes in-module, under `#[cfg(test)]` in the crate's `src/`" —
//! against the #1326 review-of-record's escape list, four wrong wirings of
//! [`crate::tracer::Session::watch`] into [`watch_may_end`](crate::lt10::watch_may_end) that the
//! nine pure pins in `tests/lt10.rs` cannot see, because they hand the seam its facts directly:
//! (a) the seam never called — the bare `TERMINAL_STATUSES` read restored, which every pure pin
//! passes, since the seam it pins is pure and still right; (b) the seam asked only when the read
//! is terminal, so a still-working wake never asks and the cap's stop no longer comes **before
//! any read there** (DEC-877 item 1); (c) the alerts handed from anywhere but the run's own
//! report; (d) the bound re-derived somewhere other than the `Watch` the caller handed in. Each
//! probe drives one whole cycle and its watch over the doubles' world, through the door the
//! runner uses ([`crate::tracer::ProductionCycle::run_observed_watched`]), and reads its answer
//! off the returned `Report` and the probe's own counters, never off the run's own state.
//!
//! The facts the doubles cannot produce are scripted at this module's own edge doubles, under
//! this module: the broker read the scripted connector answers (the paper connector answers
//! `Absent` and never a terminal order), and the protection answer the scripted executor gives
//! (the paper double executor's own [`Protected::protected`] stays its refusal — an answer it
//! cannot give is a stop, never a guess at covered, `AGENTS.md` rule 3). Each scripted fact is
//! the executor codec's own shape, composed exactly as the connector double composes its
//! accepted answer, so nothing here pins behaviour no broker can produce.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use mandate_accounting::Side;
use mandate_alpaca::Pause;
use mandate_canon::Digest;
use mandate_domain::CapabilityProfile;
use mandate_executor::{
    BrokerOrder, BrokerOutcome, BrokerRequest, ClientOrderId, ConnectorError, Effect, EventId,
    FoldedEvent, NotificationRef,
};
use mandate_num::{Price, Qty};
use mandate_runtime::{Observation, RiskClock};
use mandate_time::UtcNanos;

use super::doubles::{
    AAPL, PaperExecutor, Script, ScriptedConnector, World, instrument, output, setup,
};
use super::{Connector, Executor, Stage};
use crate::error::Cause;
use crate::tracer::ProductionCycle;

/// The closes the fixture observation carries, stored under its own digest exactly as the host
/// hands the run one (E15-13): the same bytes `stages::observed`'s harness stores.
const CLOSES: &[u8] = br#"{"closes":[["2026-09-25","255.2"]]}"#;

/// More pauses than any probe here needs (the longest, the cap's, takes four) so a watch that
/// never ends fails at once rather than running until the harness times it out (#1041 review).
const PAUSE_BUDGET: usize = 130;

/// One second past a string instant, or the reason the string does not parse.
fn at(text: &str) -> Result<UtcNanos, String> {
    UtcNanos::parse(text).map_err(|error| error.to_string())
}

/// The watch's clock: the injected cell the runner's watch reads (ES-21 — the shell reads no wall
/// clock), moving `interval` forward per pause, exactly as the run-level fixtures' `Clock` does.
/// The fixture steps whole seconds only, and a step the calendar cannot hold leaves the clock
/// standing at the instant it read — neither happens in any probe, whose pauses are the watch's
/// own bounded `interval` and whose days end inside the calendar's years.
struct Clock {
    instant: Rc<Cell<UtcNanos>>,
    pauses: Rc<RefCell<Vec<Duration>>>,
}

impl Clock {
    fn start(text: &str) -> Result<Self, String> {
        Ok(Self {
            instant: Rc::new(Cell::new(at(text)?)),
            pauses: Rc::default(),
        })
    }
}

impl Pause for Clock {
    fn now(&self) -> UtcNanos {
        self.instant.get()
    }

    async fn pause(&self, duration: Duration) {
        let taken = self.pauses.borrow().len();
        assert!(
            taken < PAUSE_BUDGET,
            "the watch paused more than its budget of {PAUSE_BUDGET} pauses: a loop that never ends"
        );
        self.pauses.borrow_mut().push(duration);
        if let Ok(step) = i64::try_from(duration.as_secs()) {
            let moved = UtcNanos::from_parts(self.instant.get().secs().saturating_add(step), 0);
            if let Ok(next) = moved {
                self.instant.set(next);
            }
        }
    }
}

/// The executor's record of an unprotected interval at its bound, as its `max_unprotected_s`
/// watchdog journals it (`ProtectionChanged` `interval_limit`, trading-domain spec §5.4): the
/// record the interval's alert names, which is what makes the alert the interval's own (DEC-2514).
fn interval_limit(record: &EventId) -> mandate_executor::EventDraft {
    let null = |name: &'static str| (name, mandate_canon::Value::Null);
    let members: Vec<(&str, mandate_canon::Value)> = vec![
        ("instrument_id", mandate_canon::Value::Str(AAPL.to_owned())),
        (
            "action",
            mandate_canon::Value::Str("interval_limit".to_owned()),
        ),
        ("orders", mandate_canon::Value::Array(Vec::new())),
        ("awaiting", mandate_canon::Value::Array(Vec::new())),
        null("qty"),
        null("stop"),
        null("take_profit"),
        null("intent_id"),
        null("bracket"),
        null("entry"),
        null("agent_id"),
        null("replacing"),
        null("created_on"),
        null("sent"),
        null("uncovered"),
        null("acknowledged"),
        null("unprotected_fraction"),
    ];
    let mut payload = mandate_canon::Object::new();
    for (name, value) in members {
        if let Ok(key) = mandate_canon::Key::new(name) {
            payload.insert(key, value);
        }
    }
    mandate_executor::EventDraft {
        event_id: record.clone(),
        event_type: "ProtectionChanged".to_owned(),
        schema_version: 2,
        config_refs: mandate_canon::Object::new(),
        causation_id: None,
        payload: mandate_canon::Value::Object(payload),
    }
}

/// The executor edge double of the probes: every step the paper double executor's, beside the
/// two facts it cannot give — the protection answer, scripted (the covered half of LT-10's first
/// half, over the instrument the watch names, recorded so the probes can read what was asked),
/// and, when `its own key` is set, the interval's own alert key `unprotected_interval_limit`
/// notified on the first tick, exactly as the executor's own `max_unprotected_s` watchdog does
/// (notifications spec §4.2's `protection` row), which is the release of LT-10's second half.
struct Scripted {
    inner: PaperExecutor,
    covered: bool,
    its_own_key: bool,
    asked: Rc<RefCell<Vec<String>>>,
}

impl Scripted {
    fn new(world: &World, covered: bool, its_own_key: bool) -> Self {
        Self {
            inner: PaperExecutor {
                world: world.clone(),
                seen: Default::default(),
                inverted: false,
                last_client_order_id: None,
            },
            covered,
            its_own_key,
            asked: Rc::default(),
        }
    }
}

impl Executor for Scripted {
    fn reset(&mut self) -> Result<(), Cause> {
        self.inner.reset()
    }

    fn use_profile(&mut self, profile: CapabilityProfile) -> Result<(), Cause> {
        self.inner.use_profile(profile)
    }

    fn step(&mut self, input: mandate_executor::Input) -> Result<Vec<Effect>, Cause> {
        let ticked = matches!(input, mandate_executor::Input::Tick(_));
        let mut effects = self.inner.step(input)?;
        if self.its_own_key && ticked {
            self.its_own_key = false;
            let record = EventId("000001000000000000000000".to_owned());
            effects.push(Effect::Journal(interval_limit(&record)));
            effects.push(Effect::Notify(NotificationRef {
                subject_event: record,
                kind: mandate_executor::OwnerAlertKind::Protection,
            }));
        }
        Ok(effects)
    }

    fn protected(&mut self, instrument: &mandate_accounting::InstrumentId) -> Result<bool, Cause> {
        self.asked.borrow_mut().push(instrument.as_str().to_owned());
        Ok(self.covered)
    }

    fn committed(&mut self, event: &FoldedEvent) -> Result<(), Cause> {
        self.inner.committed(event)
    }
}

/// The connector edge double of the probes: every submission the paper connector's own answer,
/// and every order read back `status` — the facts the watch's broker read cannot produce over the
/// doubles — composed in the executor codec's own shape, the same members the connector double's
/// accepted answer carries.
struct Readable {
    inner: ScriptedConnector,
    status: &'static str,
    reads: Rc<Cell<u32>>,
}

impl Connector for Readable {
    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        if let BrokerRequest::GetOrderByClientId(client) = request {
            self.reads.set(self.reads.get().saturating_add(1));
            return Ok(BrokerOutcome::Order(entry(client, self.status)?));
        }
        self.inner.call(request)
    }
}

/// The order the scripted read answers with: the entry named as asked, the fixture's one share,
/// whole for the terminal fills and nothing for the still-working reads. A fixture that cannot
/// parse is a connector that cannot read it back (rule 3), never a guess.
fn entry(client: &ClientOrderId, status: &'static str) -> Result<BrokerOrder, ConnectorError> {
    let parse = |what: &'static str| ConnectorError::Unreadable { code: what };
    Ok(BrokerOrder {
        broker_order_id: "569fca5f-d21f-46a1-9e3f-21311cf912f0".to_owned(),
        client_order_id: Some(client.as_str().to_owned()),
        instrument: instrument().map_err(|_| parse("instrument"))?,
        side: Side::Buy,
        qty: Qty::parse("1").map_err(|_| parse("qty"))?,
        filled_qty: Qty::parse(if status == "filled" { "1" } else { "0" })
            .map_err(|_| parse("filled qty"))?,
        limit_price: Some(Price::parse("255.2").map_err(|_| parse("limit"))?),
        stop_price: None,
        status: status.to_owned(),
        reject_code: None,
        replaced_by_broker_order_id: None,
        legs: Vec::new(),
        created_on: None,
    })
}

/// The facts one probe drives.
struct Facts {
    /// The status every read of the entry answers.
    status: &'static str,
    /// The executor's protection answer.
    covered: bool,
    /// Whether the executor notifies `unprotected_interval_limit` on the first tick.
    its_own_key: bool,
}

/// The answer of one whole cycle and its watch, with the probes' counters: the broker-order
/// reads the connector double took, the executor's protection asks, and the clock's pauses.
type Watched = (
    Result<crate::tracer::Report, crate::error::ShellError>,
    Rc<Cell<u32>>,
    Rc<RefCell<Vec<String>>>,
    Rc<RefCell<Vec<Duration>>>,
);

/// One whole cycle and its watch over the doubles' world, through the runner's own door, with
/// the probe's facts scripted at the two edges the watch reads: the bound and interval the watch
/// is handed, the clock starting at `start`. Returns the run's answer and the probe's counters.
fn watched_of(
    world: &World,
    facts: Facts,
    bound: &str,
    start: &str,
    interval: u64,
) -> Result<Watched, String> {
    let mut stages = world.stages();
    let artifacts: Arc<dyn mandate_journal::ArtifactSource + Send + Sync> =
        Arc::new(BTreeMap::from([(Digest::of(CLOSES), CLOSES.to_vec())]));
    stages.artifacts = Some(artifacts);
    let executor = Scripted::new(world, facts.covered, facts.its_own_key);
    let asked = Rc::clone(&executor.asked);
    stages.executor = Box::new(executor);
    let reads = Rc::new(Cell::new(0));
    stages.connector = Box::new(Readable {
        inner: ScriptedConnector {
            world: world.clone(),
            script: Script::Accept,
        },
        status: facts.status,
        reads: Rc::clone(&reads),
    });
    let clock = Clock::start(start)?;
    let pauses = Rc::clone(&clock.pauses);
    let watch = crate::watch::Watch {
        pause: clock,
        bound: at(bound)?,
        interval: Duration::from_secs(interval),
    };
    let observation = Observation {
        source: "alpaca_iex_daily_bars".to_owned(),
        instrument_id: Some(instrument().map_err(|cause| cause.to_string())?),
        as_of: RiskClock::from_secs(setup()?.now.secs()),
        data_ref: Digest::of(CLOSES),
    };
    let report = ProductionCycle::new(stages, setup()?).run_observed_watched(
        observation,
        output().map_err(|cause| cause.to_string())?,
        &watch,
    );
    Ok((report, reads, asked, pauses))
}

/// Review vector (a) — the #1 escape the #1326 review named: the seam never called, the bare
/// `TERMINAL_STATUSES` read restored. A terminal read left uncovered with its interval's own key
/// never reported is exactly the fact LT-10 exists for (DEC-912's rejected alternative: "End the
/// watch once the entry is terminal, as the paper watch does"), and no pure pin can hold it,
/// because the seam it pins stays right while the watch stops asking it. The probe: the run
/// keeps reading at every wake — the entry read `filled` at every wake, the executor's answer
/// uncovered, no alert key — and fails closed at the cap, three terminal reads deep, the stop
/// `cancel_unconfirmed`. The restored bare read ends the run at the first wake, so the probe
/// fails on both the answer and the count. The two asks of every full wake (the facts before the
/// wake sends, the wake's own read after the executor's hands — DEC-877 item 1's "before any
/// read there", DEC-912 item 3's "every wake asks") are pinned here too: four wakes' worth is
/// seven asks of the pinned instrument, the cap's wake refusing after its first.
#[test]
fn a_terminal_uncovered_entry_keeps_reading_to_the_cap() -> Result<(), String> {
    let world = World::default();
    let (report, reads, asked, pauses) = watched_of(
        &world,
        Facts {
            status: "filled",
            covered: false,
            its_own_key: false,
        },
        "2026-09-25T19:59:50.000000000Z",
        "2026-09-25T19:58:00.000000000Z",
        30,
    )?;
    let code = report.as_ref().err().map(crate::error::ShellError::code);
    assert_eq!(
        code,
        Some("cancel_unconfirmed"),
        "the uncovered terminal entry fails closed at the cap: {report:?}"
    );
    assert_eq!(
        reads.get(),
        3,
        "the entry is read at every wake to the cap, terminal at each and never released"
    );
    assert_eq!(
        asked.borrow().len(),
        7,
        "two asks per full wake over the pinned instrument, one at the cap's: {:?}",
        asked.borrow()
    );
    assert!(
        asked.borrow().iter().all(|asked| asked == AAPL),
        "the watch asks for the pinned instrument and nothing else: {:?}",
        asked.borrow()
    );
    assert_eq!(pauses.borrow().len(), 4, "the watch's four wakes");
    Ok(())
}

/// The report's own alert keys are the only release of the second half (DEC-912 item 3: "only
/// once the unprotected interval is journaled and its alert key is in the run's report"), which
/// kills the wiring the #1326 review named where the alerts are handed from anywhere else: the
/// seam asked, the covered half asked, and the alerts an empty slice. The executor notifies the
/// interval's own key on the tick of the wake that reads the entry terminal, and the run ends
/// clean at that wake, one read deep, its own wake and never the next.
#[test]
fn the_intervals_own_alert_key_releases_the_entry_at_its_wake() -> Result<(), String> {
    let world = World::default();
    let (report, reads, asked, pauses) = watched_of(
        &world,
        Facts {
            status: "filled",
            covered: false,
            its_own_key: true,
        },
        "2026-09-25T19:59:50.000000000Z",
        "2026-09-25T19:58:00.000000000Z",
        30,
    )?;
    let report = report.map_err(|error| format!("the released run ends clean: {error:?}"))?;
    assert_eq!(
        report.submitted.len(),
        1,
        "the all-doubles keystone holds: {report:?}"
    );
    assert_eq!(
        report.alerts,
        ["unprotected_interval_limit"],
        "the release is the run's own reported key: {:?}",
        report.alerts
    );
    assert_eq!(
        reads.get(),
        1,
        "the entry is read once: its key releases it at the read's own wake"
    );
    assert_eq!(asked.borrow().len(), 2, "the wake's two asks");
    assert_eq!(pauses.borrow().len(), 1, "the run ends at the first wake");
    Ok(())
}

/// Review vector (b): the seam asked only when the read is terminal, so a still-working wake
/// never asks and the cap's stop no longer comes **before any read there** (DEC-877 item 1).
/// The still-working entry is read `accepted` at every wake; the watch hands its cancel at the
/// bound and reads on inside it, and the first wake at the cap stops `cancel_unconfirmed` before
/// any read there — the probe's count is the terminal-conditional wiring's failure, which reads
/// at the cap and never stops.
#[test]
fn a_still_working_wake_stops_at_the_cap_before_any_read() -> Result<(), String> {
    let world = World::default();
    let (report, reads, _asked, pauses) = watched_of(
        &world,
        Facts {
            status: "accepted",
            covered: true,
            its_own_key: false,
        },
        "2026-09-25T19:59:00.000000000Z",
        "2026-09-25T19:58:00.000000000Z",
        30,
    )?;
    let code = report.as_ref().err().map(crate::error::ShellError::code);
    assert_eq!(
        code,
        Some("cancel_unconfirmed"),
        "the still-working entry fails closed at the cap: {report:?}"
    );
    assert_eq!(
        reads.get(),
        2,
        "a read at every wake inside the bound and none at the cap: the bound's wake hands the cancel"
    );
    assert_eq!(pauses.borrow().len(), 4, "the watch's four wakes");
    Ok(())
}

/// Review vector (d): the bound re-derived somewhere other than the `Watch` the caller handed
/// in. A bound the calendar cannot place fails the watch closed as absent (FT-8, DEC-853 item 5,
/// `AGENTS.md` rule 3) at the first wake, before any read there — and a wiring that re-derives
/// its bound from the run's own clock or setup finds a placeable one instead and reads, so the
/// probe's refusal and its unread entry are that wiring's failure.
#[test]
fn the_watch_fails_closed_on_a_bound_it_cannot_place() -> Result<(), String> {
    let world = World::default();
    let (report, reads, _asked, pauses) = watched_of(
        &world,
        Facts {
            status: "filled",
            covered: true,
            its_own_key: false,
        },
        "2026-09-26T19:59:50.000000000Z",
        "2026-09-26T19:58:00.000000000Z",
        30,
    )?;
    let refused = matches!(
        report,
        Err(crate::error::ShellError::Refused {
            stage: Stage::Executor,
            cause: Cause::Absent { .. },
        })
    );
    assert!(
        refused,
        "a bound no session holds fails the watch closed as absent: {report:?}"
    );
    assert_eq!(
        reads.get(),
        0,
        "no read happens at a bound the calendar cannot place"
    );
    assert_eq!(pauses.borrow().len(), 1, "the first wake refuses");
    Ok(())
}
