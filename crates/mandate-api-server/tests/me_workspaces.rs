//! One's own memberships over the workspace API (§4.10; identity spec §4.5; DEC-2113 items 4 to 6):
//! the hosted deployment's membership index is its control stream's fold, and it answers the
//! caller's `active` membership with the roles in effect at the read, or nothing; a failed read is
//! `membership_unavailable`, never an empty list.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use common::control::{activated, record, roles};
use common::{ManualClock, MemJournal, Reply, WORKSPACE, at, hosted, send, serve};
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::members::ControlLookup;
use mandate_api_server::session::{Answer, COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Value, parse};
use mandate_identity::Role::{Approver, Auditor, Operator, Viewer, WorkspaceAdmin};
use mandate_identity::{LookupFailed, MembershipEvent, MembershipRecord, PrincipalId, Role};
use mandate_journal::StoredEvent;

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const BOB: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";
const CAROL: &str = "01JB3K6F0C9R2V7N4M8Q1T5W42";

type Records = Arc<Mutex<Result<Vec<MembershipRecord>, LookupFailed>>>;

fn principal(text: &str) -> PrincipalId {
    PrincipalId::from_ulid_text(text).expect("a ULID")
}

fn lookup(records: Vec<MembershipRecord>, now: i64) -> ControlLookup {
    ControlLookup::new(hosted(), Ok(records), at(now))
}

#[test]
fn ones_own_row_is_an_active_membership_with_the_roles_in_effect() {
    let alice = principal(ALICE);
    let joined = vec![activated(
        1,
        T0 - 1_000,
        alice,
        &[WorkspaceAdmin, Approver],
        T0 - 900,
    )];
    assert_eq!(
        lookup(joined.clone(), T0).own_roles(alice),
        Ok(Some(roles(&[WorkspaceAdmin, Approver])))
    );
    assert_eq!(
        lookup(joined.clone(), T0 - 901).own_roles(alice),
        Ok(None),
        "a membership still cooling off is not active"
    );
    assert_eq!(
        lookup(joined.clone(), T0 - 900).own_roles(alice),
        Ok(Some(roles(&[WorkspaceAdmin, Approver]))),
        "active from the instant its cool-off ends"
    );
    assert_eq!(
        lookup(joined.clone(), T0).own_roles(principal(BOB)),
        Ok(None),
        "no membership"
    );
    let mut granted = joined.clone();
    granted.push(record(
        2,
        T0 - 100,
        MembershipEvent::RoleChanged {
            member: alice,
            added: BTreeMap::from([(Operator, at(T0 + 50))]),
            removed: roles(&[WorkspaceAdmin, Approver]),
            independent_approval_required: false,
        },
    ));
    assert_eq!(
        lookup(granted.clone(), T0).own_roles(alice),
        Ok(Some(BTreeSet::new())),
        "every role still cooling off: the workspace, with no roles"
    );
    assert_eq!(
        lookup(granted, T0 + 50).own_roles(alice),
        Ok(Some(roles(&[Operator])))
    );
    let mut deactivated = joined;
    deactivated.push(record(
        2,
        T0 - 20,
        MembershipEvent::Deactivated { member: alice },
    ));
    assert_eq!(lookup(deactivated.clone(), T0).own_roles(alice), Ok(None));
    let mut removed = deactivated;
    removed.push(record(
        3,
        T0 - 10,
        MembershipEvent::Removed { member: alice },
    ));
    assert_eq!(lookup(removed, T0).own_roles(alice), Ok(None));
    let failed = ControlLookup::new(hosted(), Err(LookupFailed), at(T0));
    assert_eq!(failed.own_roles(alice), Err(LookupFailed));
}

struct Fixture {
    addr: std::net::SocketAddr,
    sessions: Arc<Sessions>,
    journal: Arc<MemJournal>,
    records: Records,
}

static DIRS: Mutex<u32> = Mutex::new(0);

fn fixture() -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-me-workspaces-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x6e_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    let journal = Arc::new(MemJournal::default());
    let records: Records = Arc::new(Mutex::new(Ok(Vec::new())));
    let parsed = Arc::clone(&records);
    let config = IssuerConfig::new(
        "https://issuer.test/auth/v1",
        &["authenticated"],
        &[Algorithm::Es256],
    )
    .expect("the issuer");
    let app = App {
        settings: Settings {
            app_origin: APP.to_owned(),
        },
        sessions: Arc::clone(&sessions),
        tokens: Arc::new(Issuer::new(config, Arc::new(FixedKeys(None)))),
        links: Arc::new(common::LinksAt(dir.join("absent"))),
        clock: ManualClock::at(T0) as Arc<dyn mandate_api_server::clock::Clock>,
        journal: Arc::clone(&journal) as Arc<dyn mandate_api_server::journal::Journal>,
        members: Arc::new(move |_: &[StoredEvent]| parsed.lock().expect("unpoisoned").clone()),
        hosted: hosted(),
        artifacts: None,
        objects: None,
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::default(),
        connect: None,
    };
    Fixture {
        addr: serve(http::api(app, Drain::new())),
        sessions,
        journal,
        records,
    }
}

