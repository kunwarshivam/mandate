//! Lane ID1 (DEC-991; identity spec §3.2, §4.3, ID-13; journal spec §9.12): `mandate workspace
//! found` writes the workspace's founding grant once, as the system, with the owner bundle, on a
//! control stream holding only its opening; a re-run for the same member commits nothing and
//! succeeds, so a Kubernetes Job may run it again; any other membership, or any record after the
//! opening, refuses it. Every grant it commits is a draft the journal itself admits.
//!
//! The property test runs random bootstraps, with fences, racing writers, and answers lost after
//! the commit, against the in-memory journal, and checks each run's answer and the stored rows
//! against an oracle that reads the rows' JSON itself, never through `mandate-identity`.

mod common;

use clap::Parser;
use clap::error::ErrorKind;
use common::{CONTROL, Journal, int, object, text};
use mandate_canon::{Value, parse, to_canonical};
use mandate_cli::control::{ControlError, ControlJournal};
use mandate_cli::workspace::{Founded, Founding, WorkspaceCommand, found_in};
use mandate_cli::{Cli, Command};
use mandate_journal::{AppendOutcome, Draft, StoredEvent, StreamId};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const EPOCH: i64 = 1_790_000_000;
const BUILD: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn member(n: u8) -> String {
    format!("01J8Z4M0AD0000000000000AD{n}")
}

fn ctl() -> StreamId {
    StreamId::parse(CONTROL).unwrap()
}

/// A system draft of `event_type` on the control stream, in `environment`.
fn system_draft(environment: &str, event_id: &str, event_type: &str, payload: Value) -> Vec<u8> {
    to_canonical(&object(&[
        ("envelope_version", int(1)),
        ("environment", text(environment)),
        ("event_id", text(event_id)),
        ("stream_id", text(CONTROL)),
        ("event_type", text(event_type)),
        ("schema_version", int(1)),
        ("event_time", text("2026-09-21T14:00:00.000000000Z")),
        ("clock_source", text("local")),
        ("causation_id", Value::Null),
        ("correlation_id", Value::Null),
        (
            "actor",
            object(&[
                ("kind", text("system")),
                ("id", text("control_services")),
                ("version", text("0.1.0")),
                ("build", text(BUILD)),
            ]),
        ),
        ("config_refs", object(&[])),
        ("payload", payload),
        ("artifact_refs", Value::Array(Vec::new())),
        ("pii_refs", Value::Array(Vec::new())),
    ]))
}

fn append(journal: &mut Journal, bytes: &[u8]) {
    let head = journal.head(&ctl()).unwrap().seq;
    let epoch = journal.take_ownership(&ctl()).unwrap();
    let at = UtcNanos::from_parts(EPOCH, 0).unwrap();
    let outcome = journal.append(&ctl(), head, epoch, at, &[bytes]).unwrap();
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
}

/// The stream's `StreamOpened`, in `environment`.
fn open(journal: &mut Journal, environment: &str) {
    let payload = object(&[
        ("stream_type", text("control")),
        ("workspace_id", text("ws1")),
    ]);
    let bytes = system_draft(
        environment,
        "01J8Z4M0000000000000000OP1",
        "StreamOpened",
        payload,
    );
    append(journal, &bytes);
}

/// Another writer's record of another type.
fn other(journal: &mut Journal, n: usize) {
    let id = format!("01J8Z4M00000000000000000{n:02}");
    let payload = object(&[("note", text("other"))]);
    let bytes = system_draft("paper", &id, "ConfigSnapshotRegistered", payload);
    append(journal, &bytes);
}

/// A clock that steps one second per reading.
fn ticking() -> impl FnMut() -> UtcNanos {
    let mut secs = EPOCH;
    move || {
        secs += 1;
        UtcNanos::from_parts(secs, 0).unwrap()
    }
}

fn run(journal: &mut Journal, n: u8, method: &str) -> (Result<Founded, ControlError>, String) {
    let ulid = member(n);
    let founding = Founding {
        workspace: "ws1",
        member: &ulid,
        method,
        open: false,
    };
    let mut out = Vec::new();
    let got = found_in(journal, founding, &mut ticking(), &mut out);
    (got, String::from_utf8(out).unwrap())
}

