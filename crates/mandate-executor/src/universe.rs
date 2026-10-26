//! The executor's copy of a research agent's working universe on the account stream (mandate spec
//! §2.3, §8.5, §8.6; journal spec §9.3; DEC-2823): each instrument an admitted thesis holds, and
//! each one a thesis no longer holds, journaled as `UniverseChanged` so the account stream's fold
//! holds the universe without reading the agent's stream.

use mandate_accounting::InstrumentId;
use mandate_canon::Value;
use mandate_research::{ThesisRecord, replay_theses};
use mandate_time::UtcNanos;

use crate::batch::{Batch, config_refs};
use crate::error::ExecutorError;
use crate::payload::{int, text};
use crate::types::{AgentId, EventId, ThesisRow};

/// Replays `records`, `agent`'s thesis records in stream order, at the risk clock, and journals
/// each difference from the universe the account stream folds to: first each instrument a thesis
/// held that no surviving thesis holds now, then each surviving thesis the stream does not hold
/// yet, one `UniverseChanged` each, its `causation_id` naming the record it follows from. Records
/// handed again add nothing, since the fold already holds what was written.
///
/// A record that does not replay, an instrument that is not an id, or a version that is not a
/// reference writes nothing this pass rather than fail: the copy's input is the research agent's,
/// and nothing it writes may stop the executor's exits (`AGENTS.md` rule 13).
///
/// Only an instrument a thesis holds is ever removed: one a pinned list admitted
/// (`version_applied`, no thesis) is the version's, never the research agent's. Nothing is written
/// while there is no risk clock, no confirmed mandate version for the agent (the record's `man`
/// reference), or records that do not replay; a later pass writes it, and the process's own reading
/// of the stream holds openings meanwhile (DEC-2819).
pub(crate) fn copy(
    batch: &mut Batch<'_, '_>,
    agent: &AgentId,
    records: &[ThesisRow],
) -> Result<(), ExecutorError> {
    let Some(now) = batch.view.now else {
        return Ok(());
    };
    let Some(version) = batch.ports.mandates.version(agent) else {
        return Ok(());
    };
    let at = UtcNanos::from_parts(now.secs(), 0)?;
    let rows = records.iter().map(|row| {
        Ok(ThesisRecord {
            event_type: row.event_type.as_str(),
            payload: row.payload.clone(),
        })
    });
    let Ok(replayed) = replay_theses(rows, at) else {
        return Ok(());
    };
    let held = batch.view.universes.get(agent).cloned().unwrap_or_default();
    let mut size = held.len();
    let Ok(refs) = config_refs(&[("mandate_version", Some(version.0.as_str()))]) else {
        return Ok(());
    };
    let survivors: Vec<(InstrumentId, String, String)> = replayed
        .active
        .iter()
        .filter_map(|entry| {
            Some((
                InstrumentId::new(entry.instrument.as_str()).ok()?,
                entry.thesis_id.as_str().to_owned(),
                entry.lineage_id.as_str().to_owned(),
            ))
        })
        .collect();
    for (instrument, holding) in &held {
        let (Some(thesis), Some(lineage)) = (&holding.thesis_id, &holding.lineage_id) else {
            continue;
        };
        if survivors.iter().any(|(kept, ..)| kept == instrument) {
            continue;
        }
        let Some((reason, cause)) = ended(records, thesis, lineage) else {
            continue;
        };
        size = size.saturating_sub(1);
        let pairs = changed(agent, instrument, "removed", reason, thesis, lineage, size)?;
        batch.journal_with_refs("UniverseChanged", Some(cause), refs.clone(), pairs)?;
    }
    for (instrument, thesis, lineage) in &survivors {
        let current = held.get(instrument).and_then(|h| h.thesis_id.as_ref());
        if current == Some(thesis) {
            continue;
        }
        let Some(cause) = admitting(records, thesis) else {
            continue;
        };
        if !held.contains_key(instrument) {
            size = size.saturating_add(1);
        }
        let pairs = changed(
            agent,
            instrument,
            "admitted",
            "thesis_admitted",
            thesis,
            lineage,
            size,
        )?;
        batch.journal_with_refs("UniverseChanged", Some(cause), refs.clone(), pairs)?;
    }
    Ok(())
}

