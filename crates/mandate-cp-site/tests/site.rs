//! Workspace control services' side of the control-plane link (E20-3, DEC-983), end to end through
//! `cp-agent`'s [`receive`] into a [`Site`] over the journal's own [`MemoryJournal`], so every record
//! is judged by `append`'s §9.17 check:
//!
//! - CP-5 (b): a verified instruction is journaled on each covered workspace's control stream before
//!   its effect, and a refused message is journaled once;
//! - CP-9, once per kind (license, release manifest, catalog entry, data bundle): an older or
//!   conflicting sequence is refused and journaled `replayed`, with no effect;
//! - CP-3: a hung effect holds nothing, so an owner's kill switch commits meanwhile;
//! - CP-4: whatever arrives, only §9.17 records are ever appended.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use mandate_canon::{Digest, Key, Object, Value, encode_ulid, parse, to_canonical};
use mandate_cp_agent::{
    HandOver, Ident, KeyKind, MessageKind, PinnedKeys, RefusalReason, Site as Me, Verified,
    receive, signed_bytes,
};
use mandate_cp_site::{ControlStreams, Effects, Site, Writer};
use mandate_journal::{AppendOutcome, MemoryJournal, StreamId};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use ring::signature::{Ed25519KeyPair, KeyPair};

const DEPLOYMENT: &str = "dep_01JCELL1";
const ORG_A: &str = "org_01JACME";
const ORG_B: &str = "org_01JBETA";
const T: &str = "2026-10-17T13:05:00.000000000Z";

/// §9.17's ten records: the only event types `cp-agent`'s hand-overs may ever append (CP-4).
const SECTION_9_17: [&str; 10] = [
    "ControlPlaneEnrolled",
    "LicenseApplied",
    "LicenseStateChanged",
    "ReleaseOffered",
    "ReleaseInstalled",
    "ReleaseWithdrawnNoticed",
    "CatalogEntryRegistered",
    "DataBundleImported",
    "UsageReportSealed",
    "ControlPlaneMessageRefused",
];

fn id(text: &str) -> Ident {
    Ident::parse(text).unwrap()
}

fn pair(seed: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap()
}

fn keys() -> PinnedKeys {
    PinnedKeys::new()
        .pin(
            &id("license_key_1"),
            KeyKind::License,
            pair(1).public_key().as_ref(),
        )
        .and_then(|k| {
            k.pin(
                &id("release_key_1"),
                KeyKind::Release,
                pair(2).public_key().as_ref(),
            )
        })
        .and_then(|k| {
            k.pin(
                &id("registry_key_1"),
                KeyKind::Registry,
                pair(3).public_key().as_ref(),
            )
        })
        .and_then(|k| {
            k.pin(
                &id("data_key_1"),
                KeyKind::Data,
                pair(4).public_key().as_ref(),
            )
        })
        .unwrap()
}

fn me() -> Me {
    Me {
        deployment_id: id(DEPLOYMENT),
    }
}

/// The control streams of a cell hosting `ws_a1` and `ws_a2` of organization A and `ws_b1` of B,
/// each opened, as one journal behind a lock that each call takes and releases.
#[derive(Clone)]
struct Streams {
    journal: Arc<Mutex<MemoryJournal>>,
    epochs: BTreeMap<String, u64>,
    hosted: Vec<(Ident, Ident)>,
    down: Option<String>,
}

fn stream(workspace: &str) -> StreamId {
    StreamId::parse(&format!("ctl:{workspace}")).unwrap()
}

fn opened(workspace: &str, n: u128) -> Vec<u8> {
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"ctl:{workspace}",
        "event_type":"StreamOpened","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":null,"correlation_id":null,
        "actor":{{"kind":"system","id":"control_services","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{}},"payload":{{"stream_type":"control","workspace_id":"{workspace}"}},
        "artifact_refs":[],"pii_refs":[]}}"#,
        encode_ulid(n),
        "d".repeat(64)
    )
    .into_bytes()
}

impl Streams {
    fn new() -> Self {
        let mut journal = MemoryJournal::new();
        let mut epochs = BTreeMap::new();
        let hosted = [("ws_a1", ORG_A), ("ws_a2", ORG_A), ("ws_b1", ORG_B)];
        for (i, (workspace, _)) in hosted.iter().enumerate() {
            let epoch = journal.take_ownership(&stream(workspace));
            let n = u128::try_from(i).unwrap() + 1;
            let outcome = journal.append(
                &stream(workspace),
                0,
                epoch,
                now(),
                &[&opened(workspace, n)],
            );
            assert!(
                matches!(outcome, AppendOutcome::Committed(_)),
                "{outcome:?}"
            );
            epochs.insert((*workspace).to_owned(), epoch);
        }
        Self {
            journal: Arc::new(Mutex::new(journal)),
            epochs,
            hosted: hosted.iter().map(|(w, o)| (id(w), id(o))).collect(),
            down: None,
        }
    }

