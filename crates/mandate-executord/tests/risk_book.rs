//! The executor process's risk book (DEC-2804 items 2, 4, 5 and 8, DEC-2399): each deployed agent's
//! fold opens flat at its first deployment from inputs the journal fixes, the records it owes
//! follow their cause in the next batch, a crash's records are owed at the head, a restart's
//! replay appends nothing, and an agent that cannot be read alone on its account has no fold.

mod support;

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use mandate_accounting::InstrumentId;
use mandate_canon::{Value, parse};
use mandate_domain::{AgentMode, AssetClass, Environment};
use mandate_executor::risk_state::{RiskView, UnreadableCause};
use mandate_executor::{AgentId, EventId, MandateView};
use mandate_executord::Alert;
use mandate_executord::establish::ToCopy;
use mandate_executord::risk::{NOT_OPENED, RiskOpening, RiskOpenings};
use mandate_journal::StoredEvent;
use mandate_num::Usd;
use mandate_spec::context::stop_limit_asset_classes;
use mandate_spec::document::ProvenanceMap;
use mandate_spec::{Mandate, ValidatedMandate, ValidationContext};
use mandate_time::Date;
use support::{
    ACCOUNT, ACCOUNT_STREAM, AGENT, CONNECTION, CONTROL_STREAM, Error, Fault, Journal, NOON,
    NoFacts, OTHER_AGENT, Paper, TwoPercentStop, at, deploy, deployed, start_wired, stopped, tick,
    wired,
};

/// `research_equity_pinned`'s pinned XYZ, by its canonical asset id.
const XYZ: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
/// After New York midnight following `NOON`'s Monday.
const TUESDAY: &str = "2026-09-22T04:00:05Z";
const OTHER_VERSION: &str =
    "sha256:6666666666666666666666666666666666666666666666666666666666666666";

fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../fixtures/refcases/mandate.json"))
        .unwrap_or_default()
}

/// `research_equity_pinned` with `max_instruments`, validated on `equity`.
fn validated(max_instruments: u64, equity: Usd) -> Option<ValidatedMandate> {
    let mut document = fixture()["bases"]["research_equity_pinned"]["mandate"].clone();
    *document.pointer_mut("/universe/max_instruments")? = max_instruments.into();
    let canonical = parse(&serde_json::to_vec(&document).ok()?).ok()?;
    let context = ValidationContext {
        account_equity_usd: equity,
        other_allocations_usd: Usd::ZERO,
        validation_date: Date::parse("2026-08-31").ok()?,
        registry: None,
        provenance: ProvenanceMap::default(),
        workspace_users: 1,
        approver_users: 1,
        independent_approval_required: false,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
        current_mandate_version: None,
        stop_limit_asset_classes: stop_limit_asset_classes("alpaca"),
    };
    ValidatedMandate::new(Mandate::parse(&canonical).ok()?, &context, &[]).ok()
}

/// What the openings were asked: the agent, the control rows' last seq, the equity, the date.
type Asked = Rc<std::cell::RefCell<Vec<(String, u64, String, String)>>>;

/// Opens every agent on `research_equity_pinned` with `max_instruments`, recording each ask.
struct Fixed {
    max_instruments: u64,
    asked: Asked,
    opened: Rc<Cell<usize>>,
    /// The agents whose risk the book last handed the gate.
    published: Rc<std::cell::RefCell<Vec<String>>>,
}

impl Fixed {
    fn new(max_instruments: u64) -> Self {
        Self {
            max_instruments,
            asked: Asked::default(),
            opened: Rc::new(Cell::new(0)),
            published: Rc::default(),
        }
    }
}

impl RiskOpenings for Fixed {
    fn opening(
        &self,
        agent: &AgentId,
        control: &[StoredEvent],
        equity: Usd,
        date: Date,
    ) -> Option<RiskOpening> {
        self.asked.borrow_mut().push((
            agent.0.clone(),
            control.last().map_or(0, |row| row.seq),
            equity.to_string(),
            date.to_string(),
        ));
        self.opened.set(self.opened.get() + 1);
        Some(RiskOpening {
            mandate: validated(self.max_instruments, equity)?,
            instrument: XYZ.to_owned(),
            asset_class: AssetClass::UsEquity,
            inherited_loss_usd: Usd::ZERO,
            mark_max_age_s: 60,
        })
    }

    fn read(&self, risks: BTreeMap<AgentId, mandate_executord::risk::GateRisk>) {
        *self.published.borrow_mut() = risks.keys().map(|agent| agent.0.clone()).collect();
    }
}

fn process(
    journal: &Journal,
    openings: Fixed,
) -> Result<mandate_executord::ExecutorProcess<Journal, Paper>, Error> {
    let mut wiring = wired(Box::new(NoFacts), Box::new(TwoPercentStop))?;
    wiring.risk = Box::new(openings);
    start_wired(journal, &Paper::new(journal), wiring)
}