/// Why `thesis` of `lineage` no longer holds its instrument, and the record that says so: a
/// `ThesisInvalidated` naming it; else a refusal that retired its lineage (mandate spec §8.6 item
/// 4); else it expired, which no record says, so the copy names the record that admitted it. `None`
/// when no record admitted it, which a replay that held it cannot be.
fn ended(records: &[ThesisRow], thesis: &str, lineage: &str) -> Option<(&'static str, EventId)> {
    let names = |row: &ThesisRow, member: &str, value: &str| {
        row.payload.get(member).and_then(Value::as_str) == Some(value)
    };
    if let Some(row) = records
        .iter()
        .find(|row| row.event_type == "ThesisInvalidated" && names(row, "thesis_id", thesis))
    {
        return Some(("thesis_invalidated", row.event_id.clone()));
    }
    if let Some(row) = records.iter().find(|row| {
        row.event_type != "ThesisInvalidated"
            && names(row, "lineage_id", lineage)
            && names(row, "reason", "lineage_retired")
    }) {
        return Some(("lineage_retired", row.event_id.clone()));
    }
    admitting(records, thesis).map(|cause| ("thesis_expired", cause))
}

/// The last record that admitted `thesis`.
fn admitting(records: &[ThesisRow], thesis: &str) -> Option<EventId> {
    records
        .iter()
        .rev()
        .find(|row| {
            row.event_type != "ThesisInvalidated"
                && row.payload.get("admitted") == Some(&Value::Bool(true))
                && row.payload.get("thesis_id").and_then(Value::as_str) == Some(thesis)
        })
        .map(|row| row.event_id.clone())
}

