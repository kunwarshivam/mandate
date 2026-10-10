//! E8-10 slice D3 (NT-8, spec §5.1, DEC-704 item 3, DEC-710 item 5): a crash after every effect of
//! a step, then a restarted dispatcher over the same journal, judged by NT-8's oracle. The oracle
//! reads the committed journal, the configured audience (the journal names recipients but not
//! their channels before NT-10), and the provider's captures; never the step's state.

use super::{
    A1, AUDIENCE, BUILD, CTL, Checked, Counter, Gate, Shared, alerts, kill_switch, now, payloads,
    provider, run_over, text, world, writer,
};
use crate::DispatchError;
use crate::step::{Config, Journal, NoticeWriter, step};
use mandate_canon::{Value, parse};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_notify::{
    AddressHandle, FixtureProvider, IdempotencyKey, NoticeId, Notification, NotifyError, Origin,
    Outcome, Provider, PushChannel, Recipient, Sent,
};
use mandate_time::UtcNanos;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::error::Error;
use std::rc::Rc;

/// The subject streams of every crash scenario: one user kill switch, seen on the control stream
/// and on `A1`, and a risk-limit alert on `A1`, so two notices and four sends.
const SUBJECTS: [&str; 2] = [CTL, A1];
/// One step over [`SUBJECTS`] makes nine effects: the issue batch, then four sends and their records.
const EFFECTS: usize = 9;

/// What a step did last before its crash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Effect {
    Append,
    Send(PushChannel),
}

/// Where one run of a step crashes: right after its `at`-th effect (an append that committed or a
/// send the provider captured, from 0), which then reports a failure as if the process died before
/// it saw the effect. `late` counts every effect tried after the crash, which must stay 0.
#[derive(Default)]
struct Fuse {
    at: Option<usize>,
    effects: usize,
    crashed: Option<Effect>,
    late: usize,
}

impl Fuse {
    fn dead(&mut self) -> bool {
        if self.crashed.is_some() {
            self.late = self.late.wrapping_add(1);
        }
        self.crashed.is_some()
    }

    fn blows(&mut self, effect: Effect) -> bool {
        let blows = self.at == Some(self.effects);
        self.effects = self.effects.wrapping_add(1);
        if blows {
            self.crashed = Some(effect);
        }
        blows
    }
}

/// The journal, crashing after the fused append: it commits, and the step sees `Ambiguous`.
struct Crashing(Shared, Rc<RefCell<Fuse>>);

impl Journal for Crashing {
    fn committed(&self, stream: &StreamId) -> Vec<StoredEvent> {
        self.0.committed(stream)
    }
    fn append_notices(
        &mut self,
        writer: &NoticeWriter,
        head: u64,
        at: UtcNanos,
        drafts: &[&[u8]],
    ) -> AppendOutcome {
        if self.1.borrow_mut().dead() {
            return AppendOutcome::Unavailable;
        }
        let outcome = self.0.append_notices(writer, head, at, drafts);
        let committed = matches!(outcome, AppendOutcome::Committed(_));
        if committed && self.1.borrow_mut().blows(Effect::Append) {
            return AppendOutcome::Ambiguous;
        }
        outcome
    }
}

/// The provider, crashing after the fused send: it is captured, and the step sees an error.
struct Flaky<'a>(&'a mut FixtureProvider, Rc<RefCell<Fuse>>);

impl Provider for Flaky<'_> {
    fn send(
        &mut self,
        origin: &Origin,
        notification: &Notification,
        address: &AddressHandle,
        key: &IdempotencyKey,
    ) -> Result<Outcome, NotifyError> {
        let crash = NotifyError::Unrepresentable { what: "crash" };
        if self.1.borrow_mut().dead() {
            return Err(crash);
        }
        let outcome = self.0.send(origin, notification, address, key)?;
        if self.1.borrow_mut().blows(Effect::Send(address.channel)) {
            return Err(crash);
        }
        Ok(outcome)
    }
}

/// A dispatcher that a test restarts. The provider and the random source outlive each crash, as
/// the outside world and the operating system's source would; each restart takes a new writer
/// epoch. `drops_in_flight` seeds the bug the oracle must catch: a restart that forgets the
/// address whose send was in flight at the crash, so that send is neither made again nor recorded.
struct Restarts {
    journal: Shared,
    fx: FixtureProvider,
    random: Counter,
    drops_in_flight: bool,
    forgotten: Vec<PushChannel>,
    crashes: usize,
    crashed_after_send: usize,
}

impl Restarts {
    fn new(drops_in_flight: bool) -> Result<Self, Box<dyn Error>> {
        let journal = world()?;
        kill_switch(&journal)?;
        alerts(&journal, A1, 1, &[(1, "kill_switch"), (2, "risk_limit")])?;
        let fx = provider(&journal)?.fx;
        Ok(Self {
            journal,
            fx,
            random: Counter(0),
            drops_in_flight,
            forgotten: Vec::new(),
            crashes: 0,
            crashed_after_send: 0,
        })
    }