fn agent(id: &str) -> AgentId {
    AgentId(id.to_owned())
}

fn readable(view: &RiskView) -> bool {
    matches!(view, RiskView::Readable { .. })
}

fn payload_member(row: &StoredEvent, member: &str) -> Option<String> {
    parse(&row.body)
        .ok()?
        .get("payload")?
        .get(member)?
        .as_str()
        .map(str::to_owned)
}

fn body_member(row: &StoredEvent, member: &str) -> Option<String> {
    parse(&row.body)
        .ok()?
        .get(member)?
        .as_str()
        .map(str::to_owned)
}

/// Item 2: with no opening read, the agent has no fold, `not_opened`, `exits_only`.
#[test]
fn an_agent_whose_opening_does_not_read_has_no_fold() -> Result<(), Error> {
    let journal = deployed()?;
    let mut process = start_wired(
        &journal,
        &Paper::new(&journal),
        wired(Box::new(NoFacts), Box::new(TwoPercentStop))?,
    )?;
    process.step(at(NOON)?);
    assert!(process.ready(), "{:?}", process.lifecycle().state());
    assert_eq!(process.risk_view(&agent(AGENT)), NOT_OPENED);
    assert_eq!(
        NOT_OPENED,
        RiskView::Unreadable {
            cause: UnreadableCause::NotOpened,
            head: 0,
            mode: AgentMode::ExitsOnly,
        }
    );
    Ok(())
}

/// Item 2: the deployed agent's fold opens flat at its deployment, asked over the control rows up
/// to its `AgentDeployed` and the equity of the first snapshot the account stream journals, on
/// the deployment's New York date; its view is readable at the stream's head.
#[test]
fn a_deployed_agent_opens_from_its_first_deployment() -> Result<(), Error> {
    let journal = deployed()?;
    let openings = Fixed::new(1);
    let asked = Rc::clone(&openings.asked);
    let mut process = process(&journal, openings)?;
    process.step(at(NOON)?);
    assert!(process.ready(), "{:?}", process.lifecycle().state());
    let deployment = journal
        .rows_of(CONTROL_STREAM)
        .iter()
        .find(|row| row.event_type == "AgentDeployed")
        .map(|row| row.seq)
        .ok_or("deployment")?;
    let snapshot = journal
        .rows_of(ACCOUNT_STREAM)
        .iter()
        .find(|row| row.event_type == "AccountSnapshotRecorded")
        .and_then(|row| payload_member(row, "equity"))
        .ok_or("snapshot")?;
    assert_eq!(
        asked.borrow().first().cloned(),
        Some((
            AGENT.to_owned(),
            deployment,
            Usd::parse(&snapshot)?.to_string(),
            "2026-09-21".to_owned(),
        ))
    );
    let head = journal.rows_of(ACCOUNT_STREAM).len() as u64;
    match process.risk_view(&agent(AGENT)) {
        RiskView::Readable { head: at, .. } => assert_eq!(at, head),
        other => panic!("readable: {other:?}"),
    }
    Ok(())
}

/// Item 5: a tick that crosses New York midnight is copied onto the account stream, its `clock`
/// and `risk_clock` the tick's and its cause the scheduler's event, and the `RiskDayStarted` it
/// returns follows in the next batch, naming the agent's version (journal spec §9). A tick that
/// returns nothing is not copied.
#[test]
fn a_tick_across_midnight_is_copied_and_its_day_start_follows() -> Result<(), Error> {
    let journal = deployed()?;
    let mut process = process(&journal, Fixed::new(1))?;
    process.step(at(NOON)?);
    let copies = |journal: &Journal| {
        journal
            .rows_of(ACCOUNT_STREAM)
            .into_iter()
            .filter(|row| row.event_type == "ClockAdvanced")
            .count()
    };
    let before = copies(&journal);
    tick(&journal, "t2", "2026-09-21T15:00:01Z")?;
    process.step(at("2026-09-21T15:00:01Z")?);
    assert_eq!(
        copies(&journal),
        before,
        "a tick that returns nothing is not copied"
    );

    tick(&journal, "t3", TUESDAY)?;
    process.step(at(TUESDAY)?);
    let rows = journal.rows_of(ACCOUNT_STREAM);
    let copy = rows
        .iter()
        .rposition(|row| row.event_type == "ClockAdvanced")
        .ok_or("copy")?;
    let clock = at(TUESDAY)?.to_string();
    assert_eq!(payload_member(&rows[copy], "clock"), Some(clock.clone()));
    assert_eq!(payload_member(&rows[copy], "risk_clock"), Some(clock));
    assert_eq!(
        body_member(&rows[copy], "causation_id").as_deref(),
        Some("t3")
    );
    let next = rows.get(copy + 1).ok_or("the day start")?;
    assert_eq!(next.event_type, "RiskDayStarted");
    assert_eq!(
        journal.mandate_refs("RiskDayStarted"),
        vec![Some(support::VERSION.to_owned())]
    );
    assert!(readable(&process.risk_view(&agent(AGENT))));
    Ok(())
}