/// `run` with `--open`.
fn run_opening(journal: &mut Journal, n: u8) -> (Result<Founded, ControlError>, String) {
    let ulid = member(n);
    let founding = Founding {
        workspace: "ws1",
        member: &ulid,
        method: "oidc",
        open: true,
    };
    let mut out = Vec::new();
    let got = found_in(journal, founding, &mut ticking(), &mut out);
    (got, String::from_utf8(out).unwrap())
}

fn kinds(journal: &Journal) -> Vec<String> {
    journal
        .rows(&ctl())
        .unwrap()
        .into_iter()
        .map(|r| r.event_type)
        .collect()
}

fn refused(reason: &'static str) -> Result<Founded, ControlError> {
    Err(ControlError::Refused { reason })
}

fn body(row: &StoredEvent) -> Value {
    parse(&row.body).unwrap()
}

fn grants(journal: &Journal) -> Vec<StoredEvent> {
    journal
        .rows(&ctl())
        .unwrap()
        .into_iter()
        .filter(|r| r.event_type == "MemberActivated")
        .collect()
}

fn get<'a>(value: &'a Value, path: &str) -> &'a Value {
    path.split('.')
        .try_fold(value, |v, name| v.get(name))
        .unwrap_or(&Value::Null)
}

/// The command takes a workspace, a member, a sign-in method, and the journal, and no user and no
/// role.
#[test]
fn workspace_found_takes_a_member_and_a_method_and_no_role() {
    let line = "mandate workspace found --workspace ws1 --member 01J8Z4M0AD0000000000000AD1 \
                --method oidc --journal postgres://h/j --store s";
    let full: Vec<&str> = line.split_whitespace().collect();
    let parsed = Cli::try_parse_from(&full).map(|cli| cli.command);
    let Ok(Command::Workspace(WorkspaceCommand::Found(args))) = parsed else {
        panic!("{parsed:?}")
    };
    assert_eq!(
        (
            args.workspace.as_str(),
            args.member.as_str(),
            args.method.as_str()
        ),
        ("ws1", "01J8Z4M0AD0000000000000AD1", "oidc")
    );
    for (skip, missing) in [(3, "--workspace"), (5, "--member"), (7, "--method")] {
        let given: Vec<&str> = [&full[..skip], &full[skip + 2..]].concat();
        let kind = Cli::try_parse_from(given).err().map(|e| e.kind());
        assert_eq!(kind, Some(ErrorKind::MissingRequiredArgument), "{missing}");
    }
    for extra in [["--user", "u1"], ["--role", "viewer"]] {
        let given: Vec<&str> = full.iter().copied().chain(extra).collect();
        let kind = Cli::try_parse_from(given).err().map(|e| e.kind());
        assert_eq!(kind, Some(ErrorKind::UnknownArgument), "{extra:?}");
    }
}

/// A bad method, member, or workspace is refused before the journal is read or written.
#[test]
fn bad_input_is_refused_before_the_journal_is_touched() {
    let cases = [
        ("ws1", member(1), "password", "method_invalid"),
        ("ws1", member(1), "", "method_invalid"),
        ("ws1", "not-a-ulid".to_owned(), "oidc", "member_invalid"),
        (
            "ws1",
            "01j8z4m0ad0000000000000ad1".to_owned(),
            "oidc",
            "member_invalid",
        ),
        ("ws:1", member(1), "oidc", "owner_workspace_invalid"),
        ("", member(1), "oidc", "owner_workspace_invalid"),
    ];
    for (workspace, ulid, method, reason) in cases {
        let mut journal = Journal::default();
        open(&mut journal, "paper");
        let attempts = journal.attempts.len();
        let founding = Founding {
            workspace,
            member: &ulid,
            method,
            open: false,
        };
        let mut out = Vec::new();
        let got = found_in(&mut journal, founding, &mut ticking(), &mut out);
        assert_eq!(got, refused(reason), "{workspace:?} {ulid:?} {method:?}");
        assert_eq!(journal.attempts.len(), attempts, "nothing appended");
        assert!(out.is_empty(), "a refusal prints nothing");
    }
}

