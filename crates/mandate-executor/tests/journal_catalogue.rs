//! DEC-2139 item 3: no record the executor drafts on the account stream may outrun the journal. A
//! kill switch's batch, `[AgentModeApplied, KillSwitchActivated]`, was refused whole as
//! `unknown_schema` until journal spec v0.90 closed the account stream's `KillSwitchActivated`, so
//! the cell's executor crash-looped on every switch it applied. Each switch the executor can apply
//! is carried here through the journal's own `Draft::parse` and `check_batch`, and every account
//! event type its sources name is closed in `mandate-journal`, but the ones still pending.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use common::{FixedInstruments, FixedMandate, Shell, TestIds, agent, config, ports, price, qty};
use mandate_canon::{Value, to_canonical};
use mandate_executor::{
    AccountRef, Command, EventDraft, Initiator, Input, KillScope, OwnerConfirmation,
};
use mandate_journal::{Draft, StreamType, check_batch, stream_event_types};

/// The account-stream records the executor drafts that the journal does not close yet, each the
/// next slice's (DEC-2139 item 4). Closing one fails this test until it leaves the list.
const PENDING: [&str; 3] = [
    "ExternalActivityIngested",
    "RejectObserved",
    "OrderAbandoned",
];

/// A started executor on an opened stream, reconciled, as a production shell starts.
fn opened(ports: &mandate_executor::Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell
        .fold_one(&common::stream_opened())
        .expect("the stream opens");
    shell.restart_ready(ports)
}

fn confirmation() -> OwnerConfirmation {
    OwnerConfirmation {
        bid: price("155"),
        bid_size: qty("100"),
        floor: price("150.35"),
        user: "user-1".to_owned(),
        step_up: "assertion-1".to_owned(),
    }
}

/// The event id a registration probe is appended under.
const EVENT: &str = "01J8Z3M4000000000000000001";

/// `draft` in the envelope the shell appends it in, as canonical bytes (journal spec §3), its own
/// and its cause's ids the ULIDs `ulids` maps the test's ids to.
fn enveloped_as(draft: &EventDraft, ulids: &BTreeMap<String, String>) -> Vec<u8> {
    let ulid = |id: &str| ulids.get(id).cloned().unwrap_or_else(|| id.to_owned());
    let cause = draft
        .causation_id
        .as_ref()
        .map_or_else(|| "null".to_owned(), |id| format!("\"{}\"", ulid(&id.0)));
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper",
        "event_id":"{}","stream_id":"{}",
        "event_type":"{}","schema_version":{},
        "event_time":"2026-09-22T14:00:00.000000000Z","clock_source":"local",
        "causation_id":{},"correlation_id":null,
        "actor":{{"kind":"system","id":"executor","version":"0.1.0",
        "build":"sha256:{}"}},"config_refs":{},"payload":{},
        "artifact_refs":[],"pii_refs":[]}}"#,
        ulid(&draft.event_id.0),
        common::ACCOUNT_STREAM,
        draft.event_type,
        draft.schema_version,
        cause,
        "3".repeat(64),
        String::from_utf8_lossy(&to_canonical(&Value::Object(draft.config_refs.clone()))),
        String::from_utf8_lossy(&to_canonical(&draft.payload)),
    );
    body.into_bytes()
}

fn enveloped(draft: &EventDraft) -> Vec<u8> {
    enveloped_as(draft, &BTreeMap::new())
}

/// Each draft's id mapped to a ULID of its own, in draft order.
fn ulids(drafts: &[EventDraft]) -> BTreeMap<String, String> {
    drafts
        .iter()
        .enumerate()
        .map(|(n, d)| {
            (
                d.event_id.0.clone(),
                format!("01J8Z3M4000000000000000{:03}", n + 2),
            )
        })
        .collect()
}