/// The risk records and copies the account stream holds, in order: what the risk book wrote.
fn risk_rows(journal: &Journal) -> Vec<String> {
    journal
        .rows_of(ACCOUNT_STREAM)
        .into_iter()
        .filter(|row| {
            matches!(
                row.event_type.as_str(),
                "ClockAdvanced"
                    | "RiskDayStarted"
                    | "RiskLimitTriggered"
                    | "RiskLimitLifted"
                    | "HighWaterMarkReset"
                    | "InstrumentRestrictionChanged"
                    | "GoalCompleted"
            )
        })
        .map(|row| row.event_type)
        .collect()
}

/// The risk state a view reads, whatever head it is at.
fn snapshot(view: &RiskView) -> Option<String> {
    match view {
        RiskView::Readable { snapshot, .. } => Some(format!("{snapshot:?}")),
        RiskView::Unreadable { .. } => None,
    }
}

/// DEC-2399 items 1 and 2: a restart replays the stream to the live view and appends no risk
/// record or copy, only its own startup reconciliation.
#[test]
fn a_restart_reaches_the_live_view_and_appends_nothing() -> Result<(), Error> {
    let journal = deployed()?;
    let mut live = process(&journal, Fixed::new(1))?;
    live.step(at(NOON)?);
    tick(&journal, "t3", TUESDAY)?;
    live.step(at(TUESDAY)?);
    let view = live.risk_view(&agent(AGENT));
    let written = risk_rows(&journal);
    drop(live);
    let mut restarted = process(&journal, Fixed::new(1))?;
    restarted.step(at(TUESDAY)?);
    assert_eq!(risk_rows(&journal), written);
    let replayed = restarted.risk_view(&agent(AGENT));
    assert!(snapshot(&view).is_some());
    assert_eq!(snapshot(&replayed), snapshot(&view));
    Ok(())
}

/// DEC-2399 item 3, DEC-2804 item 4: a crash between the copy and its day start leaves the day
/// start owed, and the next process appends it at the head before anything else.
#[test]
fn a_crash_between_a_copy_and_its_records_owes_them_at_the_head() -> Result<(), Error> {
    let journal = deployed()?;
    let mut live = process(&journal, Fixed::new(1))?;
    live.step(at(NOON)?);
    tick(&journal, "t3", TUESDAY)?;
    live.step(at(TUESDAY)?);
    let whole = journal.rows_of(ACCOUNT_STREAM);
    let copy = whole
        .iter()
        .rposition(|row| row.event_type == "ClockAdvanced")
        .ok_or("copy")?;
    let view = live.risk_view(&agent(AGENT));
    let written = risk_rows(&journal);
    drop(live);
    journal.truncate(ACCOUNT_STREAM, copy + 1);
    let mut restarted = process(&journal, Fixed::new(1))?;
    restarted.step(at(TUESDAY)?);
    let rows = journal.rows_of(ACCOUNT_STREAM);
    assert_eq!(
        rows.get(copy + 1).map(|row| row.event_type.as_str()),
        Some("RiskDayStarted")
    );
    assert_eq!(
        payload_member(&rows[copy + 1], "risk_clock"),
        payload_member(&whole[copy + 1], "risk_clock")
    );
    assert_eq!(risk_rows(&journal), written);
    assert!(snapshot(&view).is_some());
    assert_eq!(
        snapshot(&restarted.risk_view(&agent(AGENT))),
        snapshot(&view)
    );
    Ok(())
}

/// Item 8: a second agent that has not stopped on the account opens no fold for either, and an
/// open fold is dropped when it is deployed; a stopped one frees the account.
#[test]
fn a_second_running_agent_opens_no_fold() -> Result<(), Error> {
    let journal = deployed()?;
    let mut process = process(&journal, Fixed::new(1))?;
    process.step(at(NOON)?);
    assert!(readable(&process.risk_view(&agent(AGENT))));
    deploy(&journal, "c3", OTHER_AGENT, CONNECTION)?;
    process.step(at(NOON)?);
    assert_eq!(process.risk_view(&agent(AGENT)), NOT_OPENED);
    assert_eq!(process.risk_view(&agent(OTHER_AGENT)), NOT_OPENED);
    stopped(&journal, "c4", OTHER_AGENT)?;
    process.step(at(NOON)?);
    assert!(readable(&process.risk_view(&agent(AGENT))));
    assert_eq!(process.risk_view(&agent(OTHER_AGENT)), NOT_OPENED);
    Ok(())
}

/// Items 2 and 8: an agent whose universe may hold a second instrument opens no fold, nor does a
/// stopped agent.
#[test]
fn a_second_instrument_or_a_stop_opens_no_fold() -> Result<(), Error> {
    let journal = deployed()?;
    let mut wide = process(&journal, Fixed::new(2))?;
    wide.step(at(NOON)?);
    assert_eq!(wide.risk_view(&agent(AGENT)), NOT_OPENED);

    let journal = deployed()?;
    stopped(&journal, "c3", AGENT)?;
    let mut done = process(&journal, Fixed::new(1))?;
    done.step(at(NOON)?);
    assert_eq!(done.risk_view(&agent(AGENT)), NOT_OPENED);
    Ok(())
}

