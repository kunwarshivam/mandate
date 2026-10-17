//! Lane ID1 (DEC-991; identity spec §3.2, §4.3, ID-13; journal spec §9.12): the founding grant.
//! `found_workspace` commits one `MemberActivated` with reason `founding` and the owner bundle,
//! through the one membership writer, only on a stream that holds its opening and nothing else; a
//! re-run for the same member finds it and commits nothing; any other membership refuses it.
//!
//! The property test drives random bootstrap runs, restarts (a lost answer after the commit),
//! racing bootstraps and other writers, and replays of the stored records, and checks every step
//! against an oracle of its own: a model of who founded the workspace, and invariants read off the
//! stored records directly, never through `founding_decision`. A seeded bug, a founding writer that
//! skips the decision, is shown to fail the same oracle.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use mandate_canon::{Value, parse};
use mandate_identity::{
    Appended, ControlEntry, ControlStream, ControlView, Founded, FoundingError, FoundingRefusal,
    InvitationId, MEMBERSHIP_TYPES, MembershipEvent as Event, MembershipFold, MembershipRecord,
    OWNER_BUNDLE, PrincipalId, Role, UnreadableRecord, WriteError, control_entry, found_workspace,
    founding_decision, founding_grant, write_membership,
};
use mandate_identity_seal::Seal;
use mandate_time::UtcNanos;
use proptest::prelude::*;

const EPOCH: i64 = 1_790_000_000;

fn t(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(EPOCH + secs, 0).unwrap()
}

fn member(n: u128) -> PrincipalId {
    PrincipalId(0x0191_0000_0000_0000_0000_0000_0000_0000 + n)
}

fn bundle() -> BTreeSet<Role> {
    [Role::Approver, Role::Operator, Role::WorkspaceAdmin].into()
}

fn record(seq: u64, at: UtcNanos, event: Event) -> MembershipRecord {
    MembershipRecord::new(Seal::grant(), seq, at, event)
}

/// What a store fault does to one append.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    /// Another bootstrap, for this member, commits its founding grant just before the append.
    RacingFounder(u128),
    /// Another writer commits a record of another type just before the append.
    RacingOther,
    /// The append commits and its answer is lost: the call fails, as a crash after the commit
    /// would.
    LostAnswer,
}

/// The control stream as a compare-and-append over its rows, `seq` from 1. `read` returns every
/// row, as the founding bootstrap's adapter does.
#[derive(Debug, Default)]
struct Stream {
    rows: Vec<ControlEntry>,
    faults: VecDeque<Fault>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Lost;

impl Stream {
    fn head(&self) -> u64 {
        self.rows.len() as u64
    }

    fn other(&mut self) {
        let seq = self.head() + 1;
        self.rows.push(ControlEntry::Other {
            seq,
            event_time: t(0),
        });
    }

    fn opened() -> Self {
        let mut stream = Self::default();
        stream.other();
        stream
    }

    /// The membership records, in `seq` order.
    fn memberships(&self) -> Vec<MembershipRecord> {
        self.rows
            .iter()
            .filter_map(|row| match row {
                ControlEntry::Membership(r) => Some(r.clone()),
                ControlEntry::Other { .. } => None,
            })
            .collect()
    }

    /// A racing bootstrap commits only where its own run would: an opened stream with nothing
    /// after its opening.
    fn race_founder(&mut self, n: u128) {
        if self.head() == 1 {
            let seq = self.head() + 1;
            let grant = founding_grant(member(n), t(1));
            self.rows
                .push(ControlEntry::Membership(record(seq, t(1), grant)));
        }
    }
}

impl ControlStream for Stream {
    type Error = Lost;

    fn read(&mut self) -> Result<ControlView, Lost> {
        Ok(ControlView {
            head: self.head(),
            tail: self.rows.clone(),
        })
    }