    /// One step by a newly started dispatcher, crashing after its `at`-th effect if it gets there.
    fn restart(&mut self, at: Option<usize>) -> Result<Result<(), DispatchError>, Box<dyn Error>> {
        let (writer, fuse) = (
            writer(&self.journal)?,
            Rc::new(RefCell::new(Fuse::default())),
        );
        fuse.borrow_mut().at = at;
        let audience: Vec<(&str, PushChannel)> = AUDIENCE
            .into_iter()
            .filter(|(_, channel)| !self.forgotten.contains(channel))
            .collect();
        let subjects: Vec<StreamId> = SUBJECTS.iter().filter_map(|s| StreamId::parse(s)).collect();
        let origin = Origin::parse("https://app.example.invalid")?;
        let config = Config {
            writer: &writer,
            subjects: &subjects,
            audience: &audience,
            origin: &origin,
            environment: Environment::Paper,
            build: BUILD,
        };
        let mut journal = Crashing(self.journal.clone(), Rc::clone(&fuse));
        let mut sends = Flaky(&mut self.fx, Rc::clone(&fuse));
        let stepped = step(&config, &mut journal, &mut self.random, &mut sends, now()?);
        let fuse = fuse.borrow();
        assert_eq!(fuse.late, 0, "no effect after the crash at {at:?}");
        assert_eq!(
            fuse.crashed.is_some(),
            stepped.is_err(),
            "{at:?}: {stepped:?}"
        );
        self.crashes = self
            .crashes
            .wrapping_add(usize::from(fuse.crashed.is_some()));
        if let Some(Effect::Send(channel)) = fuse.crashed {
            self.crashed_after_send = self.crashed_after_send.wrapping_add(1);
            if self.drops_in_flight {
                self.forgotten.push(channel);
            }
        }
        Ok(stepped)
    }

    /// A crash at each of `plan`'s points, one restart each, then a restart that runs to the end.
    fn run(&mut self, plan: &[usize]) -> Checked {
        for &at in plan {
            let _crashed_or_done_early = self.restart(Some(at))?;
        }
        self.restart(None)??;
        Ok(())
    }
}

/// The notice key of a committed `OwnerAlertSent`: its owner command, or its own event id.
fn key(row: &StoredEvent) -> Option<String> {
    let payload = parse(&row.body).ok()?.get("payload")?.clone();
    let command = payload.get("owner_command").and_then(Value::as_str);
    Some(command.map_or_else(|| row.event_id.clone(), str::to_owned))
}

/// NT-8's oracle. From the committed journal alone it derives the notice key of every
/// `OwnerAlertSent` on `subjects`, and the sends due: `(notice, recipient, channel)` for each
/// recipient a `NoticeIssued` lists and each of that recipient's configured channels. It finds one
/// `NoticeIssued` per key, exactly one `NoticeAttempted` per due send and none other, and every
/// captured send a due send under that send's own idempotency key, with every due send captured.
fn nt8(journal: &Shared, subjects: &[&str], sends: &[Sent]) -> Result<(), Box<dyn Error>> {
    let j = journal.0.borrow();
    let mut keys = BTreeSet::new();
    for subject in subjects {
        let rows = j.rows(&StreamId::parse(subject).ok_or("subject")?);
        for row in rows.iter().filter(|r| r.event_type == "OwnerAlertSent") {
            keys.insert(key(row).ok_or("alert payload")?);
        }
    }
    let issued = payloads(journal, "NoticeIssued");
    let mut issued_keys = Vec::new();
    for p in &issued {
        let cause = j.event(&text(p, "cause")).ok_or("an uncommitted cause")?;
        issued_keys.push(key(cause).ok_or("cause payload")?);
    }
    issued_keys.sort();
    let keys: Vec<String> = keys.into_iter().collect();
    if issued_keys != keys {
        return Err(format!("one notice per key: {issued_keys:?} for {keys:?}").into());
    }
    let mut due = Vec::new();
    for p in &issued {
        let notice = NoticeId::parse(&text(p, "notice"))?;
        let listed = p.get("recipients").and_then(Value::as_array);
        for recipient in listed.unwrap_or_default().iter().filter_map(Value::as_str) {
            for &(_, channel) in AUDIENCE.iter().filter(|(r, _)| r == &recipient) {
                let address = AddressHandle {
                    recipient: Recipient::parse(recipient)?,
                    channel,
                };
                let key = IdempotencyKey::of(&notice, &address)?.hex()?.to_owned();
                let send = (
                    notice.hex()?,
                    recipient.to_owned(),
                    channel.key()?.to_owned(),
                );
                due.push((send, address, key));
            }
        }
    }
    let mut want: Vec<_> = due.iter().map(|(send, _, _)| send.clone()).collect();
    want.sort();
    let attempts = payloads(journal, "NoticeAttempted");
    let triple = |p: &Value| ["notice", "recipient", "channel"].map(|m| text(p, m));
    let mut got: Vec<_> = attempts
        .iter()
        .map(triple)
        .map(|[n, r, c]| (n, r, c))
        .collect();
    got.sort();
    if got != want {
        return Err(format!("one NoticeAttempted per due send: {got:?} for {want:?}").into());
    }
    let mut made = BTreeSet::new();
    for sent in sends {
        let ours = |(send, address, key): &&((String, String, String), AddressHandle, String)| {
            *address == sent.address && *key == sent.key && sent.message.ends_with(&send.0)
        };
        let (send, _, _) = due
            .iter()
            .find(ours)
            .ok_or("a send no NoticeIssued lists")?;
        made.insert(send.clone());
    }
    if made.len() != want.len() {
        return Err(format!("every due send is made: {made:?} for {want:?}").into());
    }
    Ok(())
}