    fn types(&self, workspace: &str) -> Vec<String> {
        let journal = self.journal.lock().unwrap();
        journal
            .rows(&stream(workspace))
            .iter()
            .skip(1)
            .map(|r| r.event_type.clone())
            .collect()
    }

    fn payloads(&self, workspace: &str) -> Vec<Value> {
        let journal = self.journal.lock().unwrap();
        journal
            .rows(&stream(workspace))
            .iter()
            .skip(1)
            .map(|r| parse(&r.body).unwrap().get("payload").cloned().unwrap())
            .collect()
    }
}

impl ControlStreams for Streams {
    fn hosted(&self) -> Vec<(Ident, Ident)> {
        self.hosted.clone()
    }

    fn records(&self, workspace: &Ident) -> Option<Vec<(String, Value)>> {
        if self.down.as_deref() == Some(workspace.as_str()) {
            return None;
        }
        let journal = self.journal.lock().unwrap();
        Some(
            journal
                .rows(&stream(workspace.as_str()))
                .iter()
                .map(|r| {
                    let body = parse(&r.body).unwrap();
                    (r.event_type.clone(), body.get("payload").cloned().unwrap())
                })
                .collect(),
        )
    }

    fn append(&mut self, workspace: &Ident, draft: &[u8]) -> bool {
        if self.down.as_deref() == Some(workspace.as_str()) {
            return false;
        }
        let mut journal = self.journal.lock().unwrap();
        let s = stream(workspace.as_str());
        let head = journal.head(&s).seq;
        let epoch = self.epochs[workspace.as_str()];
        let outcome = journal.append(&s, head, epoch, now(), &[draft]);
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "the journal accepts every record the site writes: {outcome:?}\n{}",
            String::from_utf8_lossy(draft)
        );
        true
    }
}

/// Every effect, with the stream's §9.17 records as they stood when it ran.
#[derive(Clone, Default)]
struct Applied {
    seen: Arc<Mutex<Vec<(String, MessageKind, usize)>>>,
    streams: Option<Arc<Mutex<MemoryJournal>>>,
}

impl Effects for Applied {
    fn apply(&mut self, workspace: &Ident, verified: &Verified) {
        let rows = self.streams.as_ref().map_or(0, |j| {
            j.lock().unwrap().rows(&stream(workspace.as_str())).len()
        });
        self.seen.lock().unwrap().push((
            workspace.as_str().to_owned(),
            verified.message_kind(),
            rows,
        ));
    }
}

fn now() -> UtcNanos {
    UtcNanos::parse(T).unwrap()
}

type TestSite = Site<Streams, Applied, fn() -> String, fn() -> UtcNanos>;

/// A fresh event ID for every record any site in this test binary writes.
fn next_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    encode_ulid((1u128 << 80) + u128::from(NEXT.fetch_add(1, Ordering::Relaxed)))
}

fn site(streams: Streams) -> TestSite {
    let applied = Applied {
        seen: Arc::default(),
        streams: Some(streams.journal.clone()),
    };
    Site {
        streams,
        effects: applied,
        writer: Writer {
            id: "control_services".to_owned(),
            version: "0.1.0".to_owned(),
            build: Digest::of(b"control services build"),
            environment: "paper",
        },
        ids: next_id,
        clock: now,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The canonical envelope of `members`, each `(name, JSON)`.
fn envelope(members: &[(&str, &str)]) -> Vec<u8> {
    let mut object = Object::new();
    for (name, json) in members {
        object.insert(Key::new(name).unwrap(), parse(json.as_bytes()).unwrap());
    }
    to_canonical(&Value::Object(object))
}

fn message(kind: MessageKind, key_id: &str, body: &str, signer: &Ed25519KeyPair) -> Vec<u8> {
    let parsed = parse(body.as_bytes()).unwrap();
    let signature = signer.sign(&signed_bytes(kind, key_id, &parsed));
    envelope(&[
        ("kind", &format!("\"{}\"", kind.as_str())),
        ("key_id", &format!("\"{key_id}\"")),
        ("body", body),
        ("signature", &format!("\"{}\"", hex(signature.as_ref()))),
    ])
}

fn license(org: &str, sequence: u64, grace_days: u64) -> Vec<u8> {
    let body = format!(
        r#"{{"license_id":"lic_{sequence}","sequence":{sequence},"org_id":"{org}",
        "deployment_ids":["{DEPLOYMENT}"],"max_workspaces":5,"max_agents_deployed":20,
        "features":["connected_clients"],"not_before":"2026-10-01T00:00:00.000000000Z",
        "not_after":"2027-10-01T00:00:00.000000000Z","grace_days":{grace_days}}}"#
    );
    message(MessageKind::License, "license_key_1", &body, &pair(1))
}

