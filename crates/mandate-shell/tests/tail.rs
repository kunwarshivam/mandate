//! The runtime's control-stream tail's fold (E7-26, the runtime grant→submit continuation —
//! stream L / M7's remainder; DEC-155 items 2
//! and 5; LT-4; mandate spec §6.4; journal spec §2, §3, §9.2, §9.7; DEC-912 item 1; DEC-914
//! items 2 and 7). These are the tail's pins, pending on the stub until the implementation PR
//! deletes its body alone: the owner's answer reaching the runtime as `Input::Journal` with the
//! envelope's own actor (never the tracer's blanket `system`), the journal's own order handed
//! verbatim and whole, an actor other than the envelope's `user` folding as something admission
//! never admits, and a row the fold cannot read refusing as a difference. Every fixture is a
//! journal-shaped canonical envelope committed to a control stream the run read, holds no
//! credential and reaches no host (`AGENTS.md` rules 7 and 8, LT-1, ES-19).

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_journal::StoredEvent;
use mandate_runtime::{ActorKind, EventId, FoldedEvent, Seq};
use mandate_shell::tailed;

/// The workspace control stream of every fixture (journal spec §2: `ctl:<workspace>`).
const STREAM: &str = "ctl:ws_span";

/// The instant every fixture row is recorded at (journal spec §3's `event_time` shape).
const T: &str = "2026-10-13T15:05:00.000000000Z";

/// The ask's approval: the `ApprovalRequested`'s event id, an opaque id (TI-8).
const APPROVAL: &str = "01JZWTG5EV7Q4ZF0AG5H7M1BR8";

/// The envelope's `actor` member (journal spec §3): the actor the row's writer committed, with
/// the opaque user it acted as and the build it named — JSON text, so a fixture can also commit
/// an actor the tail cannot read a kind from.
fn actor(kind: &str) -> String {
    format!(
        r#"{{"kind":"{kind}","id":"user_founder","version":"0.1.0","build":"sha256:{build}"}}"#,
        build = "3".repeat(64)
    )
}

/// The `ApprovalResponseSubmitted` a founder's CLI committed (journal spec §9.7, DEC-533): the
/// members the runtime's admission reads, the row the grant arrives through.
fn answer_payload() -> String {
    format!(
        r#"{{"agent":"agent_01","approval":"{APPROVAL}","verdict":"approved","content_hash":"sha256:{hash}","submitted_at":"{T}","step_up":null,"responder":"user_founder","role":"approver"}}"#,
        hash = Digest::of(b"the ask's bound order").to_hex()
    )
}

/// The `OwnerCommandIssued` the CLI committed for a pause (journal spec §9.7): step-up never
/// asked for it.
fn command_payload() -> String {
    format!(
        r#"{{"agent":"agent_01","command":"pause","submitted_at":"{T}","step_up":null,"user":"user_founder"}}"#
    )
}

/// The `AgentDeployed` record the workspace services committed (journal spec §9.2): a
/// control-stream row that is not of the owner-input pair, folded inert by the runtime's own
/// routing.
fn deployed_payload() -> String {
    format!(
        r#"{{"agent_id":"agent_01","mandate_version":"sha256:{version}","record_ref":"sha256:{record}"}}"#,
        version = Digest::of(b"the confirmed mandate document").to_hex(),
        record = Digest::of(b"the go-live screen").to_hex()
    )
}

/// One control-stream row (journal spec §3's envelope canonicalized to the bytes the journal
/// hashed): journal-assigned columns beside a body whose `actor` and `payload` members the
/// fixture named, at `seq`, typed `event_type`, on `STREAM`.
fn row(seq: u64, event_type: &str, actor_kind: &str, payload: &str) -> StoredEvent {
    row_of(seq, event_type, &actor(actor_kind), payload)
}