/// Item 2: a redeployment on the same version changes nothing; one on another version drops the
/// fold until R2b reads version changes.
#[test]
fn a_redeployment_on_another_version_drops_the_fold() -> Result<(), Error> {
    let journal = deployed()?;
    let mut process = process(&journal, Fixed::new(1))?;
    process.step(at(NOON)?);
    deploy(&journal, "c3", AGENT, CONNECTION)?;
    process.step(at(NOON)?);
    assert!(readable(&process.risk_view(&agent(AGENT))));
    journal.write(
        CONTROL_STREAM,
        "c4",
        "AgentDeployed",
        2,
        None,
        support::object(vec![
            ("agent_id", Value::Str(AGENT.to_owned())),
            ("connection_id", Value::Str(CONNECTION.to_owned())),
            ("mandate_version", Value::Str(OTHER_VERSION.to_owned())),
        ])?,
    )?;
    process.step(at(NOON)?);
    assert_eq!(process.risk_view(&agent(AGENT)), NOT_OPENED);
    Ok(())
}

/// Item 2: an agent left without a fold asks again only once the control or account stream grew.
#[test]
fn an_unopened_agent_is_asked_again_only_when_a_stream_grew() -> Result<(), Error> {
    let journal = deployed()?;
    let openings = Fixed::new(2);
    let opened = Rc::clone(&openings.opened);
    let mut process = process(&journal, openings)?;
    process.step(at(NOON)?);
    process.step(at(NOON)?);
    let asked = opened.get();
    assert!(asked >= 1);
    process.step(at(NOON)?);
    process.step(at(NOON)?);
    assert_eq!(opened.get(), asked, "nothing grew, nothing asked");
    deploy(&journal, "c3", AGENT, CONNECTION)?;
    process.step(at(NOON)?);
    assert_eq!(opened.get(), asked + 1, "the control stream grew");
    Ok(())
}

/// DEC-2399, DEC-2805, `AGENTS.md` rule 13: a batch of owed records the journal refuses as invalid
/// makes the fold unreadable, `append_failed`, so openings hold, and the process stays ready, so
/// exits, protection and the kill switch go on.
#[test]
fn a_refused_owed_batch_holds_openings_and_keeps_the_process() -> Result<(), Error> {
    let journal = deployed()?;
    let mut process = process(&journal, Fixed::new(1))?;
    process.step(at(NOON)?);
    tick(&journal, "t3", TUESDAY)?;
    journal.fail_next_of("RiskDayStarted", &[support::Fault::Invalid]);
    process.step(at(TUESDAY)?);
    assert!(process.ready(), "{:?}", process.lifecycle().state());
    assert!(
        matches!(
            process.risk_view(&agent(AGENT)),
            RiskView::Unreadable {
                cause: UnreadableCause::AppendFailed,
                ..
            }
        ),
        "{:?}",
        process.risk_view(&agent(AGENT))
    );
    Ok(())
}

/// DEC-2804 item 7, `AGENTS.md` rule 3: the gate is handed the agent's risk while its view reads,
/// and none once a refused batch has made it unreadable, in the same pass.
#[test]
fn the_gate_never_reads_a_view_that_failed() -> Result<(), Error> {
    let journal = deployed()?;
    let openings = Fixed::new(1);
    let published = Rc::clone(&openings.published);
    let mut process = process(&journal, openings)?;
    process.step(at(NOON)?);
    assert_eq!(*published.borrow(), vec![AGENT.to_owned()]);
    tick(&journal, "t3", TUESDAY)?;
    journal.fail_next_of("RiskDayStarted", &[support::Fault::Invalid]);
    process.step(at(TUESDAY)?);
    assert!(published.borrow().is_empty(), "{:?}", published.borrow());
    Ok(())
}

/// DEC-2399 item 3, DEC-2804 item 4: an owed batch in doubt is offered again first on the next
/// pass, the same records right after their cause, and nothing else is appended in between.
#[test]
fn an_owed_batch_in_doubt_goes_first_and_nothing_slips_between() -> Result<(), Error> {
    let journal = deployed()?;
    let mut process = process(&journal, Fixed::new(1))?;
    process.step(at(NOON)?);
    tick(&journal, "t3", TUESDAY)?;
    journal.fail_next_of("RiskDayStarted", &[support::Fault::Unavailable]);
    process.step(at(TUESDAY)?);
    tick(&journal, "t4", "2026-09-22T04:00:06Z")?;
    process.step(at("2026-09-22T04:00:06Z")?);
    assert!(process.ready(), "{:?}", process.lifecycle().state());
    let types = journal.types(ACCOUNT_STREAM);
    let copy = types
        .iter()
        .rposition(|kind| kind == "ClockAdvanced")
        .ok_or("copy")?;
    assert_eq!(
        types.get(copy + 1).map(String::as_str),
        Some("RiskDayStarted"),
        "{types:?}"
    );
    assert!(readable(&process.risk_view(&agent(AGENT))));
    Ok(())
}