    fn append(&mut self, expected_head: u64, record: &MembershipRecord) -> Result<Appended, Lost> {
        let lost = match self.faults.pop_front() {
            Some(Fault::RacingFounder(n)) => {
                self.race_founder(n);
                false
            }
            Some(Fault::RacingOther) => {
                self.other();
                false
            }
            Some(Fault::LostAnswer) => true,
            None => false,
        };
        if expected_head != self.head() || record.seq() != expected_head + 1 {
            return Ok(Appended::HeadMoved);
        }
        self.rows.push(ControlEntry::Membership(record.clone()));
        if lost {
            Err(Lost)
        } else {
            Ok(Appended::Committed)
        }
    }
}

fn found(stream: &mut Stream, n: u128, at: i64) -> Result<Founded, FoundingError<Lost>> {
    found_workspace(stream, || t(at), member(n))
}

/// The owner bundle is exactly workspace admin, operator, and approver (identity spec §4.3).
#[test]
fn the_owner_bundle_is_admin_operator_and_approver() {
    let named: BTreeSet<Role> = OWNER_BUNDLE.into_iter().collect();
    assert_eq!(named, bundle());
    assert_eq!(OWNER_BUNDLE.len(), 3, "no role is listed twice");
}

/// The grant is an activation with no invitation, the owner bundle, no independence, and no
/// cool-off: its cool-off ends at its own instant (rules 99, 100, and 103).
#[test]
fn the_founding_grant_is_the_owner_bundle_with_no_cool_off() {
    assert_eq!(
        founding_grant(member(1), t(5)),
        Event::Activated {
            member: member(1),
            invitation: None,
            roles: bundle(),
            independent_approval_required: false,
            cool_off_ends_at: t(5),
        }
    );
}

/// On an opened stream holding nothing else, the grant commits at seq 2, stamped by the clock.
#[test]
fn an_opened_stream_is_founded_once() {
    let mut stream = Stream::opened();
    let got = found(&mut stream, 1, 7);
    let want = record(2, t(7), founding_grant(member(1), t(7)));
    assert_eq!(got, Ok(Founded::Granted(want.clone())));
    assert_eq!(stream.memberships(), vec![want]);
}

/// A re-run for the same member finds the grant and commits nothing.
#[test]
fn a_re_run_for_the_founder_finds_the_grant() {
    let mut stream = Stream::opened();
    let Ok(Founded::Granted(granted)) = found(&mut stream, 1, 7) else {
        panic!("not granted")
    };
    stream.other();
    let head = stream.head();
    assert_eq!(
        found(&mut stream, 1, 9),
        Ok(Founded::AlreadyFounded(granted))
    );
    assert_eq!(stream.head(), head, "nothing committed");
}

/// Another member is refused once any membership exists, and nothing is committed.
#[test]
fn another_member_is_refused_once_one_is_founded() {
    let mut stream = Stream::opened();
    found(&mut stream, 1, 7).unwrap();
    let head = stream.head();
    assert_eq!(
        found(&mut stream, 2, 9),
        Err(FoundingError::Refused(FoundingRefusal::MembersExist))
    );
    assert_eq!(stream.head(), head);
}

/// A first membership record that is not a founding grant refuses even its own member.
#[test]
fn a_membership_other_than_a_founding_grant_refuses() {
    let mut stream = Stream::opened();
    let accepted = Event::Activated {
        member: member(1),
        invitation: Some(InvitationId(4)),
        roles: [Role::Viewer].into(),
        independent_approval_required: false,
        cool_off_ends_at: t(1),
    };
    stream
        .rows
        .push(ControlEntry::Membership(record(2, t(1), accepted)));
    assert_eq!(
        found(&mut stream, 1, 7),
        Err(FoundingError::Refused(FoundingRefusal::MembersExist))
    );
    let narrower = Event::Activated {
        member: member(2),
        invitation: None,
        roles: [Role::WorkspaceAdmin].into(),
        independent_approval_required: false,
        cool_off_ends_at: t(1),
    };
    let mut stream = Stream::opened();
    stream
        .rows
        .push(ControlEntry::Membership(record(2, t(1), narrower)));
    assert_eq!(
        found(&mut stream, 2, 7),
        Err(FoundingError::Refused(FoundingRefusal::MembersExist)),
        "a founding record that is not the owner bundle is not this grant"
    );
}

/// A stream with no record is not open; one with a record after its opening and no membership is
/// not new (DEC-991 item 3). Neither commits anything.
#[test]
fn an_unopened_or_used_stream_is_refused() {
    let mut stream = Stream::default();
    assert_eq!(
        found(&mut stream, 1, 7),
        Err(FoundingError::Refused(FoundingRefusal::NotOpen))
    );
    assert_eq!(stream.head(), 0);
    let mut stream = Stream::opened();
    stream.other();
    assert_eq!(
        found(&mut stream, 1, 7),
        Err(FoundingError::Refused(FoundingRefusal::NotNew))
    );
    assert_eq!(stream.head(), 2);
}

/// Two bootstraps racing: the second's append finds the head moved, re-reads, and finds the
/// first's grant, so it grants nothing.
#[test]
fn a_racing_founder_wins_and_the_loser_grants_nothing() {
    let mut stream = Stream::opened();
    stream.faults.push_back(Fault::RacingFounder(2));
    assert_eq!(
        found(&mut stream, 1, 7),
        Err(FoundingError::Refused(FoundingRefusal::MembersExist))
    );
    assert_eq!(stream.memberships().len(), 1);
    let mut stream = Stream::opened();
    stream.faults.push_back(Fault::RacingFounder(1));
    assert!(matches!(
        found(&mut stream, 1, 7),
        Ok(Founded::AlreadyFounded(_))
    ));
    assert_eq!(stream.memberships().len(), 1);
}

/// An answer lost after the commit is a store error; the restart finds the grant.
#[test]
fn a_restart_after_a_lost_answer_finds_the_grant() {
    let mut stream = Stream::opened();
    stream.faults.push_back(Fault::LostAnswer);
    assert_eq!(
        found(&mut stream, 1, 7),
        Err(FoundingError::Write(WriteError::Store(Lost)))
    );
    assert!(matches!(
        found(&mut stream, 1, 8),
        Ok(Founded::AlreadyFounded(r)) if r.event_time() == t(7)
    ));
    assert_eq!(stream.memberships().len(), 1);
}

/// The decision reads the head and the records it is given, and nothing else.
#[test]
fn the_decision_reads_the_view() {
    let view = |head, tail| ControlView { head, tail };
    let opening = ControlEntry::Other {
        seq: 1,
        event_time: t(0),
    };
    assert_eq!(
        founding_decision(&view(0, vec![]), member(1)),
        Err(FoundingRefusal::NotOpen)
    );
    assert_eq!(
        founding_decision(&view(1, vec![opening.clone()]), member(1)),
        Ok(None)
    );
    assert_eq!(
        founding_decision(&view(3, vec![opening.clone()]), member(1)),
        Err(FoundingRefusal::NotNew),
        "a head past the opening with no membership is not new"
    );
    let grant = record(2, t(1), founding_grant(member(1), t(1)));
    let tail = vec![opening, ControlEntry::Membership(grant.clone())];
    assert_eq!(
        founding_decision(&view(2, tail.clone()), member(1)),
        Ok(Some(grant))
    );
    assert_eq!(
        founding_decision(&view(2, tail), member(2)),
        Err(FoundingRefusal::MembersExist)
    );
}

/// The refusals' stable codes.
#[test]
fn the_refusals_have_stable_codes() {
    assert_eq!(FoundingRefusal::NotOpen.code(), "workspace_not_open");
    assert_eq!(FoundingRefusal::NotNew.code(), "workspace_not_new");
    assert_eq!(FoundingRefusal::MembersExist.code(), "members_exist");
}

/// §9.12's `role` type names the five workspace roles and no org role.
#[test]
fn role_codes_are_the_workspace_roles() {
    let codes: BTreeMap<&str, Role> = Role::ALL
        .into_iter()
        .filter_map(|role| role.code().map(|code| (code, role)))
        .collect();
    let want: BTreeMap<&str, Role> = [
        ("approver", Role::Approver),
        ("auditor", Role::Auditor),
        ("operator", Role::Operator),
        ("viewer", Role::Viewer),
        ("workspace_admin", Role::WorkspaceAdmin),
    ]
    .into();
    assert_eq!(codes, want);
    for (code, role) in want {
        assert_eq!(Role::from_code(code), Some(role));
    }
    for other in ["org_owner", "org_admin", "billing_admin", "Approver", ""] {
        assert_eq!(Role::from_code(other), None, "{other}");
    }
}

fn fixture_section(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get(name).cloned().unwrap()
}

fn base(name: &str) -> Value {
    let section = fixture_section("membership");
    section
        .get("drafts")
        .and_then(|d| d.get(name))
        .cloned()
        .unwrap()
}

fn ulid(text: &str) -> u128 {
    mandate_canon::decode_ulid(text).unwrap()
}

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse(text).unwrap()
}