#[test]
fn after_one_or_two_crashes_at_any_effect_a_restart_makes_and_records_every_due_send() -> Checked {
    let mut plans: Vec<Vec<usize>> = (0..=EFFECTS).map(|at| vec![at]).collect();
    for first in 0..EFFECTS {
        plans.extend((0..EFFECTS).map(|second| vec![first, second]));
    }
    for plan in plans {
        let mut dispatcher = Restarts::new(false)?;
        dispatcher.run(&plan)?;
        let first_crashes = plan.first().is_some_and(|&at| at < EFFECTS);
        assert!(
            dispatcher.crashes >= usize::from(first_crashes),
            "{plan:?} crashed"
        );
        let sends = dispatcher.fx.sent()?;
        nt8(&dispatcher.journal, &SUBJECTS, sends).map_err(|e| format!("{plan:?}: {e}"))?;
        let resent = sends.len().checked_sub(4);
        assert_eq!(
            resent,
            Some(dispatcher.crashed_after_send),
            "{plan:?}: a send is made again only when a crash fell between it and its record"
        );
    }
    Ok(())
}

#[test]
fn a_send_whose_record_does_not_commit_is_made_again_under_its_key_and_recorded_once() -> Checked {
    for refused in [AppendOutcome::Unavailable, AppendOutcome::Ambiguous] {
        let journal = world()?;
        alerts(&journal, A1, 1, &[(1, "risk_limit")])?;
        let mut sends = provider(&journal)?;
        let mut gate = Gate {
            journal: journal.clone(),
            refuse: Some((1, refused.clone())),
            batches: Vec::new(),
        };
        let first = run_over(&mut gate, &writer(&journal)?, &[A1], &AUDIENCE, &mut sends)?;
        let name = refused.name();
        assert_eq!(first, Err(DispatchError::NotCommitted { outcome: name }));
        run_over(&mut gate, &writer(&journal)?, &[A1], &AUDIENCE, &mut sends)??;
        let sent = sends.fx.sent()?;
        let made: Vec<(&AddressHandle, &str)> =
            sent.iter().map(|s| (&s.address, s.key.as_str())).collect();
        let [once, again, other] = made.as_slice() else {
            return Err(format!("{name}: three sends, not {}", made.len()).into());
        };
        assert_eq!(once, again, "{name}: the same address under the same key");
        assert_ne!(once.1, other.1, "{name}: each address has its own key");
        nt8(&journal, &[A1], sent).map_err(|e| format!("{name}: {e}"))?;
        let attempts = payloads(&journal, "NoticeAttempted");
        let firsts = attempts
            .iter()
            .filter(|p| p.get("attempt").and_then(Value::as_int) == Some(1));
        assert_eq!(
            firsts.count(),
            2,
            "{name}: each send recorded once, as attempt 1"
        );
    }
    Ok(())
}

#[test]
fn the_oracle_catches_a_restart_that_drops_the_send_in_flight_at_a_crash() -> Checked {
    let mut caught = 0usize;
    for at in 0..=EFFECTS {
        let mut dispatcher = Restarts::new(true)?;
        dispatcher.run(&[at])?;
        let verdict = nt8(&dispatcher.journal, &SUBJECTS, dispatcher.fx.sent()?);
        let dropped = !dispatcher.forgotten.is_empty();
        assert_eq!(verdict.is_err(), dropped, "crash at {at}: {verdict:?}");
        caught = caught.wrapping_add(usize::from(dropped));
    }
    assert_eq!(
        caught, 4,
        "every crash between a send and its record drops it under the bug"
    );
    Ok(())
}