/// [`row`] with the envelope's `actor` member composed by `actor`, in whatever shape a writer
/// committed, so a fixture can name an actor the tail cannot read a kind from.
fn row_of(seq: u64, event_type: &str, actor: &str, payload: &str) -> StoredEvent {
    let envelope = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"{STREAM}","event_type":"{event_type}","schema_version":1,"event_time":"{T}","clock_source":"local","causation_id":null,"correlation_id":null,"actor":{actor},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#,
        event_id = event_id(seq)
    );
    StoredEvent {
        stream_id: STREAM.to_owned(),
        seq,
        event_id: event_id(seq),
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: T.to_owned(),
        prev_hash: Digest::of(&[]),
        hash: Digest::of(&[]),
        body: to_canonical(&parse(envelope.as_bytes()).expect("the fixture envelope parses")),
    }
}

fn event_id(seq: u64) -> String {
    format!("01J8Z3M4{seq:018}")
}

fn json(text: &str) -> Value {
    parse(text.as_bytes()).expect("the fixture member parses")
}

/// The fold's answer with the stopped run's own message in the error's place (ES-09), so every
/// pin pins the words the process prints, never a variant shape a test could not read.
fn folded_rows(rows: &[StoredEvent]) -> Result<Vec<FoldedEvent>, String> {
    tailed(rows).map_err(|stopped| stopped.to_string())
}

/// The [`FoldedEvent`] the runtime is handed for one fixture row.
fn folded(seq: u64, event_type: &str, actor: ActorKind, payload: &str) -> FoldedEvent {
    FoldedEvent {
        stream: STREAM.to_owned(),
        seq: Seq(seq),
        event_id: EventId(event_id(seq)),
        event_type: event_type.to_owned(),
        causation_id: None,
        actor,
        payload: json(payload),
    }
}

/// Mandate spec §6.4 — "The agent runtime journals the request with everything it binds
/// (`ApprovalRequested`, journal spec §9), notifies, and **acts only on an admitted human grant
/// that passes re-validation**" — and the runtime's own escalation doc — "The owner's answer
/// arrives only as a control-stream `ApprovalResponseSubmitted`, tailed and handed in as
/// `Input::Journal`" (`crates/mandate-runtime/src/escalation.rs`) — through the fold this row
/// stubs (DEC-155 items 2 and 5, "the control-stream tail ... belong[s] in the shell"): the row's
/// members reach the runtime as the row committed them, one per row, and the actor is the
/// envelope's own `user` (`crates/mandate-shell/src/tracer.rs`: "the control-stream tail that
/// maps the envelope's `actor.kind` is stream L's"), never the tracer's blanket `System`, so
/// admission (mandate-approval, EI-10's check 3) can admit the founder's own grant and no other.
#[test]
fn the_tail_hands_the_runtime_the_owners_answer_verbatim() {
    let answer = row(7, "ApprovalResponseSubmitted", "user", &answer_payload());
    let handed = folded_rows(std::slice::from_ref(&answer));
    assert_eq!(
        handed,
        Ok(vec![folded(
            7,
            "ApprovalResponseSubmitted",
            ActorKind::User,
            &answer_payload()
        )]),
        "the answer reaches the runtime as the row committed it: the ctl stream the row names, its own seq, event id and event type, its body's own payload, and the envelope's own user actor"
    );
}

/// DEC-912 item 1 — "the submit is released by the answers **in journal order**: the first answer
/// naming the ask's approval decides it" — and DEC-914 item 2's fold rule — each row mapped
/// "verbatim, in journal order ... never re-sorted by another key, never filtered to the answers
/// alone, no read of a row's members from anywhere but the row's own payload" — as the tail's own
/// half of that discipline (#1313's `answers` fold is the runner's): the read's **every** row is
/// handed, in the journal's own order, one per row, whatever its type — an `OwnerCommandIssued`
/// after the answers, an `AgentDeployed` record between them — and the seqs stay the rows' own
/// (3, 9 and 31, not the read's indices), for the runtime's own arms to decide what each acts on:
/// nothing between is the tail's to judge (DEC-138 item 3).
#[test]
fn the_tail_hands_the_journal_s_read_whole_in_its_own_order() {
    assert_eq!(
        folded_rows(&[]),
        Ok(Vec::new()),
        "a read holding nothing hands nothing"
    );
    let read = vec![
        row(3, "ApprovalResponseSubmitted", "user", &answer_payload()),
        row(9, "AgentDeployed", "system", &deployed_payload()),
        row(31, "OwnerCommandIssued", "user", &command_payload()),
    ];
    let handed = folded_rows(&read);
    assert_eq!(
        handed,
        Ok(vec![
            folded(
                3,
                "ApprovalResponseSubmitted",
                ActorKind::User,
                &answer_payload()
            ),
            folded(9, "AgentDeployed", ActorKind::System, &deployed_payload()),
            folded(
                31,
                "OwnerCommandIssued",
                ActorKind::User,
                &command_payload()
            ),
        ]),
        "every row of the read reaches the runtime, in the journal's own order, each with the row's own seq, type, payload and actor"
    );
}