/// Every base draft of the vectors' `membership` section maps to the record its payload says,
/// written out here from the fixture's values.
#[test]
fn each_membership_draft_maps_to_its_record() {
    let admin = PrincipalId(ulid("01J8Z4M0AD0000000000000AD1"));
    let invitee = PrincipalId(ulid("01J8Z4M0BE0000000000000ME1"));
    let invitation = InvitationId(ulid("01J8Z4M0C00000000000000N01"));
    let instant = at("2026-10-05T14:00:00.000000000Z");
    let day_later = at("2026-10-06T14:00:00.000000000Z");
    let approver_viewer: BTreeSet<Role> = [Role::Approver, Role::Viewer].into();
    let cases = [
        (
            "invited",
            Event::Invited {
                invitation,
                roles: approver_viewer.clone(),
                expires_at: at("2026-10-12T14:00:00.000000000Z"),
            },
        ),
        (
            "invitation_revoked",
            Event::InvitationRevoked { invitation },
        ),
        (
            "activated",
            Event::Activated {
                member: invitee,
                invitation: Some(invitation),
                roles: approver_viewer.clone(),
                independent_approval_required: true,
                cool_off_ends_at: day_later,
            },
        ),
        (
            "founding",
            Event::Activated {
                member: admin,
                invitation: None,
                roles: bundle(),
                independent_approval_required: false,
                cool_off_ends_at: instant,
            },
        ),
        (
            "role_changed",
            Event::RoleChanged {
                member: invitee,
                added: [(Role::Auditor, instant), (Role::Operator, day_later)].into(),
                removed: [Role::Viewer].into(),
                independent_approval_required: true,
            },
        ),
        ("deactivated", Event::Deactivated { member: invitee }),
        (
            "reactivated",
            Event::Reactivated {
                member: invitee,
                roles: approver_viewer,
                independent_approval_required: true,
                cool_off_ends_at: day_later,
            },
        ),
        ("removed", Event::Removed { member: invitee }),
    ];
    for (name, event) in cases {
        let got = control_entry(4, &base(name));
        let want = ControlEntry::Membership(record(4, instant, event));
        assert_eq!(got, Ok(want), "{name}");
    }
}

