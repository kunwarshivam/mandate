//! J0 (E7-19 slice 4's remainder, DEC-510): `PgJournal::append_with_config_artifacts` gives the
//! outcome `MemoryJournal::append_with_config_artifacts` gives for the same streams, drafts, and
//! artifacts (journal spec §5.1, §11 check 6), and a refusal writes nothing. Each test also states
//! its outcomes on its own terms: the reference cases, §5.1's order, or how a step was built.
//!
//! The pending gate runs without a database (DEC-109 item 7), so every test starts with the one
//! property that needs none, [`unreachable_database_property`], which reaches the stub by itself.

#[path = "../../mandate-journal/tests/common/mod.rs"]
mod common;
#[path = "../../mandate-journal/tests/conformance/mod.rs"]
#[allow(
    unused_imports,
    unused_macros,
    reason = "only the fixture helpers are used here; the suite itself runs in `pg_conformance.rs`"
)]
mod conformance;
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use common::edit;
use conformance::{fixture, fixture_artifacts, get, list, text};
use mandate_canon::{Digest, parse, to_canonical};
use mandate_journal::AppendOutcome::{self, Committed, Fenced, HeadMismatch, Unavailable};
use mandate_journal::{
    ArtifactError, ArtifactRef, ArtifactSource, Head, Invalid, InvalidReason, MemoryJournal,
    StoredEvent, StreamId,
};
use mandate_journal_pg::PgJournal;
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use support::TestDb;

type Artifacts = BTreeMap<Digest, Vec<u8>>;
type Source<'a> = &'a (dyn ArtifactSource + Sync);
type State = Vec<(Head, Vec<StoredEvent>)>;

const STREAMS: [&str; 2] = ["agent:ws_01J8Z2:agent_a", "ctl:ws_01J8Z2"];
const POLICY: &str = "config_refs.policy_set";
const REGISTRY: &str = "config_refs.model_registry";
const CONTENT: &str = "payload.content_hash";

/// The reference cases' version-2 drafts and the paths of the objects an append must read for
/// each: journal spec §9's append-time check (DEC-484), written out here, not taken from a journal.
const CHECKED: [(&str, &[&str]); 4] = [
    ("model_output", &[REGISTRY]),
    ("decision", &[POLICY, REGISTRY]),
    ("model_registry_registration", &[CONTENT]),
    ("policy_registration", &[CONTENT]),
];

/// An artifact store that cannot be read.
struct Down;

impl ArtifactSource for Down {
    fn read_artifact(&self, _: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        Err(ArtifactError::Unavailable)
    }
}

fn at() -> UtcNanos {
    UtcNanos::parse("2026-09-21T14:00:03.000000000Z").unwrap()
}

fn draft(name: &str) -> Vec<u8> {
    to_canonical(get(
        &fixture(),
        &format!("production_config_refs.valid_drafts.{name}"),
    ))
}

/// The reference cases' stored objects: the policy set and the model registry.
fn stored() -> Artifacts {
    fixture_artifacts(get(&fixture(), "production_config_refs"))
}

fn member(draft: &[u8], path: &str) -> String {
    text(&parse(draft).unwrap(), path).to_owned()
}

fn stream_of(draft: &[u8]) -> StreamId {
    StreamId::parse(&member(draft, "stream_id")).unwrap()
}

fn reference(draft: &[u8], path: &str) -> Digest {
    ArtifactRef::parse(&member(draft, path)).unwrap().digest()
}

fn without(gone: Digest) -> Artifacts {
    let mut kept = stored();
    assert!(kept.remove(&gone).is_some(), "{gone} is stored");
    kept
}

/// The stored objects with the one under `digest` changed, so it no longer hashes to its reference.
fn altered(digest: Digest) -> Artifacts {
    let mut changed = stored();
    changed.get_mut(&digest).unwrap().push(b' ');
    changed
}