fn manifest(sequence: u64, component: &str) -> Vec<u8> {
    let body = format!(
        r#"{{"release_id":"rel_{sequence}","sequence":{sequence},"images":[{{"component":"{component}","digest":"{}"}}]}}"#,
        "c".repeat(64)
    );
    message(
        MessageKind::ReleaseManifest,
        "release_key_1",
        &body,
        &pair(2),
    )
}

fn catalog(kind: &str, sequence: u64, fill: char) -> Vec<u8> {
    let body = format!(
        r#"{{"kind":"{kind}","entry_id":"entry_1","content_hash":"{}","sequence":{sequence}}}"#,
        fill.to_string().repeat(64)
    );
    let (key, signer) = if kind == "model_registry_entry" {
        ("registry_key_1", pair(3))
    } else {
        ("release_key_1", pair(2))
    };
    message(MessageKind::CatalogEntry, key, &body, &signer)
}

fn bundle(dataset: &str, sequence: u64, fill: char) -> Vec<u8> {
    let body = format!(
        r#"{{"dataset":"{dataset}","bundle_digest":"{}","sequence":{sequence}}}"#,
        fill.to_string().repeat(64)
    );
    message(MessageKind::DataBundle, "data_key_1", &body, &pair(4))
}

fn deliver(site: &mut TestSite, bytes: &[u8]) -> HandOver {
    receive(bytes, &keys(), &me(), site)
}

#[test]
fn a_license_is_journaled_on_its_organizations_workspaces_before_its_effect() {
    let streams = Streams::new();
    let mut site = site(streams.clone());
    assert_eq!(
        deliver(&mut site, &license(ORG_A, 3, 30)),
        HandOver::Journaled
    );
    assert_eq!(streams.types("ws_a1"), ["LicenseApplied"]);
    assert_eq!(streams.types("ws_a2"), ["LicenseApplied"]);
    assert!(
        streams.types("ws_b1").is_empty(),
        "another organization's workspace holds nothing"
    );
    let seen = site.effects.seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        [
            ("ws_a1".to_owned(), MessageKind::License, 2),
            ("ws_a2".to_owned(), MessageKind::License, 2),
        ],
        "each effect ran after its stream held the record"
    );
    let payload = &streams.payloads("ws_a1")[0];
    assert_eq!(payload.get("sequence").and_then(Value::as_int), Some(3));
    assert_eq!(
        payload.get("key_id").and_then(Value::as_str),
        Some("license_key_1")
    );
    let digest = Digest::of(&license(ORG_A, 3, 30)).to_hex();
    assert_eq!(
        payload.get("document_digest").and_then(Value::as_str),
        Some(digest.as_str())
    );
}