fn with(draft: &Value, path: &str, value: Value) -> Value {
    let mut draft = draft.clone();
    let mut node = &mut draft;
    let names: Vec<&str> = path.split('.').collect();
    let (last, parents) = names.split_last().unwrap();
    for name in parents {
        node = match node {
            Value::Object(o) => o.get_mut(*name).unwrap(),
            _ => panic!("{path}"),
        };
    }
    let Value::Object(o) = node else {
        panic!("{path}")
    };
    o.insert(mandate_canon::Key::new(last).unwrap(), value);
    draft
}

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

/// A membership record whose members do not map is unreadable at the member at fault, never
/// skipped; a record of any other type is `Other`, with its `seq` and `event_time`.
#[test]
fn an_unmappable_membership_record_is_unreadable() {
    let founding = base("founding");
    let roles = |codes: &[&str]| Value::Array(codes.iter().map(|c| text(c)).collect());
    let cases = [
        ("payload.member", text("not-a-ulid"), "payload.member"),
        ("payload.member", Value::Null, "payload.member"),
        (
            "payload.roles",
            roles(&["approver", "operator", "org_owner"]),
            "payload.roles",
        ),
        (
            "payload.roles",
            roles(&["approver", "approver", "workspace_admin"]),
            "payload.roles",
        ),
        (
            "payload.roles",
            roles(&["operator", "approver", "workspace_admin"]),
            "payload.roles",
        ),
        (
            "payload.invitation",
            text("01J8Z4M0C00000000000000N01"),
            "payload.invitation",
        ),
        ("payload.reason", text("accepted"), "payload.reason"),
        (
            "payload.independent_approval_required",
            text("false"),
            "payload.independent_approval_required",
        ),
        (
            "payload.cool_off_ends_at",
            text("2026-10-05T14:00:00Z"),
            "payload.cool_off_ends_at",
        ),
        ("event_time", text("yesterday"), "event_time"),
    ];
    for (path, value, member) in cases {
        let got = control_entry(9, &with(&founding, path, value.clone()));
        assert_eq!(
            got,
            Err(UnreadableRecord { seq: 9, member }),
            "{path} = {value:?}"
        );
    }
    let accepted = with(&base("activated"), "payload.invitation", Value::Null);
    assert_eq!(
        control_entry(9, &accepted),
        Err(UnreadableRecord {
            seq: 9,
            member: "payload.invitation"
        }),
        "an accepted invitation names its invitation"
    );
    let stripped = with(&founding, "payload", Value::Null);
    assert!(control_entry(9, &stripped).is_err());
    let opened = with(&founding, "event_type", text("StreamOpened"));
    assert_eq!(
        control_entry(9, &opened),
        Ok(ControlEntry::Other {
            seq: 9,
            event_time: at("2026-10-05T14:00:00.000000000Z")
        })
    );
    let untyped = with(&founding, "event_type", Value::Null);
    assert_eq!(
        control_entry(9, &untyped),
        Err(UnreadableRecord {
            seq: 9,
            member: "event_type"
        })
    );
}