/// A copy the journal refuses makes the view unreadable, keeps the process, and is not offered
/// again on the same day's next tick.
#[test]
fn a_refused_copy_is_not_offered_again_that_day() -> Result<(), Error> {
    let journal = deployed()?;
    let mut process = process(&journal, Fixed::new(1))?;
    process.step(at(NOON)?);
    tick(&journal, "t3", TUESDAY)?;
    journal.fail_next_of("ClockAdvanced", &[support::Fault::Invalid]);
    process.step(at(TUESDAY)?);
    assert!(process.ready(), "{:?}", process.lifecycle().state());
    assert!(matches!(
        process.risk_view(&agent(AGENT)),
        RiskView::Unreadable {
            cause: UnreadableCause::AppendFailed,
            ..
        }
    ));
    let before = journal.appends();
    tick(&journal, "t4", "2026-09-22T04:00:06Z")?;
    process.step(at("2026-09-22T04:00:06Z")?);
    assert_eq!(journal.appends(), before, "no copy offered again that day");
    Ok(())
}

/// `AGENTS.md` rule 13: the kill switch never waits on the risk book. A batch of owed records the
/// journal refuses leaves the fold unreadable and the switch applies in the same pass; one in
/// doubt holds every append, the switch's too, only until the journal answers (rule 5), and the
/// switch applies on the first pass it does.
#[test]
fn the_kill_switch_applies_while_the_books_records_cannot_be_appended() -> Result<(), Error> {
    let journal = deployed()?;
    let mut refused = process(&journal, Fixed::new(1))?;
    refused.step(at(NOON)?);
    tick(&journal, "t3", TUESDAY)?;
    journal.fail_next_of("RiskDayStarted", &[support::Fault::Invalid]);
    support::kill(&journal, AGENT, "01JKILL00000000000000000AA")?;
    refused.step(at(TUESDAY)?);
    assert_eq!(
        journal.switched(),
        vec![AGENT.to_owned()],
        "refused: applied"
    );
    assert!(refused.ready(), "{:?}", refused.lifecycle().state());

    let journal = deployed()?;
    let mut doubted = process(&journal, Fixed::new(1))?;
    doubted.step(at(NOON)?);
    tick(&journal, "t3", TUESDAY)?;
    journal.fail_next_of("RiskDayStarted", &[support::Fault::Unavailable]);
    support::kill(&journal, AGENT, "01JKILL00000000000000000AA")?;
    doubted.step(at(TUESDAY)?);
    let later = "2026-09-22T04:00:06Z";
    tick(&journal, "t4", later)?;
    doubted.step(at(later)?);
    assert_eq!(
        journal.switched(),
        vec![AGENT.to_owned()],
        "in doubt: applied once the journal answers"
    );
    assert!(doubted.ready(), "{:?}", doubted.lifecycle().state());
    Ok(())
}

/// DEC-2804 item 4: with the book's batch in doubt when a tick is handed, the book settles first
/// and the core takes the tick once, after it: the run ends where a run with no outage does, the
/// same rows in the same order and the same risk clock, and stays ready.
#[test]
fn the_book_settles_before_the_core_takes_a_tick() -> Result<(), Error> {
    let later = "2026-09-22T04:00:06Z";
    let run = |doubt: bool| -> Result<(Vec<String>, Option<i64>, bool), Error> {
        let journal = deployed()?;
        let mut process = process(&journal, Fixed::new(1))?;
        process.step(at(NOON)?);
        tick(&journal, "t3", TUESDAY)?;
        if doubt {
            journal.fail_next_of("RiskDayStarted", &[support::Fault::Unavailable]);
        }
        process.step(at(TUESDAY)?);
        tick(&journal, "t4", later)?;
        process.step(at(later)?);
        Ok((
            journal.types(ACCOUNT_STREAM),
            process.state().risk_clock().map(|clock| clock.secs()),
            process.ready(),
        ))
    };
    let (doubted, steady) = (run(true)?, run(false)?);
    assert!(doubted.2, "the doubted run stays ready");
    assert_eq!((doubted.0, doubted.1), (steady.0, steady.1));
    Ok(())
}

/// The journal's opaque alerts for an outage in `alerts`.
fn outages(alerts: &[Alert]) -> usize {
    alerts
        .iter()
        .filter(|alert| alert.key == "journal_unavailable" && alert.subject.is_none())
        .count()
}

