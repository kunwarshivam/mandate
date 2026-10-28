//! DEC-2823 (E17-3; mandate spec §8.5): the executor process hands each followed agent's thesis
//! records to the core, which copies an admission onto the account stream as `UniverseChanged`,
//! naming the record it follows from, once: across passes and across a restart.

mod support;

use mandate_canon::{Int, Value};
use support::{ACCOUNT_STREAM, AGENT, Error, Paper, agent_stream, at, deployed, object, start};

const THESIS: &str = "01JTHES0000000000000000001";
const INSTRUMENT: &str = "00000000-0000-4000-8000-000000000015";

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

fn copies(journal: &support::Journal) -> Vec<Value> {
    journal.payloads("UniverseChanged")
}

#[test]
fn an_admitted_thesis_is_copied_once_across_passes_and_a_restart() -> Result<(), Error> {
    let journal = deployed()?;
    journal.write(
        &agent_stream(AGENT),
        THESIS,
        "ThesisProposed",
        1,
        None,
        object(vec![
            ("thesis_id", text("th-1")),
            ("lineage_id", text("ln-1")),
            ("instrument_id", text(INSTRUMENT)),
            ("revision", Value::Int(Int::new(1).ok_or("revision")?)),
            ("expires_at", text("2026-09-30T00:00:00.000000000Z")),
            ("admitted", Value::Bool(true)),
            ("reason", Value::Null),
        ])?,
    )?;
    let broker = Paper::new(&journal);
    let mut process = start(&journal, &broker)?;
    process.step(at("2026-09-21T15:00:00Z")?);
    process.step(at("2026-09-21T15:01:00Z")?);
    let written = copies(&journal);
    assert_eq!(written.len(), 1, "{:?}", journal.types(ACCOUNT_STREAM));
    let copy = written.first().ok_or("the copy")?;
    for (member, want) in [
        ("agent_id", AGENT),
        ("instrument", INSTRUMENT),
        ("change", "admitted"),
        ("reason", "thesis_admitted"),
        ("thesis_id", "th-1"),
        ("lineage_id", "ln-1"),
    ] {
        assert_eq!(copy.get(member), Some(&text(want)), "{member}");
    }
    let named = journal
        .rows_of(ACCOUNT_STREAM)
        .into_iter()
        .filter(|row| row.event_type == "UniverseChanged")
        .filter_map(|row| {
            mandate_canon::parse(&row.body)
                .ok()?
                .get("causation_id")?
                .as_str()
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    assert_eq!(named, [THESIS], "the copy names the record it follows from");

    drop(process);
    let mut restarted = start(&journal, &broker)?;
    restarted.step(at("2026-09-21T15:02:00Z")?);
    assert_eq!(copies(&journal).len(), 1, "a restart writes no second copy");
    Ok(())
}

const EXIT: &str = "01JINTENT0000000000000000E";

fn thesis_row(journal: &support::Journal, revision: Option<i64>) -> Result<(), Error> {
    let mut members = vec![
        ("thesis_id", text("th-1")),
        ("lineage_id", text("ln-1")),
        ("instrument_id", text(INSTRUMENT)),
        ("expires_at", text("2026-09-30T00:00:00.000000000Z")),
        ("admitted", Value::Bool(true)),
        ("reason", Value::Null),
    ];
    if let Some(n) = revision {
        members.push((
            "revision",
            Value::Int(Int::new(u64::try_from(n)?).ok_or("revision")?),
        ));
    }
    journal.write(
        &agent_stream(AGENT),
        THESIS,
        "ThesisProposed",
        1,
        None,
        object(members)?,
    )
}

/// An agent holding AAPL from its opening, ready, with its protection resting at the broker.
fn holding() -> Result<
    (
        support::Journal,
        Paper,
        mandate_executord::ExecutorProcess<support::Journal, Paper>,
    ),
    Error,
> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, support::OPENED)?;
    let broker = Paper::new(&journal);
    broker.resting(support::OPENING, "AAPL", "10", "150")?;
    broker.fill(support::OPENING, "150")?;
    broker.resting_sell("md-protect", "AAPL", "10", "147")?;
    let mut process = support::start_allowing(&journal, &broker)?;
    process.step(at("2026-09-21T15:00:00Z")?);
    if !process.ready() {
        return Err("the executor is not ready".into());
    }
    Ok((journal, broker, process))
}

/// X's review, `AGENTS.md` rule 13: a thesis record the core cannot replay writes no copy and
/// never stops the process, and a risk exit proposed in the same pass goes.
#[test]
fn a_thesis_record_that_does_not_replay_never_holds_an_exit() -> Result<(), Error> {
    let (journal, broker, mut process) = holding()?;
    thesis_row(&journal, None)?;
    support::propose(&journal, AGENT, EXIT, "sell", "10", "149", "risk_exit")?;
    process.step(at("2026-09-21T15:01:00Z")?);
    assert!(
        !matches!(
            process.lifecycle().state(),
            mandate_process::State::Exited(_)
        ),
        "{:?}",
        process.lifecycle().state()
    );
    assert!(
        copies(&journal).is_empty(),
        "no copy of a record that does not replay"
    );
    assert!(
        broker.submitted().contains(&format!("md-{EXIT}")),
        "the exit goes: {:?}",
        broker.submitted()
    );
    Ok(())
}

/// X's review, the twin of `a_mark_in_doubt_never_holds_an_exit_in_its_pass` (#2449): the copy is
/// handed after the pass's proposals, so a `UniverseChanged` whose append is in doubt never holds
/// a risk exit proposed in that pass.
#[test]
fn a_universe_copy_in_doubt_never_holds_an_exit_in_its_pass() -> Result<(), Error> {
    let (journal, broker, mut process) = holding()?;
    thesis_row(&journal, Some(1))?;
    support::propose(&journal, AGENT, EXIT, "sell", "10", "149", "risk_exit")?;
    journal.fail_next_of("UniverseChanged", &[support::Fault::Unavailable]);
    process.step(at("2026-09-21T15:01:00Z")?);
    assert!(
        broker.submitted().contains(&format!("md-{EXIT}")),
        "the exit goes in the pass whose copy is in doubt: {:?}",
        broker.submitted()
    );
    Ok(())
}