fn missing(draft: usize, path: &str) -> AppendOutcome {
    let (reason, path) = (InvalidReason::MissingArtifact, path.to_owned());
    AppendOutcome::Invalid {
        draft,
        error: Invalid { reason, path },
    }
}

/// With its database unreachable, an artifact-aware append answers `Unavailable` even for a draft
/// none of whose objects is stored: §5.1 decides idempotency first, and only the database can, so a
/// draft that may already be committed is never refused for its artifacts.
fn unreachable_database_property() {
    let decision = draft("decision");
    let outcome = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let pool = PgPoolOptions::new()
            .acquire_timeout(Duration::from_secs(2))
            .connect_lazy_with(PgConnectOptions::new().host("127.0.0.1").port(1));
        let (journal, s, none) = (PgJournal::new(pool), stream_of(&decision), Artifacts::new());
        let drafts: [&[u8]; 1] = [&decision];
        journal
            .append_with_config_artifacts(&s, 1, 1, at(), &drafts, &none)
            .await
    });
    assert_eq!(outcome.unwrap(), Unavailable);
}

/// The Postgres journal under test and `MemoryJournal`, its oracle, holding the same streams.
struct Both {
    db: TestDb,
    pg: PgJournal,
    memory: MemoryJournal,
}

impl Both {
    /// After [`unreachable_database_property`], both journals with the fixture's agent and control
    /// streams opened by their first owner; `None` where `TestDb` skips.
    fn new() -> Option<Self> {
        unreachable_database_property();
        Some(Self::opened(TestDb::new()?))
    }

    fn opened(db: TestDb) -> Self {
        let (pg, mut memory) = (db.journal(), MemoryJournal::new());
        for name in ["agent_stream", "control_stream"] {
            let body = to_canonical(get(&list(&fixture(), &format!("{name}.chain"))[0], "body"));
            let fields = ["seq", "prev_hash", "recorded_at"];
            let opening = fields.iter().fold(body, |d, field| edit(&d, field, None));
            let s = stream_of(&opening);
            let epoch = memory.take_ownership(&s);
            assert_eq!(db.block_on(pg.take_ownership(&s)).unwrap(), epoch);
            let opened = memory.append(&s, 0, epoch, at(), &[&opening]);
            assert_eq!(opened.name(), "Committed");
            let got = db.block_on(pg.append(&s, 0, epoch, at(), &[&opening]));
            assert_eq!(got, Ok(opened));
        }
        Self { db, pg, memory }
    }

    /// Every stream's head and rows, in Postgres and in the memory journal.
    fn states(&self) -> (State, State) {
        let streams = STREAMS.map(|s| StreamId::parse(s).unwrap());
        let pg = streams.iter().map(|s| {
            let head = self.db.block_on(self.pg.head(s)).unwrap();
            (head, self.db.block_on(self.pg.rows(s)).unwrap())
        });
        let memory = |s: &StreamId| (self.memory.head(s), self.memory.rows(s).to_vec());
        (pg.collect(), streams.iter().map(memory).collect())
    }

    /// One artifact-aware append on both journals: the outcome, once Postgres answered as the
    /// memory journal did and holds the same heads and rows.
    fn append(&mut self, head: u64, epoch: u64, d: &[&[u8]], src: Source) -> AppendOutcome {
        let s = stream_of(d[0]);
        let (pg, memory) = (&self.pg, &mut self.memory);
        let got = self
            .db
            .block_on(pg.append_with_config_artifacts(&s, head, epoch, at(), d, src));
        let want = memory.append_with_config_artifacts(&s, head, epoch, at(), d, src);
        let got = got.unwrap();
        assert_eq!(got, want, "Postgres answers as the memory journal");
        let (pg, memory) = self.states();
        assert_eq!(pg, memory);
        want
    }

    /// [`Both::append`] for a batch that must not be written: every head and row stays as it was.
    fn refused(&mut self, head: u64, epoch: u64, d: &[&[u8]], src: Source) -> AppendOutcome {
        let before = self.states().0;
        let outcome = self.append(head, epoch, d, src);
        assert_eq!(self.states().0, before, "{outcome:?} wrote nothing");
        outcome
    }
}