/// An unopened stream is refused, and nothing is appended.
#[test]
fn an_unopened_workspace_is_refused() {
    let mut journal = Journal::default();
    assert_eq!(
        run(&mut journal, 1, "oidc").0,
        refused("workspace_not_open")
    );
    assert!(journal.rows(&ctl()).unwrap().is_empty());
}

/// On an opened stream, the grant is committed at seq 2: the system's `MemberActivated` with
/// reason `founding`, the owner bundle, the given method, no invitation, session, independence,
/// or cool-off, its instants the clock's reading, in the stream's environment; and the journal
/// itself admits it.
#[test]
fn the_grant_is_the_owner_bundle_written_by_the_system() {
    for environment in ["paper", "live"] {
        let mut journal = Journal::default();
        open(&mut journal, environment);
        let (got, printed) = run(&mut journal, 1, "oidc");
        assert_eq!(
            got,
            Ok(Founded {
                seq: 2,
                granted: true
            })
        );
        assert!(
            printed.contains(&member(1)) && printed.contains("seq 2"),
            "{printed}"
        );
        let rows = grants(&journal);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.seq, 2);
        let envelope = body(row);
        let instant = &UtcNanos::from_parts(EPOCH + 1, 0).unwrap().to_string();
        let want_payload = object(&[
            ("member", text(&member(1))),
            ("invitation", Value::Null),
            ("reason", text("founding")),
            (
                "roles",
                Value::Array(vec![
                    text("approver"),
                    text("operator"),
                    text("workspace_admin"),
                ]),
            ),
            ("method", text("oidc")),
            ("activated_at", text(instant)),
            ("independent_approval_required", Value::Bool(false)),
            ("cool_off_ends_at", text(instant)),
            ("session_ref", Value::Null),
        ]);
        assert_eq!(get(&envelope, "payload"), &want_payload);
        assert_eq!(get(&envelope, "event_time"), &text(instant));
        assert_eq!(get(&envelope, "environment"), &text(environment));
        assert_eq!(get(&envelope, "actor.kind"), &text("system"));
        assert_eq!(get(&envelope, "actor.id"), &text("control_services"));
        assert_eq!(get(&envelope, "stream_id"), &text(CONTROL));
        let admitted = Draft::parse(&row.body).map(|d| d.event_type().to_owned());
        assert_eq!(admitted, Ok("MemberActivated".to_owned()), "{environment}");
    }
}

/// Each sign-in method is written as given.
#[test]
fn each_method_is_written_as_given() {
    for method in ["passkey", "oidc", "email_link"] {
        let mut journal = Journal::default();
        open(&mut journal, "paper");
        assert!(run(&mut journal, 1, method).0.is_ok(), "{method}");
        let envelope = body(&grants(&journal)[0]);
        assert_eq!(get(&envelope, "payload.method"), &text(method));
    }
}

/// A re-run for the founder finds the grant, commits nothing, and succeeds, even after other
/// records; a run for anyone else is refused.
#[test]
fn a_re_run_succeeds_and_another_member_is_refused() {
    let mut journal = Journal::default();
    open(&mut journal, "paper");
    run(&mut journal, 1, "oidc").0.unwrap();
    other(&mut journal, 1);
    let head = journal.head(&ctl()).unwrap().seq;
    let (got, printed) = run(&mut journal, 1, "oidc");
    assert_eq!(
        got,
        Ok(Founded {
            seq: 2,
            granted: false
        })
    );
    assert!(printed.contains("already"), "{printed}");
    assert_eq!(run(&mut journal, 2, "oidc").0, refused("members_exist"));
    assert_eq!(journal.head(&ctl()).unwrap().seq, head, "nothing committed");
    assert_eq!(grants(&journal).len(), 1);
}