fn open(f: &Fixture, who: &str) -> String {
    f.sessions
        .open(principal(who), "sha256:aaaa".to_owned(), None, at(T0))
        .expect("opened")
        .value()
        .to_owned()
}

fn mine(f: &Fixture, cookie: Option<&str>) -> Reply {
    let cookie = cookie.map(|c| format!("{COOKIE_NAME}={c}"));
    let headers: Vec<(&str, &str)> = cookie
        .as_deref()
        .map(|c| vec![("Cookie", c)])
        .unwrap_or_default();
    send(f.addr, "GET", "/v1/me/workspaces", &headers, b"")
}

fn body(reply: &Reply) -> Value {
    parse(&reply.body)
        .unwrap_or_else(|e| panic!("{e:?}: {:?}", String::from_utf8_lossy(&reply.body)))
}

fn json(text: &str) -> Value {
    parse(text.as_bytes()).expect("JSON")
}

fn listed(reply: &Reply) -> Value {
    assert_eq!(
        reply.status,
        200,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    let answer = body(reply);
    let members: BTreeSet<String> = match &answer {
        Value::Object(map) => map.keys().map(|k| k.as_str().to_owned()).collect(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        members,
        ["api_version", "as_of", "build", "served_at", "workspaces"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    assert_eq!(answer.get("as_of"), Some(&json("[]")), "no stream is named");
    answer.get("workspaces").cloned().expect("workspaces")
}

fn row(roles: &[Role]) -> Value {
    let codes: Vec<String> = roles
        .iter()
        .map(|r| format!(r#""{}""#, r.code().expect("a workspace role")))
        .collect();
    json(&format!(
        r#"[{{"workspace_id":"{WORKSPACE}","label":null,"roles":[{}]}}]"#,
        codes.join(",")
    ))
}

#[test]
fn each_caller_is_served_only_their_own_membership() {
    let f = fixture();
    *f.records.lock().expect("unpoisoned") = Ok(vec![
        activated(
            1,
            T0 - 1_000,
            principal(ALICE),
            &[Approver, WorkspaceAdmin],
            T0 - 900,
        ),
        activated(2, T0 - 1_000, principal(BOB), &[Viewer, Auditor], T0 - 900),
        activated(3, T0 - 10, principal(CAROL), &[Operator], T0 + 900),
    ]);
    assert_eq!(
        listed(&mine(&f, Some(&open(&f, ALICE)))),
        row(&[WorkspaceAdmin, Approver]),
        "roles in §4.1's order"
    );
    assert_eq!(
        listed(&mine(&f, Some(&open(&f, BOB)))),
        row(&[Viewer, Auditor])
    );
    assert_eq!(
        listed(&mine(&f, Some(&open(&f, CAROL)))),
        json("[]"),
        "cooling off is not active"
    );
    assert_eq!(
        listed(&mine(&f, Some(&open(&f, "01JB3K6F0C9R2V7N4M8Q1T5W43")))),
        json("[]"),
        "a stranger is told nothing, the workspace included"
    );
}

#[test]
fn an_unreadable_index_is_membership_unavailable_and_no_full_session_reads_it() {
    let f = fixture();
    let alice = open(&f, ALICE);
    let nowhere = send(f.addr, "GET", "/nowhere", &[], b"");
    let none = mine(&f, None);
    assert_eq!(
        (none.status, &none.body),
        (401, &nowhere.body),
        "the one 401"
    );
    let narrowed = open(&f, BOB);
    assert_eq!(
        f.sessions.refresh(&narrowed, Answer::Unreachable, at(T0)),
        Ok(None)
    );
    let reduced = mine(&f, Some(&narrowed));
    assert_eq!(reduced.status, 403);
    assert_eq!(
        body(&reduced).get("code"),
        Some(&json(r#""reduction_only""#))
    );
    *f.records.lock().expect("unpoisoned") = Err(LookupFailed);
    let unparsed = mine(&f, Some(&alice));
    assert_eq!(
        unparsed.status,
        503,
        "{:?}",
        String::from_utf8_lossy(&unparsed.body)
    );
    assert_eq!(
        body(&unparsed).get("code"),
        Some(&json(r#""membership_unavailable""#))
    );
    *f.records.lock().expect("unpoisoned") = Ok(vec![activated(
        1,
        T0 - 1_000,
        principal(ALICE),
        &[Viewer],
        T0 - 900,
    )]);
    f.journal
        .failing
        .lock()
        .expect("unpoisoned")
        .insert(format!("ctl:{WORKSPACE}"));
    let unread = mine(&f, Some(&alice));
    assert_eq!(unread.status, 503);
    assert_eq!(unread.body, unparsed.body, "one answer for either failure");
    f.journal.failing.lock().expect("unpoisoned").clear();
    assert_eq!(listed(&mine(&f, Some(&alice))), row(&[Viewer]));
}