/// `UniverseChanged`'s members (journal spec §9.3), `risk_clock` aside, which the batch adds.
fn changed(
    agent: &AgentId,
    instrument: &InstrumentId,
    change: &str,
    reason: &str,
    thesis: &str,
    lineage: &str,
    size: usize,
) -> Result<Vec<(&'static str, Value)>, ExecutorError> {
    let size = int(
        u64::try_from(size).map_err(|_| ExecutorError::NonCanonicalPayload {
            field: "universe_size_after".to_owned(),
        })?,
    )?;
    Ok(vec![
        ("agent_id", text(agent.0.as_str())),
        ("instrument", text(instrument.as_str())),
        ("change", text(change)),
        ("reason", text(reason)),
        ("thesis_id", text(thesis)),
        ("lineage_id", text(lineage)),
        ("universe_size_after", size),
    ])
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use mandate_accounting::Config;
    use mandate_canon::{Int, Value};
    use mandate_time::UtcNanos;
    use proptest::prelude::*;

    use crate::error::ExecutorError;
    use crate::payload::object;
    use crate::ports::Ports;
    use crate::reconcile::tests::{
        Everything, Executor, SchemaIds, drafted, executor_config, fees, journal_accepts_with_refs,
    };
    use crate::state::fold;
    use crate::types::{AgentId, Effect, EventId, FoldedEvent, Input, RiskClock, Seq, ThesisRow};

    const AGENT: &str = "agent-r";
    const START: i64 = 1_790_000_000;

    fn instrument(n: usize) -> String {
        format!("00000000-0000-4000-8000-00000000001{n}")
    }

    fn row(seq: usize, event_type: &str, members: Vec<(&'static str, Value)>) -> ThesisRow {
        ThesisRow {
            event_id: EventId(format!("01JTHES{seq:019}")),
            event_type: event_type.to_owned(),
            payload: object(members).unwrap_or(Value::Null),
        }
    }

    fn text(value: &str) -> Value {
        Value::Str(value.to_owned())
    }

    fn at(secs: i64) -> String {
        UtcNanos::from_parts(secs, 0)
            .map(|at| at.to_string())
            .unwrap_or_default()
    }

    /// A `ThesisProposed`: admitted (`reason` null) or refused with `reason`.
    fn proposed(
        seq: usize,
        thesis: &str,
        lineage: &str,
        on: usize,
        expires: i64,
        reason: Option<&str>,
    ) -> ThesisRow {
        row(
            seq,
            "ThesisProposed",
            vec![
                ("thesis_id", text(thesis)),
                ("lineage_id", text(lineage)),
                ("instrument_id", text(&instrument(on))),
                ("revision", Value::Int(one())),
                ("expires_at", text(&at(expires))),
                ("admitted", Value::Bool(reason.is_none())),
                ("reason", reason.map_or(Value::Null, text)),
            ],
        )
    }

    fn one() -> Int {
        Int::new(1).unwrap_or_else(|| unreachable!("1 is an Int"))
    }

    fn invalidated(seq: usize, thesis: &str) -> ThesisRow {
        row(seq, "ThesisInvalidated", vec![("thesis_id", text(thesis))])
    }

    fn harness() -> Result<(Executor, crate::types::ExecutorConfig, Config), ExecutorError> {
        Ok((
            Executor::opened(&ports_of(&executor_config(), &fees()?))?,
            executor_config(),
            fees()?,
        ))
    }

    fn ports_of<'a>(config: &'a crate::types::ExecutorConfig, fees: &'a Config) -> Ports<'a> {
        Ports {
            ids: &SchemaIds,
            mandates: &Everything,
            instruments: &Everything,
            config,
            fees,
        }
    }

    fn universe(executor: &Executor) -> BTreeSet<String> {
        executor
            .state
            .universe(&AgentId(AGENT.to_owned()))
            .into_iter()
            .map(|i| i.as_str().to_owned())
            .collect()
    }

    fn copies(effects: &[Effect]) -> Result<Vec<(String, String, String)>, ExecutorError> {
        let mut out = Vec::new();
        for effect in effects {
            if let Effect::Journal(draft) = effect {
                journal_accepts_with_refs(draft)?;
                assert!(draft.causation_id.is_some(), "a copy names its origin");
                let member = |name: &str| {
                    draft
                        .payload
                        .get(name)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                };
                out.push((member("change"), member("reason"), member("instrument")));
            }
        }
        Ok(out)
    }

    /// DEC-2823: an admitted thesis is copied once, naming its record; handed again it adds
    /// nothing. An invalidation removes the instrument with its reason, naming the invalidation. An
    /// instrument a pinned list admitted, with no thesis, is never removed by the research replay.
    #[test]
    fn an_admission_is_copied_once_and_an_invalidation_removes_it() -> Result<(), ExecutorError> {
        let (mut executor, config, fees) = harness()?;
        let ports = ports_of(&config, &fees);
        let agent = AgentId(AGENT.to_owned());
        executor.run(Input::Tick(RiskClock::from_secs(START)), &ports)?;
        let head = executor.state.account_head().0.saturating_add(1);
        let pinned = FoldedEvent {
            stream: executor.state.account_stream(),
            seq: Seq(head),
            event_id: EventId(format!("j-{head}")),
            event_type: "UniverseChanged".to_owned(),
            causation_id: Some(EventId("01JVERS0000000000000000001".to_owned())),
            payload: object(vec![
                ("agent_id", text(AGENT)),
                ("instrument", text(&instrument(9))),
                ("change", text("admitted")),
                ("reason", text("version_applied")),
                ("thesis_id", Value::Null),
                ("lineage_id", Value::Null),
                ("universe_size_after", Value::Int(one())),
                (
                    "risk_clock",
                    crate::payload::clock(RiskClock::from_secs(START))?,
                ),
            ])?,
        };
        fold(&mut executor.state, &pinned)?;
        let mut records = vec![proposed(1, "th-1", "ln-1", 0, START + 3600, None)];
        let theses = |records: &Vec<ThesisRow>| Input::Theses {
            agent: agent.clone(),
            records: records.clone(),
        };
        let first = executor.run(theses(&records), &ports)?;
        assert_eq!(
            copies(&first)?,
            [(
                "admitted".to_owned(),
                "thesis_admitted".to_owned(),
                instrument(0)
            )]
        );
        assert!(matches!(
            first.iter().find_map(|e| match e { Effect::Journal(d) => d.causation_id.clone(), _ => None }),
            Some(id) if records.first().is_some_and(|row| row.event_id == id)
        ));
        assert_eq!(
            universe(&executor),
            BTreeSet::from([instrument(0), instrument(9)])
        );
        assert!(
            drafted(&executor.run(theses(&records), &ports)?).is_empty(),
            "once"
        );

        records.push(invalidated(2, "th-1"));
        let removed = executor.run(theses(&records), &ports)?;
        assert_eq!(
            copies(&removed)?,
            [(
                "removed".to_owned(),
                "thesis_invalidated".to_owned(),
                instrument(0)
            )]
        );
        assert_eq!(
            universe(&executor),
            BTreeSet::from([instrument(9)]),
            "the pinned one stays"
        );
        assert!(drafted(&executor.run(theses(&records), &ports)?).is_empty());
        Ok(())
    }

    /// One step of a research agent's stream, or the clock moving on.
    #[derive(Debug, Clone)]
    enum Step {
        Admit {
            on: usize,
            lineage: usize,
            lasts: i64,
        },
        Invalidate {
            which: usize,
        },
        Retire {
            lineage: usize,
        },
        Wait {
            secs: i64,
        },
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![
            (0..3usize, 0..2usize, 1..400i64).prop_map(|(on, lineage, lasts)| Step::Admit {
                on,
                lineage,
                lasts
            }),
            (0..8usize).prop_map(|which| Step::Invalidate { which }),
            (0..2usize).prop_map(|lineage| Step::Retire { lineage }),
            (1..300i64).prop_map(|secs| Step::Wait { secs }),
        ]
    }

    /// The universe §8.6 reads from the records, computed without the replay: each instrument's
    /// latest admitted thesis, while it is not invalidated, its lineage not retired, and the clock
    /// short of its expiry.
    fn oracle(
        admitted: &[(usize, String, usize, i64)],
        invalid: &BTreeSet<String>,
        retired: &BTreeSet<usize>,
        now: i64,
    ) -> BTreeSet<String> {
        let mut current: BTreeMap<usize, (String, usize, i64)> = BTreeMap::new();
        for (on, thesis, lineage, expires) in admitted {
            current.insert(*on, (thesis.clone(), *lineage, *expires));
        }
        current
            .into_iter()
            .filter(|(_, (thesis, lineage, expires))| {
                !invalid.contains(thesis) && !retired.contains(lineage) && now < *expires
            })
            .map(|(on, _)| instrument(on))
            .collect()
    }

    proptest! {
        /// DEC-2823: after every pass, the universe the account stream folds to from the
        /// executor's `UniverseChanged` copies is the universe §8.6 derives from the thesis
        /// records at the risk clock, and every copy is one the journal accepts.
        #[test]
        fn the_folded_universe_is_the_one_the_theses_derive(steps in prop::collection::vec(step(), 1..24)) {
            let (mut executor, config, fees) = harness().map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            let ports = ports_of(&config, &fees);
            let agent = AgentId(AGENT.to_owned());
            let mut now = START;
            executor.run(Input::Tick(RiskClock::from_secs(now)), &ports).map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            let mut records = Vec::new();
            let mut admitted: Vec<(usize, String, usize, i64)> = Vec::new();
            let mut invalid = BTreeSet::new();
            let mut retired = BTreeSet::new();
            for step in steps {
                let seq = records.len().saturating_add(1);
                match step {
                    Step::Admit { on, lineage, lasts } => {
                        let thesis = format!("th-{seq}");
                        records.push(proposed(seq, &thesis, &format!("ln-{lineage}"), on, now.saturating_add(lasts), None));
                        admitted.push((on, thesis, lineage, now.saturating_add(lasts)));
                    }
                    Step::Invalidate { which } => {
                        let Some((_, thesis, ..)) = which.checked_rem(admitted.len()).and_then(|index| admitted.get(index)).cloned() else { continue };
                        records.push(invalidated(seq, &thesis));
                        invalid.insert(thesis);
                    }
                    Step::Retire { lineage } => {
                        records.push(proposed(seq, &format!("th-{seq}"), &format!("ln-{lineage}"), 0, now.saturating_add(10), Some("lineage_retired")));
                        retired.insert(lineage);
                    }
                    Step::Wait { secs } => {
                        now = now.saturating_add(secs);
                        executor.run(Input::Tick(RiskClock::from_secs(now)), &ports).map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
                    }
                }
                let ran = executor
                    .run(Input::Theses { agent: agent.clone(), records: records.clone() }, &ports)
                    .map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
                copies(&ran).map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
                prop_assert_eq!(universe(&executor), oracle(&admitted, &invalid, &retired, now));
            }
        }
    }
}