/// Every kill switch the executor applies, of each scope, initiator, and confirmation the owner
/// may give, is journaled in one batch the journal accepts whole.
#[test]
fn every_kill_switch_batch_is_accepted_by_the_journal() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[FixedInstruments::LIQUID_EQUITY]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let scopes = [
        KillScope::Agent(agent(common::AGENT)),
        KillScope::Account(AccountRef(common::ACCOUNT.to_owned())),
    ];
    let switches = [
        (Initiator::Owner, None),
        (Initiator::Owner, Some(confirmation())),
        (Initiator::RiskLimit, None),
        (Initiator::PlatformOperator, None),
    ];
    let mut failed = Vec::new();
    for scope in &scopes {
        for (initiator, confirmation) in &switches {
            let mut shell = opened(&ports);
            let ran = shell.run(
                Input::Command(Command::KillSwitch {
                    scope: scope.clone(),
                    initiator: *initiator,
                    confirmation: confirmation.clone(),
                    cause: None,
                }),
                &ports,
            );
            let case = format!(
                "{scope:?} {initiator:?} confirmed {}",
                confirmation.is_some()
            );
            assert!(
                ran.draft("KillSwitchActivated").is_some(),
                "{case}: the switch is journaled"
            );
            let ulids = ulids(&ran.drafts);
            let bytes: Vec<Vec<u8>> = ran
                .drafts
                .iter()
                .map(|draft| enveloped_as(draft, &ulids))
                .collect();
            let parsed: Result<Vec<Draft>, _> = bytes.iter().map(|b| Draft::parse(b)).collect();
            match parsed.map(|drafts| check_batch(&drafts).map_err(|(_, e)| e)) {
                Ok(Ok(())) => {}
                Ok(Err(e)) | Err(e) => failed.push(format!(
                    "{case}: {:?} refused {} at {}",
                    ran.draft_types(),
                    e.reason.code(),
                    e.path
                )),
            }
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Every event type `dir`'s Rust sources draft by name, recursively: the literal each
/// `Batch::journal` or `Batch::journal_with_refs` call opens with.
fn drafted_names(dir: &Path, names: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            drafted_names(&path, names);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let source = std::fs::read_to_string(&path).unwrap();
            for call in [".journal(", ".journal_with_refs("] {
                for (at, _) in source.match_indices(call) {
                    let rest = source[at + call.len()..].trim_start();
                    if let Some(named) = rest.strip_prefix('"').and_then(|r| r.split('"').next()) {
                        names.insert(named.to_owned());
                    }
                }
            }
        }
    }
}

/// Whether the journal registers `event_type` on the account stream at some version: a draft of
/// it with an empty payload is refused for its members, never as `unknown_schema`.
fn registered(event_type: &str) -> bool {
    (1..=4).any(|version| {
        let draft = EventDraft {
            event_id: mandate_executor::EventId(EVENT.to_owned()),
            event_type: event_type.to_owned(),
            schema_version: version,
            config_refs: mandate_canon::Object::new(),
            causation_id: None,
            payload: Value::Object(mandate_canon::Object::new()),
        };
        Draft::parse(&enveloped(&draft))
            .map_or_else(|e| e.reason.code() != "unknown_schema", |_| true)
    })
}

/// Every event type the executor drafts on the account stream is registered in `mandate-journal`
/// at some version, but exactly the pending ones, so a writer can no longer outrun the journal.
#[test]
fn every_event_type_the_executor_drafts_is_registered_but_the_pending_ones() {
    let mut names = BTreeSet::new();
    drafted_names(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut names,
    );
    let account: BTreeSet<&str> = stream_event_types(StreamType::Account).collect();
    let elsewhere: Vec<&String> = names
        .iter()
        .filter(|name| !account.contains(name.as_str()))
        .collect();
    assert!(
        elsewhere.is_empty(),
        "drafted off the account stream: {elsewhere:?}"
    );
    let named: Vec<&str> = names.iter().map(String::as_str).collect();
    assert!(named.contains(&"KillSwitchActivated"), "{named:?}");
    let open: BTreeSet<&str> = named
        .iter()
        .copied()
        .filter(|event_type| !registered(event_type))
        .collect();
    assert_eq!(
        open,
        PENDING.into_iter().collect(),
        "drafted on the account stream: {named:?}"
    );
}