/// CP-9 once per kind: the instruction in effect again journals nothing; a lower sequence, or the
/// same sequence with other content, is refused `replayed` and journaled once, with no effect; a
/// higher one is journaled.
#[test]
fn every_kind_refuses_a_replay_and_journals_the_refusal() {
    type Case = (&'static str, fn(u64, bool) -> Vec<u8>);
    let cases: [Case; 4] = [
        ("LicenseApplied", |s, other| {
            license(ORG_A, s, if other { 7 } else { 30 })
        }),
        ("ReleaseOffered", |s, other| {
            manifest(s, if other { "web" } else { "api" })
        }),
        ("CatalogEntryRegistered", |s, other| {
            catalog("connector_package", s, if other { 'b' } else { 'a' })
        }),
        ("DataBundleImported", |s, other| {
            bundle("sec_filings", s, if other { 'b' } else { 'a' })
        }),
    ];
    for (record, make) in cases {
        let streams = Streams::new();
        let mut site = site(streams.clone());
        assert_eq!(
            deliver(&mut site, &make(5, false)),
            HandOver::Journaled,
            "{record}"
        );
        assert_eq!(
            deliver(&mut site, &make(5, false)),
            HandOver::AlreadyApplied,
            "{record}"
        );
        let replays = [make(4, false), make(5, true)];
        for replay in &replays {
            assert_eq!(
                deliver(&mut site, replay),
                HandOver::Refused(RefusalReason::Replayed),
                "{record}"
            );
            assert_eq!(
                deliver(&mut site, replay),
                HandOver::AlreadyApplied,
                "{record}: once"
            );
        }
        assert_eq!(
            deliver(&mut site, &make(6, false)),
            HandOver::Journaled,
            "{record}"
        );
        let types = streams.types("ws_a1");
        assert_eq!(
            types,
            [
                record,
                "ControlPlaneMessageRefused",
                "ControlPlaneMessageRefused",
                record
            ],
            "{record}"
        );
        let refusal = &streams.payloads("ws_a1")[1];
        assert_eq!(
            refusal.get("reason").and_then(Value::as_str),
            Some("replayed")
        );
        assert_eq!(refusal.get("sequence").and_then(Value::as_int), Some(4));
        let effects = site.effects.seen.lock().unwrap().len();
        let per_delivery = if record == "LicenseApplied" { 2 } else { 3 };
        assert_eq!(
            effects,
            2 * per_delivery,
            "{record}: no effect for a replay"
        );
    }
}

#[test]
fn catalog_kinds_and_datasets_are_judged_apart() {
    let streams = Streams::new();
    let mut site = site(streams.clone());
    assert_eq!(
        deliver(&mut site, &catalog("connector_package", 9, 'a')),
        HandOver::Journaled
    );
    assert_eq!(
        deliver(&mut site, &catalog("model_registry_entry", 2, 'b')),
        HandOver::Journaled
    );
    assert_eq!(
        deliver(&mut site, &bundle("sec_filings", 9, 'a')),
        HandOver::Journaled
    );
    assert_eq!(
        deliver(&mut site, &bundle("press_releases", 1, 'b')),
        HandOver::Journaled
    );
    assert_eq!(streams.types("ws_b1").len(), 4);
}

#[test]
fn a_refused_message_is_journaled_once_on_every_workspace() {
    let streams = Streams::new();
    let mut site = site(streams.clone());
    let unsigned = envelope(&[
        ("kind", r#""license""#),
        ("key_id", r#""license_key_1""#),
        ("body", r#"{"sequence":2}"#),
    ]);
    assert_eq!(
        deliver(&mut site, &unsigned),
        HandOver::Refused(RefusalReason::Unsigned)
    );
    assert_eq!(deliver(&mut site, &unsigned), HandOver::AlreadyApplied);
    for workspace in ["ws_a1", "ws_a2", "ws_b1"] {
        assert_eq!(streams.types(workspace), ["ControlPlaneMessageRefused"]);
        let p = &streams.payloads(workspace)[0];
        assert_eq!(p.get("reason").and_then(Value::as_str), Some("unsigned"));
        assert_eq!(p.get("sequence").and_then(Value::as_int), Some(2));
    }
    let outside = envelope(&[("kind", r#""kill_switch""#), ("body", "{}")]);
    assert_eq!(
        deliver(&mut site, &outside),
        HandOver::Refused(RefusalReason::OutOfList)
    );
    let p = &streams.payloads("ws_b1")[1];
    assert_eq!(p.get("message").and_then(Value::as_str), Some("unknown"));
    assert!(site.effects.seen.lock().unwrap().is_empty());
}

#[test]
fn a_license_for_an_organization_not_hosted_here_is_refused_not_addressed() {
    let streams = Streams::new();
    let mut site = site(streams.clone());
    let other = license("org_01JGAMMA", 1, 30);
    assert_eq!(
        deliver(&mut site, &other),
        HandOver::Refused(RefusalReason::NotAddressed)
    );
    assert_eq!(streams.types("ws_a1"), ["ControlPlaneMessageRefused"]);
    assert!(site.effects.seen.lock().unwrap().is_empty());
}

#[test]
fn an_unreadable_stream_is_unavailable_and_the_retry_completes_without_duplicates() {
    let mut streams = Streams::new();
    streams.down = Some("ws_a2".to_owned());
    let mut down = site(streams.clone());
    assert_eq!(
        deliver(&mut down, &license(ORG_A, 3, 30)),
        HandOver::Unavailable
    );
    assert_eq!(streams.types("ws_a1"), ["LicenseApplied"]);
    assert!(
        streams.types("ws_a2").is_empty(),
        "nothing took effect where nothing was journaled"
    );
    streams.down = None;
    let mut up = site(streams.clone());
    assert_eq!(
        deliver(&mut up, &license(ORG_A, 3, 30)),
        HandOver::Journaled
    );
    assert_eq!(streams.types("ws_a1"), ["LicenseApplied"], "no duplicate");
    assert_eq!(streams.types("ws_a2"), ["LicenseApplied"]);
}

/// Workspace services' own effect hangs mid-apply, as a hung `cp-agent` would hold its hand-over;
/// the journal is not held, so an owner's kill switch commits meanwhile (CP-3).
#[test]
fn a_kill_switch_commits_while_a_hand_over_hangs() {
    struct Hung(
        mpsc::Receiver<()>,
        mpsc::Sender<bool>,
        Arc<Mutex<MemoryJournal>>,
    );
    impl Effects for Hung {
        fn apply(&mut self, _: &Ident, _: &Verified) {
            let free = self.2.try_lock().is_ok();
            let _ = self.1.send(free);
            let _ = self.0.recv();
        }
    }
    let streams = Streams::new();
    let journal = streams.journal.clone();
    let held = streams.journal.clone();
    let epoch = streams.epochs["ws_a1"];
    let (release, wait) = mpsc::channel();
    let (entered, started) = mpsc::channel::<bool>();
    let hung = thread::spawn(move || {
        let mut site = Site {
            streams,
            effects: Hung(wait, entered, held),
            writer: Writer {
                id: "control_services".to_owned(),
                version: "0.1.0".to_owned(),
                build: Digest::of(b"control services build"),
                environment: "paper",
            },
            ids: next_id,
            clock: now,
        };
        receive(&license(ORG_A, 3, 30), &keys(), &me(), &mut site)
    });
    assert!(
        started.recv_timeout(Duration::from_secs(10)).unwrap(),
        "the effect runs with the journal's writer free"
    );
    let kill = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"ctl:ws_a1",
        "event_type":"OwnerCommandIssued","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":null,"correlation_id":null,
        "actor":{{"kind":"user","id":"user_owner_01","version":"1","build":null}},
        "config_refs":{{}},"payload":{{"command":"kill_switch","scope":"workspace"}},
        "artifact_refs":[],"pii_refs":[]}}"#,
        encode_ulid(1 << 100)
    );
    let outcome = {
        let mut j = journal.lock().unwrap();
        let head = j.head(&stream("ws_a1")).seq;
        j.append(&stream("ws_a1"), head, epoch, now(), &[kill.as_bytes()])
    };
    assert!(
        matches!(outcome, AppendOutcome::Committed(_)),
        "{outcome:?}"
    );
    release.send(()).unwrap();
    drop(release);
    assert_eq!(hung.join().unwrap(), HandOver::Journaled);
}

proptest! {
    /// CP-4's fuzz: whatever arrives (random bytes, and every kind with random content signed by
    /// the right key), nothing but a §9.17 record is ever appended.
    #[test]
    fn only_section_9_17_records_are_ever_appended(
        noise in proptest::collection::vec(any::<u8>(), 0..128),
        kind in 0usize..5,
        sequence in 0u64..20,
        org in prop::sample::select(vec![ORG_A, ORG_B, "org_01JGAMMA"]),
        extra in prop::sample::select(vec!["", r#","command":"kill_switch""#, r#","mandate":"x""#]),
    ) {
        let streams = Streams::new();
        let mut site = site(streams.clone());
        deliver(&mut site, &noise);
        let bodies = [
            format!(r#"{{"license_id":"l","sequence":{sequence},"org_id":"{org}","deployment_ids":["{DEPLOYMENT}"],"max_workspaces":1,"max_agents_deployed":1,"features":[],"not_before":"2026-10-01T00:00:00.000000000Z","not_after":"2027-10-01T00:00:00.000000000Z","grace_days":0{extra}}}"#),
            format!(r#"{{"release_id":"r","sequence":{sequence},"images":[{{"component":"api","digest":"{}"}}]{extra}}}"#, "c".repeat(64)),
            format!(r#"{{"release_id":"r","reason":"security"{extra}}}"#),
            format!(r#"{{"kind":"connector_package","entry_id":"e","content_hash":"{}","sequence":{sequence}{extra}}}"#, "e".repeat(64)),
            format!(r#"{{"dataset":"d","bundle_digest":"{}","sequence":{sequence}{extra}}}"#, "f".repeat(64)),
        ];
        let signers = [
            (MessageKind::License, "license_key_1", 1),
            (MessageKind::ReleaseManifest, "release_key_1", 2),
            (MessageKind::ReleaseWithdrawn, "release_key_1", 2),
            (MessageKind::CatalogEntry, "release_key_1", 2),
            (MessageKind::DataBundle, "data_key_1", 4),
        ];
        let (k, key, seed) = signers[kind];
        deliver(&mut site, &message(k, key, &bodies[kind], &pair(seed)));
        for workspace in ["ws_a1", "ws_a2", "ws_b1"] {
            for t in streams.types(workspace) {
                prop_assert!(SECTION_9_17.contains(&t.as_str()), "{t}");
            }
        }
    }
}

/// A withdrawal carries no sequence and is never a replay (DEC-981 item 5); the same withdrawal
/// again journals nothing, and another release's withdrawal is its own.
#[test]
fn a_withdrawal_is_journaled_once_per_document() {
    let streams = Streams::new();
    let mut site = site(streams.clone());
    let withdrawn = |release: &str| {
        let body = format!(r#"{{"release_id":"{release}","reason":"security"}}"#);
        message(
            MessageKind::ReleaseWithdrawn,
            "release_key_1",
            &body,
            &pair(2),
        )
    };
    assert_eq!(deliver(&mut site, &withdrawn("rel_1")), HandOver::Journaled);
    assert_eq!(
        deliver(&mut site, &withdrawn("rel_1")),
        HandOver::AlreadyApplied
    );
    assert_eq!(deliver(&mut site, &withdrawn("rel_2")), HandOver::Journaled);
    assert_eq!(
        streams.types("ws_b1"),
        ["ReleaseWithdrawnNoticed", "ReleaseWithdrawnNoticed"]
    );
}

/// Review of the handler: a deployment hosting no workspace journals nothing, so it answers
/// `Unavailable` and `cp-agent` keeps the message, rather than acknowledging it as applied.
#[test]
fn a_deployment_hosting_no_workspace_keeps_the_message() {
    let mut streams = Streams::new();
    streams.hosted.clear();
    let mut site = site(streams);
    assert_eq!(
        deliver(&mut site, &manifest(3, "api")),
        HandOver::Unavailable
    );
    assert_eq!(
        deliver(&mut site, &license(ORG_A, 3, 30)),
        HandOver::Unavailable
    );
    let unsigned = envelope(&[("kind", r#""license""#), ("body", "{}")]);
    assert_eq!(deliver(&mut site, &unsigned), HandOver::Unavailable);
}

/// Two catalog entries of one kind may share a sequence's content; another entry with the same
/// sequence is a conflict, refused `replayed`, never taken as the one in effect.
#[test]
fn another_catalog_entry_at_the_same_sequence_is_a_replay() {
    let streams = Streams::new();
    let mut site = site(streams.clone());
    assert_eq!(
        deliver(&mut site, &catalog("connector_package", 4, 'a')),
        HandOver::Journaled
    );
    let body = format!(
        r#"{{"kind":"connector_package","entry_id":"entry_2","content_hash":"{}","sequence":4}}"#,
        "a".repeat(64)
    );
    let other = message(MessageKind::CatalogEntry, "release_key_1", &body, &pair(2));
    assert_eq!(
        deliver(&mut site, &other),
        HandOver::Refused(RefusalReason::Replayed)
    );
}

/// A license with both features and two deployments is journaled as the journal accepts it.
#[test]
fn a_license_with_both_features_and_two_deployments_is_journaled() {
    let streams = Streams::new();
    let mut site = site(streams.clone());
    let body = format!(
        r#"{{"license_id":"lic_9","sequence":9,"org_id":"{ORG_A}",
        "deployment_ids":["dep_01JCELL0","{DEPLOYMENT}"],"max_workspaces":5,"max_agents_deployed":20,
        "features":["connected_clients","hybrid"],"not_before":"2026-10-01T00:00:00.000000000Z",
        "not_after":"2027-10-01T00:00:00.000000000Z","grace_days":30}}"#
    );
    let bytes = message(MessageKind::License, "license_key_1", &body, &pair(1));
    assert_eq!(deliver(&mut site, &bytes), HandOver::Journaled);
    let p = &streams.payloads("ws_a1")[0];
    let features: Vec<&str> = p
        .get("features")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(features, ["connected_clients", "hybrid"]);
}
