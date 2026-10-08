//! P0 (E10-16, DEC-520): `PgControlJournal` and the `--journal` and `--store` options, against
//! oracles sharing no code with it: the journal vectors, `MemoryJournal`'s artifact-aware append,
//! and the CLI tests' own journal. Every Postgres test starts with [`unparsable_dsn_property`], which
//! needs no database, so the pending gate sees the stub without one (DEC-510 item 3).

#[path = "common/mod.rs"]
mod cli;
#[path = "../../mandate-journal/tests/common/mod.rs"]
mod common;
#[path = "../../mandate-journal/tests/conformance/mod.rs"]
#[allow(
    unused_imports,
    unused_macros,
    reason = "only the fixture helpers are used here; the suite runs in `mandate-journal-pg`"
)]
mod conformance;
#[path = "../../mandate-journal-pg/tests/support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};

use clap::Parser;
use clap::error::ErrorKind;
use common::edit;
use conformance::{fixture, fixture_artifacts, get, list, text};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Value, to_canonical};
use mandate_cli::control::{ControlError, ControlJournal};
use mandate_cli::postgres::{JournalArgs, PgControlJournal};
use mandate_journal::AppendOutcome::{self, AlreadyCommitted, Committed, HeadMismatch};
use mandate_journal::{ArtifactRef, ArtifactStore, InvalidReason, MemoryJournal, StreamId};
use mandate_journal_pg::APP_ROLE;
use mandate_time::UtcNanos;
use secrecy::ExposeSecret;
use support::{TestDb, URL_VAR};

const CONTROL: &str = "ctl:ws_01J8Z2";
const LATER: &str = "\"2026-09-20T13:05:01.000000000Z\"";

type Objects = std::collections::BTreeMap<mandate_canon::Digest, Vec<u8>>;

fn args(journal: &str, store: &Path) -> JournalArgs {
    JournalArgs {
        journal: journal.into(),
        store: store.to_owned(),
    }
}

/// A fresh store root for one test.
fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("mandate-cli-p0-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    root
}

/// A DSN that does not parse is refused when the journal is opened, as a journal error that never
/// repeats the DSN or its password (`AGENTS.md` rule 7).
fn unparsable_dsn_property() {
    let secret = "p0-sentinel-password";
    let dsn = format!("postgres://owner:{secret}@[::1/mandate");
    let opened = PgControlJournal::open(&args(&dsn, &scratch("dsn")));
    match opened {
        Err(ControlError::Journal(why)) => assert!(!why.contains(secret), "{why}"),
        Err(other) => panic!("{other:?}"),
        Ok(_) => panic!("a DSN that does not parse was opened"),
    }
}

/// After [`unparsable_dsn_property`], a database, and the journal opened at a DSN acting as the
/// application role in its schema, with an empty store; `None` where `TestDb` skips.
fn opened(name: &str) -> Option<(TestDb, PgControlJournal, PathBuf)> {
    unparsable_dsn_property();
    let db = TestDb::new()?;
    let url = std::env::var(URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    let root = scratch(name);
    let journal = PgControlJournal::open(&args(&format!("{url}{joiner}options={options}"), &root));
    Some((db, journal.unwrap(), root))
}

/// The writer's draft of a vector's stored body: the body without the journal-assigned fields.
fn draft_of(body: &Value) -> Vec<u8> {
    let draft = edit(&to_canonical(body), "seq", None);
    edit(&edit(&draft, "prev_hash", None), "recorded_at", None)
}

fn opening() -> Vec<u8> {
    draft_of(get(&list(&fixture(), "control_stream.chain")[0], "body"))
}

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse(text).unwrap()
}

fn stream() -> StreamId {
    StreamId::parse(CONTROL).unwrap()
}