#[test]
#[ignore = "pending E7-19"]
fn an_unreachable_database_is_unavailable_whatever_the_artifacts() {
    unreachable_database_property();
}

#[test]
#[ignore = "pending E7-19"]
fn a_missing_or_changed_object_is_refused_until_it_is_stored() {
    let Some(mut both) = Both::new() else { return };
    for (name, paths) in CHECKED {
        let d = draft(name);
        for path in paths {
            let gone = both.refused(1, 1, &[&d], &without(reference(&d, path)));
            assert_eq!(gone, missing(0, path), "{name} without {path}");
            let changed = both.refused(1, 1, &[&d], &altered(reference(&d, path)));
            assert_eq!(changed, Unavailable, "{name}, {path} changed");
        }
        let down = both.refused(1, 1, &[&d], &Down);
        assert_eq!(down, Unavailable, "{name}, an unreadable store");
    }
    let (output, decision) = (draft("model_output"), draft("decision"));
    let batch: [&[u8]; 2] = [&output, &decision];
    let gone = without(reference(&decision, POLICY));
    let refusal = both.refused(1, 1, &batch, &gone);
    assert_eq!(refusal, missing(1, POLICY), "a batch refused at its second");
    let Committed(rows) = both.append(1, 1, &batch, &stored()) else {
        panic!("the batch with its objects stored did not commit")
    };
    let seqs: Vec<u64> = rows.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, [2, 3], "the refusals used no seq");
    for (head, (name, _)) in (1..).zip(&CHECKED[2..]) {
        let outcome = both.append(head, 1, &[&draft(name)], &stored());
        assert_eq!(outcome.name(), "Committed", "{name} with its object stored");
    }
}

#[test]
#[ignore = "pending E7-19"]
fn every_invalid_reference_is_refused_as_the_reference_cases_say() {
    let Some(mut both) = Both::new() else { return };
    let fx = fixture();
    let cases = list(&fx, "production_config_refs.invalid_drafts");
    assert!(!cases.is_empty(), "the reference cases hold invalid drafts");
    for case in cases {
        let name = text(case, "name");
        let mut d = draft(text(case, "base"));
        for change in list(case, "changes") {
            let value = change.get("value").map(to_canonical);
            let value = value.map(|v| String::from_utf8(v).unwrap());
            d = edit(&d, text(change, "path"), value.as_deref());
        }
        let AppendOutcome::Invalid { draft, error } = both.refused(1, 1, &[&d], &stored()) else {
            panic!("{name} was not refused as Invalid")
        };
        let got = (draft, error.reason.code(), error.path.as_str());
        let want = (0, text(case, "expect.reason"), text(case, "expect.path"));
        assert_eq!(got, want, "{name}");
    }
}

/// Journal spec §5.1's order: idempotency, then the objects, then fencing and the head check.
#[test]
#[ignore = "pending E7-19"]
fn the_object_check_follows_idempotency_and_precedes_fencing_and_the_head_check() {
    let Some(mut both) = Both::new() else { return };
    let (d, all, none) = (draft("decision"), stored(), Artifacts::new());
    let gone = without(reference(&d, POLICY));
    let fenced = both.refused(1, 2, &[&d], &gone);
    assert_eq!(fenced, missing(0, POLICY), "another epoch");
    let stale = both.refused(0, 1, &[&d], &gone);
    assert_eq!(stale, missing(0, POLICY), "a stale head");
    let fenced = both.refused(1, 2, &[&d], &all);
    assert_eq!(fenced, Fenced { current_epoch: 1 });
    let stale = both.refused(0, 1, &[&d], &all);
    assert!(matches!(stale, HeadMismatch { actual_seq: 1, .. }));
    let Committed(rows) = both.append(1, 1, &[&d], &all) else {
        panic!("the decision with its objects stored did not commit")
    };
    let retry = AppendOutcome::AlreadyCommitted(rows);
    let sources: [(Source, &str); 3] = [(&all, "stored"), (&none, "none"), (&Down, "down")];
    for (source, label) in sources {
        let outcome = both.append(1, 7, &[&d], source);
        assert_eq!(outcome, retry, "a retry with objects {label}");
    }
    let (s, again) = (stream_of(&d), [d.as_slice()]);
    let plain = both.pg.append(&s, 1, 1, at(), &again);
    assert_eq!(both.db.block_on(plain), Ok(retry), "a plain retry (#644)");
    let resent = edit(&d, "payload.qty", Some("\"11\""));
    let conflict = both.append(2, 1, &[&resent], &none);
    assert_eq!(
        conflict,
        AppendOutcome::IdempotencyConflict { stored_seq: 2 }
    );
}