/// The [`FoldedEvent`] contract's fail-closed sentence — "Admission admits a response only from a
/// `user` (check 3, EI-10), so a reader that cannot tell gives anything but `User`"
/// (`crates/mandate-runtime/src/types.rs`) — over journal spec §3's closed kind set ("`kind` ∈
/// `system`, `agent`, `user`, `broker`, `platform_operator`, `client`"): the four non-user kinds
/// map each to its own actor, and the sixth — the `client` of v0.22, "A client is never a
/// `user`, so a rule that requires a user actor" refuses — together with an actor holding no
/// kind the tail can read, folds `System`, what admission never admits
/// (`crates/mandate-shell/src/tracer.rs`'s own `TRACER_ACTOR` carrier), so no reading of the
/// envelope can carry a grant the writer did not stand behind (EI-10, DEC-176: the refusing one).
#[test]
fn only_the_envelope_s_own_user_folds_as_a_user() {
    let named = [
        (1, "user", ActorKind::User),
        (2, "system", ActorKind::System),
        (3, "agent", ActorKind::Agent),
        (4, "broker", ActorKind::Broker),
        (5, "platform_operator", ActorKind::PlatformOperator),
        (6, "client", ActorKind::System),
    ];
    let mut read: Vec<StoredEvent> = named
        .iter()
        .map(|(seq, kind, _)| row(*seq, "ApprovalResponseSubmitted", kind, &answer_payload()))
        .collect();
    read.push(row_of(
        7,
        "ApprovalResponseSubmitted",
        r#"{"id":"cl1","version":"0.1.0","build":null}"#,
        &answer_payload(),
    ));
    let actors: Vec<ActorKind> = folded_rows(&read)
        .expect("the read is one the tail can fold, member by member")
        .into_iter()
        .map(|event| event.actor)
        .collect();
    let mut expected: Vec<ActorKind> = named.iter().map(|(_, _, actor)| *actor).collect();
    expected.push(ActorKind::System);
    assert_eq!(
        actors, expected,
        "each named kind folds its own actor, and a client and an actor holding no readable kind fold System, never User"
    );
}

/// `AGENTS.md` rule 3 and the shell's own fold discipline — "a payload and causation a stored
/// body carries, read from its canonical bytes rather than from anything the shell remembers, so
/// what is folded is what was committed" (`crates/mandate-shell/src/tracer.rs`, `body_of`) — for
/// the tail: a row whose body cannot be read, and a body holding no `payload` member, refuse with
/// the envelope's own refusals and **nothing handed**, the whole read refused rather than a
/// partial fold guessed on (#1311's pinned rule: "an answer the run cannot read refuses as a
/// difference" — never a guess, never a skip).
#[test]
fn a_row_the_tail_cannot_read_refuses_as_a_difference() {
    let unreadable = StoredEvent {
        body: b"not a journal record".to_vec(),
        ..row(1, "ApprovalResponseSubmitted", "user", &answer_payload())
    };
    let refused = folded_rows(std::slice::from_ref(&unreadable));
    assert_eq!(
        refused,
        Err("the journal envelope's stored body could not be composed".to_owned()),
        "a body the tail cannot read is a difference, nothing handed"
    );
    let memberless = StoredEvent {
        body: b"{}".to_vec(),
        ..row(2, "ApprovalResponseSubmitted", "user", &answer_payload())
    };
    let refused = folded_rows(std::slice::from_ref(&memberless));
    assert_eq!(
        refused,
        Err("the journal envelope's payload could not be composed".to_owned()),
        "a body holding no payload member is a difference, nothing handed"
    );
}