/// The control-stream vectors appended through `ControlJournal` are stored byte for byte with the
/// vectors' hashes, and read back the same (journal spec §5.1, §11).
#[test]
#[ignore = "pending E10-16"]
fn the_control_stream_vectors_are_stored_byte_for_byte() {
    let Some(t) = opened("v") else { return };
    let (_db, mut pg, _) = t;
    let fx = fixture();
    let chain = list(&fx, "control_stream.chain");
    assert!(!chain.is_empty(), "the vectors hold a control stream");
    let epoch = pg.take_ownership(&stream()).unwrap();
    let mut stored = Vec::new();
    for (head, entry) in (0u64..).zip(chain) {
        let body = get(entry, "body");
        let when = at(text(body, "recorded_at"));
        let outcome = pg.append(&stream(), head, epoch, when, &[&draft_of(body)]);
        let Committed(rows) = outcome.unwrap() else {
            panic!("seq {} did not commit", head + 1)
        };
        let [row] = rows.as_slice() else {
            panic!("one row per append: {rows:?}")
        };
        assert_eq!(String::from_utf8_lossy(&row.body), text(entry, "canonical"));
        assert_eq!(row.hash.to_hex(), text(entry, "hash"));
        stored.push(row.clone());
    }
    assert_eq!(pg.rows(&stream()).unwrap(), stored);
    let (head, last) = (pg.head(&stream()).unwrap(), stored.last().unwrap());
    assert_eq!([head.seq, head.writer_epoch], [last.seq, epoch]);
    assert_eq!(head.hash, last.hash);
}

/// A version-2 registration is refused `missing_artifact`, writing nothing, until its object is in
/// the store at `--store`, and then commits: the outcomes of `MemoryJournal`'s artifact-aware
/// append over the same objects (journal spec §11 check 6; DEC-510).
#[test]
#[ignore = "pending E10-16"]
fn a_registration_commits_only_once_its_object_is_in_the_store() {
    let Some(t) = opened("o") else { return };
    let (_db, mut pg, root) = t;
    let section = get(&fixture(), "production_config_refs").clone();
    let all = fixture_artifacts(&section);
    let (mut objects, mut memory) = (Objects::new(), MemoryJournal::new());
    let (opening, start) = (opening(), at("2026-09-20T13:00:00.000000400Z"));
    let epoch = pg.take_ownership(&stream()).unwrap();
    assert_eq!(memory.take_ownership(&stream()), epoch);
    let first = pg.append(&stream(), 0, epoch, start, &[&opening]).unwrap();
    let second = memory.append(&stream(), 0, epoch, start, &[&opening]);
    assert_eq!(first, second);
    let (mut files, when) = (FsArtifactStore::open(&root).unwrap(), start);
    for (head, name) in (1..).zip(["model_registry_registration", "policy_registration"]) {
        let record = get(&section, &format!("valid_drafts.{name}"));
        let draft = to_canonical(record);
        let reference = ArtifactRef::parse(text(record, "payload.content_hash")).unwrap();
        let before = pg.rows(&stream()).unwrap();
        let refused = pg.append(&stream(), head, epoch, when, &[&draft]).unwrap();
        let oracle =
            memory.append_with_config_artifacts(&stream(), head, epoch, when, &[&draft], &objects);
        assert_eq!(refused, oracle, "{name} before its object is stored");
        let missing = InvalidReason::MissingArtifact;
        let at_draft = |o: &AppendOutcome| matches!(o, AppendOutcome::Invalid { draft: 0, error } if error.reason == missing);
        assert!(at_draft(&refused), "{name}: {refused:?}");
        let after = pg.rows(&stream()).unwrap();
        assert_eq!(after, before, "{name}: the refusal wrote nothing");
        let object = all[&reference.digest()].clone();
        assert_eq!(files.put_artifact(&object).unwrap(), reference);
        objects.insert(reference.digest(), object);
        let committed = pg.append(&stream(), head, epoch, when, &[&draft]).unwrap();
        let oracle =
            memory.append_with_config_artifacts(&stream(), head, epoch, when, &[&draft], &objects);
        assert_eq!(committed, oracle, "{name} once its object is stored");
        assert_eq!(committed.name(), "Committed", "{name}");
    }
    assert_eq!(pg.rows(&stream()).unwrap(), memory.rows(&stream()));
}

