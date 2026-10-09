//! `AGENTS.md` rule 13 for the host's observation (E15-13, the brief's slice H3): an observation
//! whose data is not in the run's store is refused before the runtime is handed it, so no batch
//! goes into doubt and the kill switch still steps in the same session. The journal refuses an
//! observation whose data is not in the same store, as an artifact-aware append would, so a shell
//! that left the check to the append would put the runtime in doubt and the kill switch here would
//! be refused.

use std::collections::BTreeMap;
use std::sync::Arc;

use mandate_canon::{Digest, Value};
use mandate_journal::{AppendOutcome, ArtifactRef, ArtifactSource, StoredEvent, get_artifact};
use mandate_runtime::{AgentId, Command, Initiator, Input, KillScope, Observation, RiskClock};

use super::doubles::{FixtureMandate, LedgerJournal, World, agent_stream, instrument, setup};
use super::{JournalWriter, MandateSource};
use crate::error::{Cause, ShellError};
use crate::tracer::Session;

const CLOSES: &[u8] = br#"{"closes":[["2026-09-25","255.2"]]}"#;

type Store = Option<Arc<dyn ArtifactSource + Send + Sync>>;
type Answer = Result<(), ShellError>;

/// The harness ledger behind an append that refuses an `ObservationRecorded` whose `data_ref` is
/// not in `store` or does not re-hash there, answering `Unavailable` as a store-checking append
/// would.
struct StoreChecked(LedgerJournal, Store);

impl JournalWriter for StoreChecked {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause> {
        self.0.take_ownership(stream)
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        self.0.read(stream)
    }

    fn append(&mut self, stream: &str, head: u64, epoch: u64, drafts: &[Vec<u8>]) -> AppendOutcome {
        let unstored = drafts.iter().any(|draft| {
            let Ok(body) = mandate_canon::parse(draft) else {
                return false;
            };
            let observed =
                body.get("event_type").and_then(Value::as_str) == Some("ObservationRecorded");
            let data = body
                .get("payload")
                .and_then(|p| p.get("data_ref"))
                .and_then(Value::as_str);
            let found = data
                .and_then(ArtifactRef::parse)
                .zip(self.1.as_deref())
                .is_some_and(|(data, store)| get_artifact(store, &data).is_ok());
            observed && !found
        });
        if unstored {
            return AppendOutcome::Unavailable;
        }
        self.0.append(stream, head, epoch, drafts)
    }
}

/// The agent stream's event types after `observation` is offered over `store` in one started
/// session, then the agent's kill switch, with what each answered.
fn offer(
    store: Option<BTreeMap<Digest, Vec<u8>>>,
) -> Result<(Answer, Answer, Vec<String>), String> {
    let world = World::default();
    let setup = setup()?;
    let mut stages = world.stages();
    let store: Store = match store {
        Some(store) => Some(Arc::new(store)),
        None => None,
    };
    stages.journal = Box::new(StoreChecked(LedgerJournal(world.clone()), store.clone()));
    stages.artifacts = store;
    let mandate = FixtureMandate {
        world: world.clone(),
        environment: mandate_journal::Environment::Paper,
    };
    let admitted = mandate.admitted().map_err(|e| e.to_string())?;
    let observation = Observation {
        source: "alpaca_iex_daily_bars".to_owned(),
        instrument_id: Some(instrument().map_err(|e| e.to_string())?),
        as_of: RiskClock::from_secs(setup.now.secs()),
        data_ref: Digest::of(CLOSES),
    };
    let mut session = Session::open_governed(&mut stages, &setup, &admitted.view, None)
        .map_err(|e| e.to_string())?;
    session.start().map_err(|e| e.to_string())?;
    let observed = session.observe(observation);
    let killed = session.feed(Input::Command(Command::KillSwitch {
        scope: KillScope::Agent(AgentId("tracer-aapl".to_owned())),
        initiator: Initiator::Owner,
        confirmation: None,
    }));
    drop(session);
    let rows = stages
        .journal
        .read(&agent_stream())
        .map_err(|e| e.to_string())?;
    Ok((
        observed,
        killed,
        rows.into_iter().map(|row| row.event_type).collect(),
    ))
}

/// The kill switch was stepped: the runtime journaled `KillSwitchActivated` once, and the run then
/// stopped at the harness sink, which hands no flatten (`ConvertingSink`), never at a runtime
/// still holding a batch in doubt.
fn assert_killed(name: &str, killed: &Result<(), ShellError>, kinds: &[String]) {
    let code = killed.as_ref().err().map(ShellError::code);
    assert_eq!(code, Some("intent_not_handed"), "{name}: {killed:?}");
    let activated = kinds.iter().filter(|k| *k == "KillSwitchActivated").count();
    assert_eq!(activated, 1, "{name}: {kinds:?}");
}

/// The observation stored: journaled, and the kill switch after it stepped. Not stored, no store,
/// or other bytes under its digest: refused as `market_data_untrusted` with nothing journaled for
/// it, and the kill switch in the same session still stepped and journaled.
#[test]
#[ignore = "pending E15-13"]
fn a_refused_observation_leaves_the_kill_switch_working() -> Result<(), String> {
    let stored = BTreeMap::from([(Digest::of(CLOSES), CLOSES.to_vec())]);
    let (observed, killed, kinds) = offer(Some(stored))?;
    observed.map_err(|e| format!("the stored observation: {e:?}"))?;
    let observations = kinds.iter().filter(|k| *k == "ObservationRecorded").count();
    assert_eq!(observations, 1, "{kinds:?}");
    assert_killed("stored", &killed, &kinds);
    let other = BTreeMap::from([(Digest::of(CLOSES), b"other".to_vec())]);
    let cases = [
        ("none", None),
        ("empty", Some(BTreeMap::new())),
        ("other", Some(other)),
    ];
    for (name, store) in cases {
        let (observed, killed, kinds) = offer(store)?;
        let code = observed.as_ref().err().map(ShellError::code);
        assert_eq!(code, Some("market_data_untrusted"), "{name}: {observed:?}");
        let observations = kinds.iter().filter(|k| *k == "ObservationRecorded").count();
        assert_eq!(observations, 0, "{name}: {kinds:?}");
        assert_killed(name, &killed, &kinds);
    }
    Ok(())
}