/// What a property step does to the objects its append reads, by index in digest order.
#[derive(Debug, Clone)]
enum Damage {
    Intact,
    Drop(usize),
    Corrupt(usize),
    Down,
}

/// Random sessions of the four drafts, with objects dropped, changed, or unreadable and the epoch
/// sometimes another, give the memory journal's outcomes and the outcome each step was built to
/// have, from this test's own record of what committed.
#[test]
#[ignore = "pending E7-19"]
fn random_sessions_match_the_memory_journal_and_their_construction() {
    let Some(both) = Both::new() else { return };
    drop(both);
    let refs: Vec<Digest> = stored().into_keys().collect();
    assert_eq!(refs.len(), 2, "the policy set and the model registry");
    let damage = prop_oneof![
        3 => Just(Damage::Intact),
        2 => (0..2usize).prop_map(Damage::Drop),
        1 => (0..2usize).prop_map(Damage::Corrupt),
        1 => Just(Damage::Down),
    ];
    let step = (0..4usize, damage, prop::bool::weighted(0.8));
    let config = Config {
        cases: 12,
        failure_persistence: None,
        ..Config::default()
    };
    let result = TestRunner::new(config).run(&prop::collection::vec(step, 1..10), |steps| {
        let mut both = Both::opened(TestDb::new().unwrap());
        let mut heads: BTreeMap<String, u64> = STREAMS.map(|s| (s.to_owned(), 1)).into();
        let mut committed = BTreeSet::new();
        for (which, damage, current_epoch) in steps {
            let (name, paths) = CHECKED[which];
            let d = draft(name);
            let named = |k: usize| paths.iter().find(|p| reference(&d, p) == refs[k]);
            let (source, hit): (Box<dyn ArtifactSource + Sync>, _) = match damage {
                Damage::Intact => (Box::new(stored()), None),
                Damage::Drop(k) => (Box::new(without(refs[k])), named(k).map(|p| missing(0, p))),
                Damage::Corrupt(k) => (Box::new(altered(refs[k])), named(k).map(|_| Unavailable)),
                Damage::Down => (Box::new(Down), Some(Unavailable)),
            };
            let head = heads.get_mut(stream_of(&d).as_str()).unwrap();
            let outcome = both.append(*head, if current_epoch { 1 } else { 2 }, &[&d], &*source);
            if committed.contains(&member(&d, "event_id")) {
                prop_assert_eq!(outcome.name(), "AlreadyCommitted", "{} again", name);
            } else if let Some(want) = hit {
                prop_assert_eq!(outcome, want, "{} with {:?}", name, damage);
            } else if !current_epoch {
                prop_assert_eq!(outcome, Fenced { current_epoch: 1 });
            } else {
                let Committed(rows) = outcome else {
                    return Err(TestCaseError::fail(format!("{name} did not commit")));
                };
                *head += 1;
                prop_assert_eq!(rows.iter().map(|r| r.seq).collect::<Vec<_>>(), [*head]);
                committed.insert(member(&d, "event_id"));
            }
        }
        Ok(())
    });
    if let Err(failure) = result {
        panic!("{failure}");
    }
}