/// The seven types are §9.12's, and only they map to membership records.
#[test]
fn the_membership_types_are_section_nine_twelves() {
    let section = fixture_section("membership");
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let named: BTreeSet<&str> = drafts
        .values()
        .filter_map(|d| d.get("event_type").and_then(Value::as_str))
        .collect();
    let listed: BTreeSet<&str> = MEMBERSHIP_TYPES.into_iter().collect();
    assert_eq!(named, listed);
}

/// One step of a random history.
#[derive(Debug, Clone)]
enum Step {
    /// `mandate workspace open`: the stream's opening, once.
    Open,
    /// Another writer's record of another type.
    Other,
    /// A bootstrap run for member `n`, at `at`, with the faults its appends meet.
    Bootstrap {
        n: u128,
        at: i64,
        faults: Vec<Fault>,
    },
    /// A replay: the stored membership records folded afresh.
    Replay,
}

fn step() -> impl Strategy<Value = Step> {
    let fault = prop_oneof![
        (0_u128..3).prop_map(Fault::RacingFounder),
        Just(Fault::RacingOther),
        Just(Fault::LostAnswer),
    ];
    prop_oneof![
        1 => Just(Step::Open),
        1 => Just(Step::Other),
        5 => (0_u128..3, 1_i64..1_000, prop::collection::vec(fault, 0..3))
            .prop_map(|(n, at, faults)| Step::Bootstrap { n, at, faults }),
        2 => Just(Step::Replay),
    ]
}

/// The oracle's own model: whether the stream is open, whether a record of another type follows
/// its opening, and who founded it.
#[derive(Debug, Default)]
struct Model {
    open: bool,
    used: bool,
    founder: Option<u128>,
}

impl Model {
    /// A racing bootstrap of `n` commits where the model says a run would.
    fn race(&mut self, n: u128) {
        if self.open && !self.used && self.founder.is_none() {
            self.founder = Some(n);
        }
    }

    /// What a run of `n` meeting `faults` answers, and the model after it. A fault happens only
    /// at an append, and a run appends only when it would grant; after a race the run re-reads
    /// and decides again, and after a lost answer it has committed.
    fn run(&mut self, n: u128, faults: &[Fault]) -> Expected {
        for fault in faults {
            if self.expect(n) != Expected::Granted {
                return self.expect(n);
            }
            match fault {
                Fault::RacingFounder(k) => self.race(*k),
                Fault::RacingOther => self.used = true,
                Fault::LostAnswer => {
                    self.founder = Some(n);
                    return Expected::Lost;
                }
            }
        }
        let expected = self.expect(n);
        if expected == Expected::Granted {
            self.founder = Some(n);
        }
        expected
    }

