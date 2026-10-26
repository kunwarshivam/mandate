//! E7-4b (journal spec v0.104 §9.28, DEC-2828, DEC-2830): the process hands every owner's
//! acknowledgment on the control stream for an agent it follows to the executor, which copies it
//! onto the account stream and judges it there. The overlay is the latest membership record's
//! `independent_approval_required`, `false` with none and `true` while it does not read; an
//! acknowledgment record that does not read holds openings and hands nothing.

mod support;

use mandate_canon::{Int, Value};
use mandate_process::State;
use support::{
    ACCOUNT_STREAM, AGENT, CONTROL_STREAM, Error, Journal, NOON, Paper, at, deployed, mode_applied,
    object, opened, propose, start, start_allowing,
};

const OPEN: &str = "01JINTENT0000000000000000A";
const EXIT: &str = "01JINTENT0000000000000000C";

const HOLD: &str = "01JBHD00001000000000000000";
const ACK: &str = "01JBAK00001000000000000000";

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

fn noon() -> Result<u64, Error> {
    Ok(u64::try_from(at(NOON)?.secs())?)
}

/// The account with every agent held `exits_only` by the executor's own reconciliation hold, a
/// minute before noon.
fn held() -> Result<Journal, Error> {
    let journal = deployed()?;
    opened(&journal)?;
    mode_applied(
        &journal,
        HOLD,
        "*",
        "exits_only",
        "reconciliation:incomplete",
        noon()?.saturating_sub(60),
    )?;
    Ok(journal)
}

/// The owner's own acknowledgment of the hold, stepped up ten seconds before noon.
fn acknowledge(journal: &Journal) -> Result<(), Error> {
    let noon = noon()?;
    journal.write(
        CONTROL_STREAM,
        ACK,
        "OwnerAcknowledged",
        2,
        None,
        object(vec![
            ("agent", text(AGENT)),
            ("event", text(HOLD)),
            ("user", text("user_owner_01")),
            ("requested_by", text("user_owner_01")),
            ("independent_required", Value::Bool(false)),
            ("request", Value::Null),
            ("submitted_at", Value::Int(Int::new(noon).ok_or("noon")?)),
            ("risk_clock", text(&at(NOON)?.to_string())),
            (
                "step_up",
                object(vec![
                    ("assertion_id", text("assert_ak00001")),
                    (
                        "authenticated_at",
                        Value::Int(Int::new(noon.saturating_sub(10)).ok_or("noon")?),
                    ),
                    ("method", text("cli_confirm")),
                ])?,
            ),
            ("subject", text("reconciliation")),
        ])?,
    )
}

fn membership(journal: &Journal, id: &str, required: Value) -> Result<(), Error> {
    journal.write(
        CONTROL_STREAM,
        id,
        "MemberRoleChanged",
        1,
        None,
        object(vec![("independent_approval_required", required)])?,
    )
}

/// The account stream's records after the deployment's own, from the hold on.
fn after_hold(journal: &Journal) -> Vec<(String, Value)> {
    let rows = journal.rows_of(ACCOUNT_STREAM);
    let from = rows
        .iter()
        .position(|row| row.event_id == HOLD)
        .unwrap_or(rows.len());
    rows[from..]
        .iter()
        .skip(1)
        .filter_map(|row| {
            let body = mandate_canon::parse(&row.body).ok()?;
            Some((row.event_type.clone(), body.get("payload")?.clone()))
        })
        .collect()
}

fn run(journal: &Journal) -> Result<(), Error> {
    let broker = Paper::new(journal);
    let mut process = start(journal, &broker)?;
    process.step(at(NOON)?);
    process.step(at(NOON)?);
    Ok(())
}

