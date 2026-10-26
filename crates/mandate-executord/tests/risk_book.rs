//! The executor process's risk book (DEC-2804 items 2, 4, 5 and 8, DEC-2399): each deployed agent's
//! fold opens flat at its first deployment from inputs the journal fixes, the records it owes
//! follow their cause in the next batch, a crash's records are owed at the head, a restart's
//! replay appends nothing, and an agent that cannot be read alone on its account has no fold.

mod support;

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use mandate_canon::{Value, parse};
use mandate_domain::{AgentMode, AssetClass, Environment};
use mandate_executor::AgentId;
use mandate_executor::risk_state::{RiskView, UnreadableCause};
use mandate_executord::risk::{NOT_OPENED, RiskOpening, RiskOpenings};
use mandate_journal::StoredEvent;
use mandate_num::Usd;
use mandate_spec::context::stop_limit_asset_classes;
use mandate_spec::document::ProvenanceMap;
use mandate_spec::{Mandate, ValidatedMandate, ValidationContext};
use mandate_time::Date;
use support::{
    ACCOUNT_STREAM, AGENT, CONNECTION, CONTROL_STREAM, Error, Journal, NOON, NoFacts, OTHER_AGENT,
    Paper, TwoPercentStop, at, deploy, deployed, start_wired, stopped, tick, wired,
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
}

impl Fixed {
    fn new(max_instruments: u64) -> Self {
        Self {
            max_instruments,
            asked: Asked::default(),
            opened: Rc::new(Cell::new(0)),
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