/// `--open` is a flag, off unless given (DEC-2114).
#[test]
fn open_is_a_flag_off_by_default() {
    let line = "mandate workspace found --workspace ws1 --member 01J8Z4M0AD0000000000000AD1 \
                --method oidc --journal postgres://h/j --store s";
    let base: Vec<&str> = line.split_whitespace().collect();
    for (extra, want) in [(None, false), (Some("--open"), true)] {
        let given: Vec<&str> = base.iter().copied().chain(extra).collect();
        let parsed = Cli::try_parse_from(given).map(|cli| cli.command);
        let Ok(Command::Workspace(WorkspaceCommand::Found(args))) = parsed else {
            panic!("{parsed:?}")
        };
        assert_eq!(args.open, want, "{extra:?}");
    }
}

/// With `--open`, a stream holding no event is opened in `paper` as the system's
/// `control_services`, at the clock's first reading, and founded at seq 2; a re-run opens nothing
/// and commits nothing.
#[test]
fn open_opens_an_empty_stream_once_then_founds_it() {
    let mut journal = Journal::default();
    let (got, printed) = run_opening(&mut journal, 1);
    assert_eq!(
        got,
        Ok(Founded {
            seq: 2,
            granted: true
        })
    );
    assert_eq!(kinds(&journal), ["StreamOpened", "MemberActivated"]);
    let rows = journal.rows(&ctl()).unwrap();
    let opening = body(&rows[0]);
    assert_eq!(get(&opening, "environment"), &text("paper"));
    assert_eq!(get(&opening, "actor.kind"), &text("system"));
    assert_eq!(get(&opening, "actor.id"), &text("control_services"));
    assert_eq!(get(&opening, "payload.workspace_id"), &text("ws1"));
    assert_eq!(
        get(&opening, "event_time"),
        &text(&UtcNanos::from_parts(EPOCH + 1, 0).unwrap().to_string())
    );
    assert_eq!(
        Draft::parse(&rows[0].body).map(|d| d.event_type().to_owned()),
        Ok("StreamOpened".to_owned())
    );
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(lines.len(), 2, "{printed}");
    assert!(
        lines[0].starts_with("opened ") && lines[0].contains("seq 1"),
        "{printed}"
    );
    assert!(
        lines[1].starts_with("founded ") && lines[1].contains("seq 2"),
        "{printed}"
    );
    let (again, printed) = run_opening(&mut journal, 1);
    assert_eq!(
        again,
        Ok(Founded {
            seq: 2,
            granted: false
        })
    );
    assert!(!printed.contains("opened"), "{printed}");
    assert_eq!(kinds(&journal), ["StreamOpened", "MemberActivated"]);
}

/// `--open` never opens a stream that holds an event: an opened stream is founded as without it,
/// in its own environment, and a used or founded one is refused as without it.
#[test]
fn open_leaves_a_stream_holding_any_event_as_it_is() {
    let mut live = Journal::default();
    open(&mut live, "live");
    assert_eq!(
        run_opening(&mut live, 1).0,
        Ok(Founded {
            seq: 2,
            granted: true
        })
    );
    assert_eq!(kinds(&live), ["StreamOpened", "MemberActivated"]);
    assert_eq!(get(&body(&grants(&live)[0]), "environment"), &text("live"));
    let mut used = Journal::default();
    open(&mut used, "paper");
    other(&mut used, 1);
    assert_eq!(run_opening(&mut used, 1).0, refused("workspace_not_new"));
    assert_eq!(kinds(&used), ["StreamOpened", "ConfigSnapshotRegistered"]);
    assert_eq!(run_opening(&mut live, 2).0, refused("members_exist"));
    assert_eq!(kinds(&live), ["StreamOpened", "MemberActivated"]);
    let mut empty = Journal::default();
    assert_eq!(run(&mut empty, 1, "oidc").0, refused("workspace_not_open"));
    assert!(kinds(&empty).is_empty(), "without --open nothing is opened");
}

/// The rows another process's `found --open` for `member(1)` commits on a journal of its own,
/// with its own later clock: its opening, and its grant when `granted`.
fn another_openers(granted: bool) -> Vec<Vec<u8>> {
    let mut theirs = Journal::default();
    let ulid = member(1);
    let mut secs = EPOCH + 100;
    let mut later = move || {
        secs += 1;
        UtcNanos::from_parts(secs, 0).unwrap()
    };
    let founding = Founding {
        workspace: "ws1",
        member: &ulid,
        method: "oidc",
        open: true,
    };
    found_in(&mut theirs, founding, &mut later, &mut Vec::new()).unwrap();
    let rows = theirs.rows(&ctl()).unwrap();
    rows.iter()
        .take(if granted { 2 } else { 1 })
        .map(|r| r.body.clone())
        .collect()
}