/// The control stream's establishment `c1` of `conn1` as account `acct1`, due to be copied.
fn establishment() -> Result<ToCopy, Error> {
    let Value::Object(members) = support::object(vec![
        ("account_ref", Value::Str(ACCOUNT.to_owned())),
        ("connection_id", Value::Str(CONNECTION.to_owned())),
        ("environment", Value::Str("paper".to_owned())),
    ])?
    else {
        return Err("members".into());
    };
    Ok(ToCopy {
        original: EventId("c1".to_owned()),
        members,
    })
}

/// A process whose book's day start went in doubt at Tuesday's first tick, its copy committed.
fn day_start_in_doubt(
    journal: &Journal,
    faults: &[Fault],
) -> Result<mandate_executord::ExecutorProcess<Journal, Paper>, Error> {
    let mut process = process(journal, Fixed::new(1))?;
    process.step(at(NOON)?);
    tick(journal, "t3", TUESDAY)?;
    journal.fail_next_of("RiskDayStarted", faults);
    process.step(at(TUESDAY)?);
    Ok(process)
}

/// The journal's one opaque alert for an outage: an owed batch in doubt raises it on the pass it
/// goes in doubt, and not again while it stays in doubt.
#[test]
fn an_owed_batch_in_doubt_is_alerted_once() -> Result<(), Error> {
    let journal = deployed()?;
    let mut process = process(&journal, Fixed::new(1))?;
    process.step(at(NOON)?);
    tick(&journal, "t3", TUESDAY)?;
    journal.fail_next_of("RiskDayStarted", &[Fault::Unavailable, Fault::Unavailable]);
    let first = process.step(at(TUESDAY)?);
    let later = "2026-09-22T04:00:06Z";
    tick(&journal, "t4", later)?;
    let second = process.step(at(later)?);
    assert_eq!((outages(&first), outages(&second)), (1, 0));
    Ok(())
}

/// A copy in doubt raises the outage's alert, and the pass that commits it ends the outage, so the
/// next one is alerted again.
#[test]
fn a_copy_in_doubt_is_alerted_and_its_commit_ends_the_outage() -> Result<(), Error> {
    let journal = deployed()?;
    let mut process = process(&journal, Fixed::new(1))?;
    process.step(at(NOON)?);
    tick(&journal, "t3", TUESDAY)?;
    journal.fail_next_of("ClockAdvanced", &[Fault::Unavailable]);
    let first = process.step(at(TUESDAY)?);
    let later = "2026-09-22T04:00:06Z";
    tick(&journal, "t4", later)?;
    let second = process.step(at(later)?);
    let wednesday = "2026-09-23T04:00:05Z";
    tick(&journal, "t5", wednesday)?;
    journal.fail_next_of("ClockAdvanced", &[Fault::Unavailable]);
    let third = process.step(at(wednesday)?);
    assert_eq!(
        (outages(&first), outages(&second), outages(&third)),
        (1, 0, 1),
        "{:?}",
        journal.types(ACCOUNT_STREAM)
    );
    Ok(())
}

/// DEC-2804 item 4: an establishment's copy waits behind the book's batch in doubt, and is built
/// again at the head that batch leaves: both commit, the book's records right after their cause.
#[test]
fn an_establishment_copy_follows_the_books_batch_in_doubt() -> Result<(), Error> {
    let journal = deployed()?;
    let mut process = day_start_in_doubt(&journal, &[Fault::Unavailable])?;
    assert_eq!(
        process.copy_establishment(at(TUESDAY)?, &establishment()?),
        Ok(())
    );
    let types = journal.types(ACCOUNT_STREAM);
    assert_eq!(
        types.get(types.len().saturating_sub(3)..),
        Some(
            &[
                "ClockAdvanced".to_owned(),
                "RiskDayStarted".to_owned(),
                "ConnectionEstablished".to_owned()
            ][..]
        )
    );
    assert!(process.ready(), "{:?}", process.lifecycle().state());
    Ok(())
}

/// A fenced process offers nothing more: when the book's batch in doubt is fenced on its retry,
/// the establishment's copy is refused, nothing else is appended, and the process stops.
#[test]
fn a_fenced_retry_of_the_books_batch_appends_nothing_more() -> Result<(), Error> {
    let journal = deployed()?;
    let mut process = day_start_in_doubt(&journal, &[Fault::Unavailable, Fault::Fenced])?;
    let before = journal.appends();
    assert_eq!(
        process.copy_establishment(at(TUESDAY)?, &establishment()?),
        Err("copy_refused")
    );
    assert_eq!(journal.appends(), before + 1, "only the fenced retry");
    assert!(
        !journal
            .types(ACCOUNT_STREAM)
            .contains(&"RiskDayStarted".to_owned())
    );
    assert!(!process.ready(), "{:?}", process.lifecycle().state());
    Ok(())
}

/// The support snapshot's AAPL as XYZ, by its asset id, which the fold binds, so the agent trades
/// the instrument its fold follows.
struct Xyz;