/// An accepted acknowledgment is copied, and the very next record lifts the hold it names, caused
/// by the copy; a restart copies it no second time.
#[test]
fn an_acknowledgment_on_the_control_stream_lifts_the_hold_once() -> Result<(), Error> {
    let journal = held()?;
    acknowledge(&journal)?;
    run(&journal)?;
    run(&journal)?;
    let after = after_hold(&journal);
    let types: Vec<&str> = after.iter().map(|(t, _)| t.as_str()).collect();
    let copy = types.iter().position(|t| *t == "OwnerAcknowledged");
    assert_eq!(
        types.iter().filter(|t| **t == "OwnerAcknowledged").count(),
        1,
        "one copy: {types:?}"
    );
    let lift = copy.and_then(|c| after.get(c + 1));
    assert_eq!(
        lift.map(|(t, p)| (t.as_str(), p.get("to"), p.get("restriction"))),
        Some((
            "AgentModeApplied",
            Some(&text("normal")),
            Some(&text("reconciliation:incomplete"))
        )),
        "{types:?}"
    );
    Ok(())
}

/// With the latest membership record requiring independence, the owner's own acknowledgment is
/// copied and refused `not_independent`; an earlier record that did not require it does not count.
#[test]
fn the_latest_membership_record_requires_independence() -> Result<(), Error> {
    let journal = held()?;
    membership(&journal, "mr1", Value::Bool(false))?;
    membership(&journal, "mr2", Value::Bool(true))?;
    acknowledge(&journal)?;
    run(&journal)?;
    let after = after_hold(&journal);
    let types: Vec<&str> = after.iter().map(|(t, _)| t.as_str()).collect();
    let copy = types.iter().position(|t| *t == "OwnerAcknowledged");
    let refusal = copy.and_then(|c| after.get(c + 1));
    assert_eq!(
        refusal.map(|(t, p)| (t.as_str(), p.get("reason"))),
        Some(("OwnerCommandRefused", Some(&text("not_independent")))),
        "{types:?}"
    );
    assert!(!types.contains(&"AgentModeApplied"), "{types:?}");
    Ok(())
}

/// A membership record whose `independent_approval_required` does not read is never read as
/// `false`: it requires independence, so the owner's own acknowledgment is copied and refused
/// `not_independent`, in the journal, and the hold stands.
#[test]
fn an_unreadable_membership_record_requires_independence() -> Result<(), Error> {
    for unreadable in [text("no"), Value::Null] {
        let journal = held()?;
        membership(&journal, "mr1", Value::Bool(false))?;
        membership(&journal, "mr2", unreadable.clone())?;
        acknowledge(&journal)?;
        run(&journal)?;
        let after = after_hold(&journal);
        let types: Vec<&str> = after.iter().map(|(t, _)| t.as_str()).collect();
        let copy = types.iter().position(|t| *t == "OwnerAcknowledged");
        assert_eq!(
            copy.and_then(|c| after.get(c + 1))
                .map(|(t, p)| (t.as_str(), p.get("reason"))),
            Some(("OwnerCommandRefused", Some(&text("not_independent")))),
            "{unreadable:?}: {types:?}"
        );
        assert!(
            !types.contains(&"AgentModeApplied"),
            "{unreadable:?}: {types:?}"
        );
    }
    Ok(())
}

/// An acknowledgment on the control stream that does not read holds every opening, and only
/// openings: the risk exit is handed, nothing is copied, and the process keeps running.
#[test]
fn an_unreadable_acknowledgment_holds_openings_and_hands_every_exit() -> Result<(), Error> {
    let journal = deployed()?;
    journal.write(
        CONTROL_STREAM,
        ACK,
        "OwnerAcknowledged",
        2,
        None,
        object(vec![("event", text(HOLD))])?,
    )?;
    propose(&journal, AGENT, OPEN, "buy", "10", "150", "open")?;
    propose(&journal, AGENT, EXIT, "sell", "10", "150", "risk_exit")?;
    let broker = Paper::new(&journal);
    let mut process = start_allowing(&journal, &broker)?;
    process.step(at(NOON)?);
    process.step(at(NOON)?);
    assert_eq!(journal.received(), [EXIT], "the exit only");
    assert!(
        !journal
            .types(ACCOUNT_STREAM)
            .iter()
            .any(|t| t == "OwnerAcknowledged"),
        "nothing copied"
    );
    assert!(
        !matches!(process.lifecycle().state(), State::Exited(_)),
        "still running: {:?}",
        process.lifecycle().state()
    );
    Ok(())
}