/// Two processes run `--open` on the same empty stream, and the other commits first, between this
/// one's read and its append. Its opening has this one's id (derived at head 0) but its own
/// instant, so the journal answers this one's opening as an idempotency conflict: this run fails
/// closed, committing nothing, and is never a second opening. A rerun, as the founding hook's
/// retry is, finds the stream opened and founds it, or finds the other's grant and commits nothing
/// (DEC-2114).
#[test]
fn two_processes_opening_at_once_open_the_stream_once() {
    let theirs_at = UtcNanos::from_parts(EPOCH + 101, 0).unwrap().to_string();
    for (granted, landed) in [(true, 1), (false, 2)] {
        let mut journal = Journal::default();
        journal.land_before_next = another_openers(landed == 2);
        let (first, printed) = run_opening(&mut journal, 1);
        assert!(
            matches!(first, Err(ControlError::Journal(_))),
            "{landed}: {first:?}"
        );
        assert!(!printed.contains("founded"), "{printed}");
        assert_eq!(
            kinds(&journal).len(),
            landed,
            "nothing of this run's is stored"
        );
        let (rerun, _) = run_opening(&mut journal, 1);
        assert_eq!(rerun, Ok(Founded { seq: 2, granted }), "{landed}");
        assert_eq!(kinds(&journal), ["StreamOpened", "MemberActivated"]);
        let rows = journal.rows(&ctl()).unwrap();
        assert_eq!(get(&body(&rows[0]), "event_time"), &text(&theirs_at));
        assert_eq!(grants(&journal).len(), 1);
    }
}

/// A stream with a record after its opening and no membership is not new.
#[test]
fn a_used_workspace_is_refused() {
    let mut journal = Journal::default();
    open(&mut journal, "paper");
    other(&mut journal, 1);
    assert_eq!(run(&mut journal, 1, "oidc").0, refused("workspace_not_new"));
    assert!(grants(&journal).is_empty());
}

/// Fences are retried after a pause; a record landing between the read and the append moves the
/// head, and the re-read refuses; an answer lost after the commit is found on the re-read.
#[test]
fn fences_races_and_lost_answers() {
    let mut journal = Journal::default();
    open(&mut journal, "paper");
    journal.fence_next = 2;
    assert_eq!(
        run(&mut journal, 1, "oidc").0,
        Ok(Founded {
            seq: 2,
            granted: true
        })
    );
    assert_eq!(journal.waits.len(), 2, "a pause after each fence");

    let mut journal = Journal::default();
    open(&mut journal, "paper");
    journal.behind_next = 1;
    assert_eq!(run(&mut journal, 1, "oidc").0, refused("workspace_not_new"));
    assert!(grants(&journal).is_empty());

    let mut journal = Journal::default();
    open(&mut journal, "paper");
    journal.ambiguous_next = 1;
    assert_eq!(
        run(&mut journal, 1, "oidc").0,
        Ok(Founded {
            seq: 2,
            granted: false
        })
    );
    assert_eq!(grants(&journal).len(), 1);
}

/// A journal that never settles is reported, and commits nothing.
#[test]
fn a_journal_that_never_settles_is_reported() {
    let mut journal = Journal::default();
    open(&mut journal, "paper");
    journal.fence_next = 100;
    let got = run(&mut journal, 1, "oidc").0;
    assert!(matches!(got, Err(ControlError::Journal(_))), "{got:?}");
    assert!(grants(&journal).is_empty());
}

/// One step of a random history.
#[derive(Debug, Clone)]
enum Step {
    Open,
    Other,
    Run {
        n: u8,
        fences: u32,
        behind: bool,
        lost: bool,
    },
    /// A replay: every stored grant parsed again by the journal.
    Replay,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        1 => Just(Step::Open),
        1 => Just(Step::Other),
        5 => (1_u8..4, 0_u32..3, any::<bool>(), any::<bool>())
            .prop_map(|(n, fences, behind, lost)| Step::Run { n, fences, behind, lost }),
        1 => Just(Step::Replay),
    ]
}