impl mandate_executor::InstrumentSnapshot for Xyz {
    fn asset_class(&self, instrument: &InstrumentId) -> Option<mandate_accounting::AssetClass> {
        (instrument.as_str() == XYZ).then_some(mandate_accounting::AssetClass::UsEquity)
    }

    fn increment(&self, instrument: &InstrumentId) -> Option<mandate_num::ShareIncrement> {
        (instrument.as_str() == XYZ).then_some(mandate_num::ShareIncrement::Whole)
    }

    fn exit_tier(&self, instrument: &InstrumentId) -> Option<mandate_executor::ExitTier> {
        let aapl = InstrumentId::new(support::AAPL).ok()?;
        mandate_executor::InstrumentSnapshot::exit_tier(&support::Instruments, &aapl)
            .filter(|_| instrument.as_str() == XYZ)
    }
}

impl MandateView for Xyz {
    fn version(&self, agent: &AgentId) -> Option<mandate_executor::MandateVersion> {
        support::Mandate.version(agent)
    }

    fn crypto_stop_limit_offset(&self, agent: &AgentId) -> Option<mandate_num::Fraction> {
        support::Mandate.crypto_stop_limit_offset(agent)
    }

    fn covers(&self, _agent: &AgentId, instrument: &InstrumentId) -> bool {
        instrument.as_str() == XYZ
    }
}

/// The wiring of an agent trading XYZ under `openings`, every opening's gate facts read.
fn trading(openings: Fixed) -> Result<mandate_executord::Wiring, Error> {
    let mut wiring = wired(Box::new(support::Allowing), Box::new(TwoPercentStop))?;
    wiring.risk = Box::new(openings);
    wiring.instruments = Box::new(Xyz);
    wiring.mandates = Box::new(Xyz);
    Ok(wiring)
}

/// The agent proposes buying 10 XYZ at 150 to open, as intent `id`.
fn open_xyz(journal: &Journal, id: &str) -> Result<(), Error> {
    let text = |value: &str| Value::Str(value.to_owned());
    journal.write(
        &support::agent_stream(AGENT),
        id,
        "IntentProposed",
        1,
        Some("decision"),
        support::object(vec![
            ("instrument_id", text(XYZ)),
            ("limit_price", text("150")),
            ("purpose", text("open")),
            ("qty", text("10")),
            ("side", text("buy")),
            ("tif", text("day")),
            ("type", text("limit")),
        ])?,
    )
}

/// A feed quoting XYZ sane at its bid at the instant asked, or nothing while its bid is empty, and
/// marking XYZ.
#[derive(Clone)]
struct QuotingXyz(Rc<std::cell::RefCell<&'static str>>);

impl QuotingXyz {
    fn at(bid: &'static str) -> Self {
        Self(Rc::new(std::cell::RefCell::new(bid)))
    }

    fn quote(&self, bid: &'static str) {
        *self.0.borrow_mut() = bid;
    }
}

impl mandate_executord::quotes::QuoteFeed for QuotingXyz {
    fn observe(
        &self,
        instrument: &InstrumentId,
        now: mandate_time::UtcNanos,
    ) -> Result<mandate_executor::MarketObservation, &'static str> {
        if instrument.as_str() != XYZ || self.0.borrow().is_empty() {
            return Err("no_quote");
        }
        Ok(mandate_executor::MarketObservation {
            instrument: instrument.clone(),
            bid: mandate_num::Price::parse(*self.0.borrow()).ok(),
            bid_size: mandate_num::Qty::parse("2").ok(),
            ask: None,
            last_trade: None,
            mark: None,
            sane: true,
            observed_at: mandate_executor::RiskClock::from_secs(now.secs()),
        })
    }

    fn feed(&self) -> &'static str {
        "fixture"
    }
}

impl mandate_executord::marks::Marked for QuotingXyz {
    fn instruments(&self) -> BTreeSet<InstrumentId> {
        InstrumentId::new(XYZ).into_iter().collect()
    }
}

/// DEC-2829 item 3's process test: the agent's opening in its fold's instrument, marked first,
/// fills, and the reconciliation that books the fill journals the order's adopted state with its
/// compensation. The fold reads the whole batch, steps the fill on the mark, and the gate is
/// still handed the agent's risk, so the agent's next opening is judged on it rather than held.
#[test]
fn an_own_fill_booked_by_reconciliation_keeps_the_agents_risk_read() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let openings = Fixed::new(1);
    let published = Rc::clone(&openings.published);
    let process = holding(
        &journal,
        &broker,
        trading(openings)?,
        &QuotingXyz::at("150"),
    )?;
    let types = journal.types(ACCOUNT_STREAM);
    let mark = types.iter().position(|kind| kind == "MarkUpdated");
    let fill = types.iter().position(|kind| kind == "FillApplied");
    assert!(
        mark.is_some() && mark < fill && types.contains(&"CompensatingEvent".to_owned()),
        "{types:?}"
    );
    let RiskView::Readable { snapshot, .. } = process.risk_view(&agent(AGENT)) else {
        return Err(format!("{:?}", process.risk_view(&agent(AGENT))).into());
    };
    assert_eq!(
        snapshot.agent_equity,
        Usd::parse("10000")?,
        "10000 - 1500 + 10 × 150"
    );
    assert_eq!(snapshot.agent_mode, AgentMode::Normal);
    assert_eq!(*published.borrow(), vec![AGENT.to_owned()]);
    Ok(())
}