    /// What a run of `n` answers, given the model as the run finds it.
    fn expect(&self, n: u128) -> Expected {
        match self.founder {
            Some(f) if f == n => Expected::Already,
            Some(_) => Expected::Refused(FoundingRefusal::MembersExist),
            None if !self.open => Expected::Refused(FoundingRefusal::NotOpen),
            None if self.used => Expected::Refused(FoundingRefusal::NotNew),
            None => Expected::Granted,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expected {
    Granted,
    Already,
    Lost,
    Refused(FoundingRefusal),
}

/// The invariants, read off the stored rows directly: at most one membership record; it is a
/// founding grant of exactly the owner bundle, with no independence and no cool-off; its member
/// is the model's founder; and the fold of the records grants that member the owner bundle and
/// no one anything else.
fn check(stream: &Stream, model: &Model) -> Result<(), String> {
    let memberships = stream.memberships();
    if memberships.len() > 1 {
        return Err(format!("{} membership records", memberships.len()));
    }
    let founder = memberships.first().map(|r| match r.event() {
        Event::Activated {
            member,
            invitation: None,
            roles,
            independent_approval_required: false,
            cool_off_ends_at,
        } if *roles == bundle() && *cool_off_ends_at == r.event_time() => Ok(*member),
        other => Err(format!("outside the founding scope: {other:?}")),
    });
    let founder = founder.transpose()?;
    if founder != model.founder.map(member) {
        return Err(format!("founded {founder:?}, model {:?}", model.founder));
    }
    let fold = MembershipFold::new(Seal::grant(), memberships);
    let later = t(1_000_000);
    if fold.unreadable(later) {
        return Err("the fold is unreadable".to_owned());
    }
    for n in 0..3 {
        let roles = fold.effective_roles(member(n), later);
        let want = if founder == Some(member(n)) {
            bundle()
        } else {
            BTreeSet::new()
        };
        if roles != want {
            return Err(format!("member {n} holds {roles:?}"));
        }
    }
    let users = u32::from(founder.is_some());
    if fold.workspace_users(later) != users {
        return Err(format!("{} users", fold.workspace_users(later)));
    }
    Ok(())
}

/// A run's answer as the oracle classifies it.
fn classify(got: &Result<Founded, FoundingError<Lost>>) -> Expected {
    match got {
        Ok(Founded::Granted(_)) => Expected::Granted,
        Ok(Founded::AlreadyFounded(_)) => Expected::Already,
        Err(FoundingError::Write(WriteError::Store(Lost))) => Expected::Lost,
        Err(FoundingError::Refused(why)) => Expected::Refused(*why),
        Err(FoundingError::Write(other)) => panic!("{other:?}"),
    }
}

/// Runs `steps` with `run` as the bootstrap, checking every step against the oracle.
fn history(
    steps: &[Step],
    run: impl Fn(&mut Stream, u128, i64) -> Result<Founded, FoundingError<Lost>>,
) -> Result<(), String> {
    let (mut stream, mut model) = (Stream::default(), Model::default());
    for step in steps {
        match step {
            Step::Open if !model.open => {
                stream.other();
                model.open = true;
            }
            Step::Open | Step::Replay => {}
            Step::Other if model.open => {
                stream.other();
                model.used |= model.founder.is_none();
            }
            Step::Other => {}
            Step::Bootstrap { n, at, faults } => {
                stream.faults = faults.iter().copied().collect();
                let expected = model.run(*n, faults);
                let got = classify(&run(&mut stream, *n, *at));
                if got != expected {
                    return Err(format!("{step:?}: got {got:?}, want {expected:?}"));
                }
                stream.faults.clear();
            }
        }
        if model.open && stream.head() == 0 {
            return Err("the model is open and the stream is empty".to_owned());
        }
        check(&stream, &model).map_err(|why| format!("after {step:?}: {why}"))?;
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, failure_persistence: None, ..ProptestConfig::default() })]

    /// No sequence of bootstrap runs, restarts, races, and replays yields two founders or a grant
    /// outside the founding scope, and each run answers what the oracle's model says.
    #[test]
    fn no_history_yields_two_founders_or_a_wider_grant(steps in prop::collection::vec(step(), 1..24)) {
        let result = history(&steps, |stream, n, at| found(stream, n, at));
        prop_assert!(result.is_ok(), "{result:?}");
    }
}

/// The oracle is not vacuous: a founding writer that skips the decision and writes the grant
/// through the plain membership writer founds twice, which the oracle reports.
#[test]
fn the_oracle_catches_a_writer_that_skips_the_decision() {
    let skipping = |stream: &mut Stream, n: u128, at: i64| {
        write_membership(stream, || t(at), founding_grant(member(n), t(at)))
            .map(Founded::Granted)
            .map_err(FoundingError::Write)
    };
    let steps = [
        Step::Open,
        Step::Bootstrap {
            n: 1,
            at: 5,
            faults: vec![],
        },
        Step::Bootstrap {
            n: 2,
            at: 6,
            faults: vec![],
        },
    ];
    assert!(history(&steps, skipping).is_err());
    assert!(history(&steps, |s, n, at| found(s, n, at)).is_ok());
}

/// The oracle reads the grant's scope too: a writer granting a wider role set is reported.
#[test]
fn the_oracle_catches_a_wider_grant() {
    let wider = |stream: &mut Stream, n: u128, at: i64| {
        let mut roles = bundle();
        roles.insert(Role::Auditor);
        let event = Event::Activated {
            member: member(n),
            invitation: None,
            roles,
            independent_approval_required: false,
            cool_off_ends_at: t(at),
        };
        write_membership(stream, || t(at), event)
            .map(Founded::Granted)
            .map_err(FoundingError::Write)
    };
    let steps = [
        Step::Open,
        Step::Bootstrap {
            n: 1,
            at: 5,
            faults: vec![],
        },
    ];
    assert!(history(&steps, wider).is_err());
}
