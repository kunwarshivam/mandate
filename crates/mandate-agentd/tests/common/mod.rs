//! Test doubles: a fencing journal written here from journal spec §5.1, independent of the
//! crate's own lease, a paper broker that counts what it is sent, and hosts that record their
//! calls.

#![allow(
    dead_code,
    reason = "each test binary uses a different part of the shared doubles"
)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use mandate_agentd::{Config, Epochs, Host, Status};
use mandate_journal::{AppendOutcome, Environment, StoredEvent};
use mandate_shell::Cause;
use mandate_shell::stages::JournalWriter;

pub const AGENT: &str = "agent:ws_test:agent_a";
pub const ACCOUNT: &str = "acct:ws_test:paper_a";
pub const PERIOD: u64 = 1_000;

pub fn streams() -> Vec<String> {
    vec![AGENT.to_owned(), ACCOUNT.to_owned()]
}

pub fn paper() -> Config {
    Config {
        environment: Environment::Paper,
        streams: streams(),
        period_ns: PERIOD,
    }
}

/// One stream: its writer epoch and its records, each an opaque line.
#[derive(Debug, Default)]
pub struct FakeStream {
    pub epoch: u64,
    pub rows: Vec<String>,
}

#[derive(Debug, Default)]
pub struct FakeJournal {
    pub streams: BTreeMap<String, FakeStream>,
    pub down: bool,
}

impl FakeJournal {
    pub fn epoch(&self, stream: &str) -> u64 {
        self.streams.get(stream).map_or(0, |s| s.epoch)
    }

    pub fn rows(&self, stream: &str) -> Vec<String> {
        self.streams
            .get(stream)
            .map_or_else(Vec::new, |s| s.rows.clone())
    }
}

/// A handle on a shared [`FakeJournal`], with the epoch of every append it was asked to make.
#[derive(Debug, Clone, Default)]
pub struct FakeStore {
    pub journal: Rc<RefCell<FakeJournal>>,
    pub appended_under: Rc<RefCell<Vec<u64>>>,
}

impl Epochs for FakeStore {
    fn writer_epoch(&self, stream: &str) -> Result<u64, Cause> {
        let journal = self.journal.borrow();
        if journal.down {
            return Err(Cause::Absent { what: "store down" });
        }
        Ok(journal.epoch(stream))
    }
}

impl JournalWriter for FakeStore {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause> {
        let mut journal = self.journal.borrow_mut();
        if journal.down {
            return Err(Cause::Absent { what: "store down" });
        }
        let state = journal.streams.entry(stream.to_owned()).or_default();
        state.epoch += 1;
        Ok(state.epoch)
    }

    fn read(&self, _stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        if self.journal.borrow().down {
            return Err(Cause::Absent { what: "store down" });
        }
        Ok(Vec::new())
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        self.appended_under.borrow_mut().push(writer_epoch);
        let mut journal = self.journal.borrow_mut();
        let state = journal.streams.entry(stream.to_owned()).or_default();
        if writer_epoch != state.epoch {
            return AppendOutcome::Fenced {
                current_epoch: state.epoch,
            };
        }
        if expected_head != state.rows.len() as u64 {
            return AppendOutcome::Unavailable;
        }
        for draft in drafts {
            state.rows.push(String::from_utf8_lossy(draft).into_owned());
        }
        AppendOutcome::Committed(Vec::new())
    }
}

/// A paper broker: every client order id it was sent, in order.
#[derive(Debug, Default)]
pub struct Broker {
    pub sent: Vec<String>,
}

/// Where a cycle stops as if its process died.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crash {
    Never,
    AfterJournal,
    AfterSend,
}

/// A host that journals each submission before it sends it (journal spec §5.2), keyed by a client
/// order id, and whose reconciliation looks every unanswered submission up at the broker and
/// resends only one the broker does not have.
pub struct PaperHost {
    pub journal: Rc<RefCell<FakeJournal>>,
    pub broker: Rc<RefCell<Broker>>,
    pub crash: Crash,
    pub log: Rc<RefCell<Vec<(usize, &'static str)>>>,
    pub id: usize,
    pub status: Option<Status>,
    pub cycle_fails: bool,
    pub settle_fails: bool,
    pub before_append: Option<Box<dyn FnMut()>>,
}

impl PaperHost {
    pub fn new(
        id: usize,
        journal: &Rc<RefCell<FakeJournal>>,
        broker: &Rc<RefCell<Broker>>,
        log: &Rc<RefCell<Vec<(usize, &'static str)>>>,
    ) -> Self {
        Self {
            journal: Rc::clone(journal),
            broker: Rc::clone(broker),
            crash: Crash::Never,
            log: Rc::clone(log),
            id,
            status: None,
            cycle_fails: false,
            settle_fails: false,
            before_append: None,
        }
    }

    fn record(&self, journal: &mut dyn JournalWriter, line: &str) -> Result<(), Cause> {
        let epoch = journal.take_ownership(ACCOUNT)?;
        let head = self.journal.borrow().rows(ACCOUNT).len() as u64;
        match journal.append(ACCOUNT, head, epoch, &[line.as_bytes().to_vec()]) {
            AppendOutcome::Committed(_) => Ok(()),
            other => Err(Cause::Append {
                outcome: other.name(),
            }),
        }
    }

    fn unanswered(&self) -> Vec<String> {
        let rows = self.journal.borrow().rows(ACCOUNT);
        rows.iter()
            .filter_map(|row| row.strip_prefix("submitted:"))
            .filter(|id| !rows.contains(&format!("answered:{id}")))
            .map(str::to_owned)
            .collect()
    }
}

impl Host for PaperHost {
    fn reconcile(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause> {
        self.log.borrow_mut().push((self.id, "reconcile"));
        for id in self.unanswered() {
            if !self.broker.borrow().sent.contains(&id) {
                journal.take_ownership(ACCOUNT)?;
                self.broker.borrow_mut().sent.push(id.clone());
            }
            self.record(journal, &format!("answered:{id}"))?;
        }
        Ok(())
    }

    fn cycle(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause> {
        self.log.borrow_mut().push((self.id, "cycle"));
        if self.cycle_fails {
            return Err(Cause::Absent { what: "cycle" });
        }
        let n = self
            .journal
            .borrow()
            .rows(ACCOUNT)
            .iter()
            .filter(|row| row.starts_with("submitted:"))
            .count();
        let id = format!("coid-{}", n + 1);
        if let Some(hook) = self.before_append.as_mut() {
            hook();
        }
        self.record(journal, &format!("submitted:{id}"))?;
        if self.crash == Crash::AfterJournal {
            return Err(Cause::Absent { what: "crash" });
        }
        self.broker.borrow_mut().sent.push(id.clone());
        if self.crash == Crash::AfterSend {
            return Err(Cause::Absent { what: "crash" });
        }
        self.record(journal, &format!("answered:{id}"))
    }

    fn settle(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause> {
        self.log.borrow_mut().push((self.id, "settle"));
        if let Some(status) = &self.status {
            assert_eq!(status.phase(), mandate_agentd::Phase::Draining);
        }
        if self.settle_fails {
            return Err(Cause::Absent { what: "settle" });
        }
        journal.take_ownership(ACCOUNT).map(drop)
    }
}
