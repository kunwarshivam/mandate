//! E8-10 slice D10 (DEC-2500 items 3, 4 and 6; DEC-2516): the dispatcher as a process. It follows
//! a subject stream only its control stream names, holds its replay until the journal answers, is
//! fenced by a newer dispatcher, settles a drain, and maps each step's result to its lifecycle. The
//! oracles are the notice stream's rows and the lifecycle's state, never the process's own view.

use super::{A1, AUDIENCE, CTL, Checked, Counter, Shared, alerts, id, now, payloads};
use crate::DispatchError;
use crate::process::{Dispatcher, Settings, outcome};
use crate::step::{Journal, NoticeWriter};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_notify::{FixtureProvider, Origin};
use mandate_process::{Exit, Input, State};
use mandate_time::UtcNanos;
use std::cell::Cell;
use std::error::Error;
use std::rc::Rc;

const BUILD: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";

fn settings(floor: &[&str]) -> Result<Settings, Box<dyn Error>> {
    Ok(Settings {
        workspace: "ws_1".to_owned(),
        floor: floor.iter().filter_map(|s| StreamId::parse(s)).collect(),
        audience: AUDIENCE
            .iter()
            .map(|(recipient, channel)| ((*recipient).to_owned(), *channel))
            .collect(),
        origin: Origin::parse("https://app.example.invalid")?,
        environment: Environment::Paper,
        build: BUILD.to_owned(),
    })
}

/// The shared journal, serving `control` as the control stream's committed rows, as the
/// dispatcher only reads it, and failing every read while `down` is set.
#[derive(Clone)]
struct Laid {
    journal: Shared,
    control: Vec<StoredEvent>,
    down: Rc<Cell<bool>>,
}

impl Journal for Laid {
    fn committed(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, DispatchError> {
        if self.down.get() {
            return Err(DispatchError::JournalUnavailable);
        }
        if stream.as_str() == CTL {
            return Ok(self.control.clone());
        }
        self.journal.committed(stream)
    }
    fn append_notices(
        &mut self,
        writer: &NoticeWriter,
        head: u64,
        at: UtcNanos,
        drafts: &[&[u8]],
    ) -> AppendOutcome {
        self.journal.append_notices(writer, head, at, drafts)
    }
}

/// A control stream whose one row names `A1`'s connection, so only the control stream names `A1`.
fn laid(journal: &Shared) -> Laid {
    let body = r#"{"event_type":"ConnectionEstablished","environment":"paper","payload":{"connection_id":"c1","account_ref":"A1"}}"#;
    let row = StoredEvent {
        stream_id: CTL.to_owned(),
        seq: 1,
        event_id: id(7, 1),
        event_type: "ConnectionEstablished".to_owned(),
        schema_version: 2,
        environment: "paper".to_owned(),
        recorded_at: super::T.to_owned(),
        prev_hash: mandate_canon::Digest::of(b"prev"),
        hash: mandate_canon::Digest::of(body.as_bytes()),
        body: body.as_bytes().to_vec(),
    };
    Laid {
        journal: journal.clone(),
        control: vec![row],
        down: Rc::new(Cell::new(false)),
    }
}

fn provider() -> Result<FixtureProvider, Box<dyn Error>> {
    let mut vault = Vec::new();
    for &(recipient, channel) in &AUDIENCE {
        let recipient = mandate_notify::Recipient::parse(recipient)?;
        vault.push((
            mandate_notify::AddressHandle { recipient, channel },
            "address".to_owned(),
        ));
    }
    Ok(FixtureProvider::holding(vault)?)
}

fn writer(journal: &Shared) -> Result<NoticeWriter, Box<dyn Error>> {
    Ok(super::writer(journal)?)
}

#[test]
fn a_stream_only_the_control_stream_names_is_followed_and_its_alerts_issued() -> Checked {
    let journal = super::world()?;
    alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
    let mut process = Dispatcher::start(
        laid(&journal),
        provider()?,
        Counter(0),
        writer(&journal)?,
        settings(&[])?,
        now()?,
    );
    process.step(now()?);
    assert_eq!(process.lifecycle().state(), State::Ready);
    let issued: Vec<String> = payloads(&journal, "NoticeIssued")
        .iter()
        .map(|p| super::text(p, "cause"))
        .collect();
    assert_eq!(
        issued,
        [id(1, 1)],
        "the alert on A1, found through the control stream"
    );
    Ok(())
}

#[test]
fn an_unread_journal_holds_the_replay_and_sends_no_heartbeat() -> Checked {
    let journal = super::world()?;
    alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
    let flaky = laid(&journal);
    let down = Rc::clone(&flaky.down);
    down.set(true);
    let mut process = Dispatcher::start(
        flaky,
        provider()?,
        Counter(0),
        writer(&journal)?,
        settings(&[])?,
        now()?,
    );
    process.step(now()?);
    assert_ne!(process.lifecycle().state(), State::Ready);
    assert!(payloads(&journal, "NoticeIssued").is_empty());
    down.set(false);
    process.step(now()?);
    assert_eq!(process.lifecycle().state(), State::Ready);
    assert_eq!(payloads(&journal, "NoticeIssued").len(), 1);
    Ok(())
}

#[test]
fn a_newer_dispatcher_fences_this_one_and_a_drain_settles() -> Checked {
    let journal = super::world()?;
    let mut old = Dispatcher::start(
        laid(&journal),
        provider()?,
        Counter(0),
        writer(&journal)?,
        settings(&[])?,
        now()?,
    );
    old.step(now()?);
    let mut newer = Dispatcher::start(
        laid(&journal),
        provider()?,
        Counter(50),
        writer(&journal)?,
        settings(&[])?,
        now()?,
    );
    alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
    old.step(now()?);
    assert_eq!(old.lifecycle().state(), State::Exited(Exit::Fenced));
    assert!(
        payloads(&journal, "NoticeIssued").is_empty(),
        "the fenced one issued nothing"
    );
    newer.step(now()?);
    assert_eq!(payloads(&journal, "NoticeIssued").len(), 1);
    newer.drain(now()?);
    newer.step(now()?);
    assert_eq!(newer.lifecycle().state(), State::Exited(Exit::Drained));
    Ok(())
}

/// DEC-2500 item 4: what each step result does to the lifecycle, written out per result.
#[test]
fn each_step_result_has_its_lifecycle_input() {
    let cases: [(Result<(), DispatchError>, Option<Input>); 9] = [
        (Ok(()), Some(Input::Heartbeat)),
        (Err(DispatchError::JournalUnavailable), None),
        (
            Err(DispatchError::Fenced { current_epoch: 2 }),
            Some(Input::Fenced),
        ),
        (
            Err(DispatchError::NotCommitted {
                outcome: "Unavailable",
            }),
            None,
        ),
        (
            Err(DispatchError::NotCommitted {
                outcome: "Ambiguous",
            }),
            None,
        ),
        (
            Err(DispatchError::NotCommitted {
                outcome: "HeadMismatch",
            }),
            None,
        ),
        (
            Err(DispatchError::NotCommitted { outcome: "Invalid" }),
            Some(Input::Failed),
        ),
        (
            Err(DispatchError::NotCommitted {
                outcome: "IdempotencyConflict",
            }),
            Some(Input::Failed),
        ),
        (Err(DispatchError::NoMessageId), Some(Input::Failed)),
    ];
    for (ran, want) in cases {
        assert_eq!(outcome(&ran), want, "{ran:?}");
    }
}