/// The oracle's model: whether the stream is open, whether a record follows its opening, and who
/// founded it.
#[derive(Debug, Default)]
struct Model {
    open: bool,
    used: bool,
    founder: Option<u8>,
}

impl Model {
    fn expect(&self, n: u8) -> Result<Founded, ControlError> {
        let already = Ok(Founded {
            seq: 2,
            granted: false,
        });
        match self.founder {
            Some(f) if f == n => already,
            Some(_) => refused("members_exist"),
            None if !self.open => refused("workspace_not_open"),
            None if self.used => refused("workspace_not_new"),
            None => Ok(Founded {
                seq: 2,
                granted: true,
            }),
        }
    }

    /// A run only appends when it would grant: fences are retried, a racing record makes the
    /// stream used, and a lost answer has committed.
    fn run(&mut self, n: u8, behind: bool, lost: bool) -> Result<Founded, ControlError> {
        let first = self.expect(n);
        if !matches!(first, Ok(Founded { granted: true, .. })) {
            return first;
        }
        if behind {
            self.used = true;
            return self.expect(n);
        }
        self.founder = Some(n);
        if lost {
            return self.expect(n);
        }
        first
    }
}

/// The invariants, from the rows' JSON: at most one `MemberActivated`; it is the model's
/// founder's grant of exactly the owner bundle, written by the system, at seq 2.
fn check(journal: &Journal, model: &Model, replay: bool) -> Result<(), String> {
    let rows = grants(journal);
    if rows.len() > 1 {
        return Err(format!("{} grants", rows.len()));
    }
    let founder = rows.first().map(|row| {
        let envelope = body(row);
        let payload = get(&envelope, "payload");
        let roles = get(payload, "roles");
        let bundle = Value::Array(vec![
            text("approver"),
            text("operator"),
            text("workspace_admin"),
        ]);
        let scoped = row.seq == 2
            && *roles == bundle
            && get(payload, "reason") == &text("founding")
            && get(payload, "invitation") == &Value::Null
            && get(payload, "independent_approval_required") == &Value::Bool(false)
            && get(payload, "cool_off_ends_at") == get(&envelope, "event_time")
            && get(&envelope, "actor.kind") == &text("system");
        (scoped, get(payload, "member").clone())
    });
    match (founder, model.founder) {
        (None, None) => {}
        (Some((true, named)), Some(n)) if named == text(&member(n)) => {}
        other => return Err(format!("stored {other:?}, model {:?}", model.founder)),
    }
    if replay {
        for row in rows {
            Draft::parse(&row.body).map_err(|e| format!("replay: {e:?}"))?;
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// No sequence of bootstrap runs, fences, racing writers, lost answers, and replays yields two
    /// founders or a grant outside the founding scope, and each run answers as the oracle's model
    /// says.
    #[test]
    fn no_history_yields_two_founders(steps in prop::collection::vec(step(), 1..16)) {
        let (mut journal, mut model) = (Journal::default(), Model::default());
        let mut others = 0;
        for step in &steps {
            match step {
                Step::Open if !model.open => {
                    open(&mut journal, "paper");
                    model.open = true;
                }
                Step::Other if model.open => {
                    others += 1;
                    other(&mut journal, others);
                    model.used |= model.founder.is_none();
                }
                Step::Run { n, fences, behind, lost } => {
                    journal.fence_next = *fences;
                    journal.behind_next = u32::from(*behind);
                    journal.ambiguous_next = u32::from(*lost);
                    let got = run(&mut journal, *n, "oidc").0;
                    let want = model.run(*n, *behind, *lost);
                    prop_assert_eq!(got, want, "{:?}", step);
                    journal.fence_next = 0;
                    journal.behind_next = 0;
                    journal.ambiguous_next = 0;
                }
                Step::Open | Step::Other | Step::Replay => {}
            }
            let checked = check(&journal, &model, matches!(step, Step::Replay));
            prop_assert!(checked.is_ok(), "after {:?}: {:?}", step, checked);
        }
    }
}

/// A journal whose commits become visible to reads one read late: the read right after an append
/// that answered `Unavailable` does not see what it committed.
struct LateReads {
    inner: Journal,
    hidden: bool,
    committed_once: bool,
}

impl ControlJournal for LateReads {
    fn rows(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, ControlError> {
        let mut rows = self.inner.rows(stream)?;
        if self.hidden {
            rows.retain(|r| r.event_type != "MemberActivated");
        }
        Ok(rows)
    }

    fn head(&self, stream: &StreamId) -> Result<mandate_journal::Head, ControlError> {
        self.inner.head(stream)
    }

    fn take_ownership(&mut self, stream: &StreamId) -> Result<u64, ControlError> {
        self.inner.take_ownership(stream)
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> Result<AppendOutcome, ControlError> {
        let outcome =
            self.inner
                .append(stream, expected_head, writer_epoch, recorded_at, drafts)?;
        if !self.committed_once && matches!(outcome, AppendOutcome::Committed(_)) {
            self.committed_once = true;
            self.hidden = true;
            return Ok(AppendOutcome::Unavailable);
        }
        self.hidden = false;
        Ok(outcome)
    }

    fn wait(&mut self, delay: std::time::Duration) {
        self.inner.wait(delay);
    }
}

/// A grant committed by an attempt whose answer was lost, and seen only after the next attempt
/// appended at the same head, is an idempotency conflict under the member's id; it is re-read
/// and found, never reported as a failure or written twice.
#[test]
fn a_late_visible_grant_is_found_not_failed() {
    let mut inner = Journal::default();
    open(&mut inner, "paper");
    let mut journal = LateReads {
        inner,
        hidden: false,
        committed_once: false,
    };
    let ulid = member(1);
    let founding = Founding {
        workspace: "ws1",
        member: &ulid,
        method: "oidc",
        open: false,
    };
    let got = found_in(&mut journal, founding, &mut ticking(), &mut Vec::new());
    assert_eq!(
        got,
        Ok(Founded {
            seq: 2,
            granted: false
        })
    );
    assert_eq!(grants(&journal.inner).len(), 1);
}

/// A stream whose first event is not its opening is not founded.
#[test]
fn a_stream_not_opened_first_is_refused() {
    let mut journal = Journal::default();
    other(&mut journal, 1);
    let got = run(&mut journal, 1, "oidc").0;
    assert!(matches!(got, Err(ControlError::Journal(_))), "{got:?}");
    assert!(grants(&journal).is_empty());
}

/// A journal that answers every append `Committed` with no stored event, as a faulty adapter might.
struct EmptyAnswers(Journal);

impl ControlJournal for EmptyAnswers {
    fn rows(&self, stream: &StreamId) -> Result<Vec<StoredEvent>, ControlError> {
        self.0.rows(stream)
    }

    fn head(&self, stream: &StreamId) -> Result<mandate_journal::Head, ControlError> {
        self.0.head(stream)
    }

    fn take_ownership(&mut self, stream: &StreamId) -> Result<u64, ControlError> {
        self.0.take_ownership(stream)
    }

    fn append(
        &mut self,
        _: &StreamId,
        _: u64,
        _: u64,
        _: UtcNanos,
        _: &[&[u8]],
    ) -> Result<AppendOutcome, ControlError> {
        Ok(AppendOutcome::Committed(Vec::new()))
    }

    fn wait(&mut self, delay: std::time::Duration) {
        self.0.wait(delay);
    }
}

/// An answer of `Committed` that does not hold the grant's event is an error, never a grant.
#[test]
fn a_commit_answer_without_the_grant_is_not_a_grant() {
    let mut inner = Journal::default();
    open(&mut inner, "paper");
    let mut journal = EmptyAnswers(inner);
    let ulid = member(1);
    let founding = Founding {
        workspace: "ws1",
        member: &ulid,
        method: "oidc",
        open: false,
    };
    let got = found_in(&mut journal, founding, &mut ticking(), &mut Vec::new());
    assert!(matches!(got, Err(ControlError::Journal(_))), "{got:?}");
}