/// A retried outcome as the CLI's retry arms read it: its kind, and the rows' `seq` and `event_id`
/// or the head and epoch it names. Sealed bytes are left out: the CLI tests' journal seals nothing.
fn shape(outcome: &AppendOutcome) -> String {
    match outcome {
        Committed(rows) | AlreadyCommitted(rows) => {
            let rows: Vec<(u64, &str)> =
                rows.iter().map(|r| (r.seq, r.event_id.as_str())).collect();
            format!("{} {rows:?}", outcome.name())
        }
        HeadMismatch { actual_seq, .. } => format!("HeadMismatch {actual_seq}"),
        other => format!("{other:?}"),
    }
}

/// Ownership, fencing, a head that moved, an identical retry and a changed resend answer as the CLI
/// tests' own journal answers them, the journal every M7 command test already runs against
/// (journal spec §5.1; DEC-257 item 16).
#[test]
#[ignore = "pending E10-16"]
fn ownership_fencing_and_retries_answer_as_the_cli_tests_journal() {
    let Some(t) = opened("r") else { return };
    let (_db, mut pg, _) = t;
    let mut double = cli::Journal::default();
    let journals: [&mut dyn ControlJournal; 2] = [&mut pg, &mut double];
    let chain = list(&fixture(), "control_stream.chain").to_vec();
    let id = |i: usize| text(get(&chain[i], "body"), "event_id").to_owned();
    let (opening, next) = (opening(), draft_of(get(&chain[1], "body")));
    let changed = edit(&next, "event_time", Some(LATER));
    let (s, when) = (stream(), at("2026-09-20T13:05:00.000000400Z"));
    let mut seen = Vec::new();
    for journal in journals {
        let (first, second) = (journal.take_ownership(&s), journal.take_ownership(&s));
        let mut answers = vec![format!("epochs {} {}", first.unwrap(), second.unwrap())];
        let steps: [(u64, u64, &[u8]); 6] = [
            (0, 1, &opening),
            (0, 2, &opening),
            (0, 2, &next),
            (1, 2, &next),
            (1, 2, &next),
            (2, 2, &changed),
        ];
        for (head, epoch, draft) in steps {
            answers.push(shape(
                &journal.append(&s, head, epoch, when, &[draft]).unwrap(),
            ));
        }
        seen.push(answers);
    }
    let want = [
        "epochs 1 2".to_owned(),
        "Fenced { current_epoch: 2 }".to_owned(),
        format!("Committed [(1, {:?})]", id(0)),
        "HeadMismatch 1".to_owned(),
        format!("Committed [(2, {:?})]", id(1)),
        format!("AlreadyCommitted [(2, {:?})]", id(1)),
        "IdempotencyConflict { stored_seq: 2 }".to_owned(),
    ];
    assert_eq!(
        seen,
        [want.clone(), want],
        "Postgres, then the CLI tests' journal"
    );
}

#[derive(Parser)]
struct Command {
    #[command(flatten)]
    target: JournalArgs,
}

#[test]
fn journal_and_store_are_both_required() {
    let given = ["mandate", "--journal", "postgres://h/j", "--store", "s"];
    let target = Command::try_parse_from(given)
        .map_err(|e| e.to_string())
        .unwrap()
        .target;
    let journal = target.journal.expose_secret();
    assert_eq!((journal, target.store), ("postgres://h/j", "s".into()));
    for given in [["--store", "s"], ["--journal", "postgres://h/j"]] {
        let missing = Command::try_parse_from(["mandate"].into_iter().chain(given));
        let kind = missing.err().map(|e| e.kind());
        assert_eq!(kind, Some(ErrorKind::MissingRequiredArgument), "{given:?}");
    }
}

#[test]
fn the_options_debug_form_never_shows_the_dsn() {
    let given = args("postgres://owner:p0-sentinel@h/j", Path::new("store-root"));
    let shown = format!("{given:?}");
    assert!(!shown.contains("p0-sentinel"), "{shown}");
    assert!(shown.contains("store-root"), "{shown}");
}