/// A process whose agent holds the 10 XYZ its opening bought at 150 on a mark at 150, booked by
/// the reconciliation at 15:05, `quotes` quoting and marking XYZ.
fn holding(
    journal: &Journal,
    broker: &Paper,
    mut wiring: mandate_executord::Wiring,
    quotes: &QuotingXyz,
) -> Result<mandate_executord::ExecutorProcess<Journal, Paper>, Error> {
    wiring.quotes = Box::new(quotes.clone());
    wiring.marked = Box::new(quotes.clone());
    let mut process = start_wired(journal, broker, wiring)?;
    process.step(at(NOON)?);
    open_xyz(journal, "01JINTENT0000000000000000Q")?;
    tick(journal, "t3", "2026-09-21T15:01:00Z")?;
    process.step(at("2026-09-21T15:01:00Z")?);
    let submitted = broker.submitted();
    assert_eq!(submitted.len(), 1, "{:?}", journal.types(ACCOUNT_STREAM));
    for id in &submitted {
        broker.fill(id, "150")?;
    }
    tick(journal, "t4", "2026-09-21T15:05:00Z")?;
    process.step(at("2026-09-21T15:05:00Z")?);
    Ok(process)
}

/// A fenced process acts on nothing more: when the records a committed mark leaves owed are
/// fenced, the pass stops at the mark, and nothing after it is journaled, the owed records included.
#[test]
fn records_owed_after_a_committed_batch_and_fenced_stop_the_pass() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let quotes = QuotingXyz::at("150");
    let mut process = holding(&journal, &broker, trading(Fixed::new(1))?, &quotes)?;
    quotes.quote("50");
    journal.fail_next_of("RiskLimitTriggered", &[Fault::Fenced]);
    tick(&journal, "t5", "2026-09-21T15:06:00Z")?;
    process.step(at("2026-09-21T15:06:00Z")?);
    let types = journal.types(ACCOUNT_STREAM);
    assert_eq!(
        types.last().map(String::as_str),
        Some("MarkUpdated"),
        "{types:?}"
    );
    assert!(
        !types.contains(&"RiskLimitTriggered".to_owned()),
        "{types:?}"
    );
    assert!(!process.ready(), "{:?}", process.lifecycle().state());
    Ok(())
}

/// DEC-2834: the agent's opening fills before the executor has marked its instrument. The fill is
/// booked and E is unknown: the view is `unpriced`, the gate is handed no risk for the agent, so
/// its next opening is held, and nothing trips. The first sane regular mark prices it, and the gate
/// reads the agent's risk again, E being 10000 − 1500 + 10 × 150.
#[test]
fn a_fill_before_any_mark_holds_openings_until_the_first_mark_prices_it() -> Result<(), Error> {
    let journal = deployed()?;
    let broker = Paper::new(&journal);
    let openings = Fixed::new(1);
    let published = Rc::clone(&openings.published);
    let quotes = QuotingXyz::at("");
    let mut process = holding(&journal, &broker, trading(openings)?, &quotes)?;
    let types = journal.types(ACCOUNT_STREAM);
    assert!(
        types.contains(&"FillApplied".to_owned()) && !types.contains(&"MarkUpdated".to_owned()),
        "{types:?}"
    );
    assert!(
        matches!(
            process.risk_view(&agent(AGENT)),
            RiskView::Unreadable {
                cause: UnreadableCause::Unpriced,
                mode: AgentMode::ExitsOnly,
                ..
            }
        ),
        "{:?}",
        process.risk_view(&agent(AGENT))
    );
    assert!(published.borrow().is_empty(), "{:?}", published.borrow());
    let tripped = [
        "RiskLimitTriggered",
        "RiskLimitLifted",
        "InstrumentRestrictionChanged",
        "AgentModeApplied",
    ];
    assert!(
        !types.iter().any(|kind| tripped.contains(&kind.as_str())),
        "nothing trips while unpriced: {types:?}"
    );
    quotes.quote("150");
    tick(&journal, "t5", "2026-09-21T15:06:00Z")?;
    process.step(at("2026-09-21T15:06:00Z")?);
    let RiskView::Readable { snapshot, .. } = process.risk_view(&agent(AGENT)) else {
        return Err(format!("{:?}", process.risk_view(&agent(AGENT))).into());
    };
    assert_eq!(snapshot.agent_equity, Usd::parse("10000")?);
    assert_eq!(snapshot.agent_mode, AgentMode::Normal);
    assert_eq!(*published.borrow(), vec![AGENT.to_owned()]);
    Ok(())
}
