//! A connect start under passkey step-up over HTTP ([DEC-2126] items 1 to 3, DEC-2108 items 3, 7
//! and 8, journal spec §9.32 rules 253 and 254): the challenge from the challenge route, signed by
//! a software authenticator, verified by the server's real binding, and committed as
//! `ConnectionRequested` then `StepUpAsserted` in one batch, with the raw assertion stored; a
//! refused start, or one on a stream that breaks rule 254, appends and stores nothing; and a
//! `cli_confirm` start still works while DEC-2125's interim lasts.
//!
//! The binding is wrapped in a recorder, so a failure shows what the port answered. Every expected
//! record is written here by hand from the request the test sent.
//!
//! [DEC-2126]: ../../../docs/project/decisions/DEC-2126.md

mod common;
mod support_connect;
mod support_passkey;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::control::{Rows, envelope, stored};
use common::{WORKSPACE, send};
use mandate_api::envelope::{StepUpAssertion, StepUpEvidence};
use mandate_api::problem::{Problem, ProblemCode, Violation};
use mandate_api::wire::decode;
use mandate_api_server::confirm::CliConfirm;
use mandate_api_server::methods::{Methods, Passkey};
use mandate_api_server::objects::{ObjectStore, Objects};
use mandate_api_server::session::COOKIE_NAME;
use mandate_api_server::stepup::{Action, Asserted, Judged, StepUp, Unverified};
use mandate_api_server::subject::{LinkRefusal, NamesPrincipal};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_identity::PrincipalId;
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource, StoredEvent, get_artifact};
use serde_json::json;
use support_connect::{
    ALICE, APP, Fixture, KEY, MemObjects, StepUpBinding, T0, appended, base, code, ctl, derived,
    digest, fixture_binding, json, principal, signed_in, start, text, unissued,
};
use support_passkey::{CREDENTIAL, Signed, Signer, assert_by, asserted, enrol, enrolled, unb64url};

const CHALLENGE_KEY: &str = "k1Z-9_challeng01";
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The binding as the server holds it, with every answer it gave to `asserted` kept.
struct Recorded {
    inner: Methods,
    answers: Arc<Mutex<Vec<String>>>,
}

impl StepUp for Recorded {
    fn verify(
        &self,
        evidence: &StepUpEvidence,
        action: &Action,
        judged: &Judged<'_>,
    ) -> Result<(), ProblemCode> {
        self.inner.verify(evidence, action, judged)
    }

    fn asserted(
        &self,
        evidence: &StepUpEvidence,
        assertion: Option<&StepUpAssertion>,
        action: &Action,
        judged: &Judged<'_>,
    ) -> Result<Option<Asserted>, Unverified> {
        let answer = self.inner.asserted(evidence, assertion, action, judged);
        self.answers
            .lock()
            .expect("unpoisoned")
            .push(format!("{answer:?}"));
        answer
    }
}

/// Subject links that name Alice alone.
struct AliceLinked;

impl NamesPrincipal for AliceLinked {
    fn names(&self, who: PrincipalId) -> Result<bool, LinkRefusal> {
        Ok(who == principal(ALICE))
    }
}

/// The connect fixture with the real binding: Alice's passkey enrolled as [`CREDENTIAL`] at
/// counter 0, past its cool-off, its key in the object store.
struct World {
    f: Fixture,
    signer: Signer,
    answers: Arc<Mutex<Vec<String>>>,
    cookie: String,
    seeded: usize,
}

fn world() -> World {
    world_storing(|store| store)
}

/// [`world`] whose object store is `store` made of the in-memory one holding Alice's key.
fn world_storing(store: impl FnOnce(Arc<MemObjects>) -> Arc<dyn ObjectStore>) -> World {
    let signer = Signer::es256();
    let mut held = BTreeMap::new();
    let key = signer.cose_key();
    held.insert(Digest::of(&key), key);
    let objects = store(Arc::new(Objects::new(held)));
    let answers = Arc::new(Mutex::new(Vec::new()));
    let keys: Arc<dyn ArtifactSource + Send + Sync> = objects.clone();
    let kept = Arc::clone(&answers);
    let f = fixture_binding(StepUpBinding {
        bind: Box::new(move |challenges| {
            Arc::new(Recorded {
                inner: Methods {
                    passkey: Passkey {
                        challenges: Arc::clone(&challenges),
                        rp: support_passkey::rp(),
                        keys: Some(keys),
                    },
                    cli_confirm: CliConfirm {
                        challenges,
                        links: Arc::new(AliceLinked),
                    },
                },
                answers: kept,
            })
        }),
        objects,
    });
    let mut rows = Rows::opened();
    enrol(
        &mut rows,
        T0 - 900,
        enrolled(principal(ALICE), CREDENTIAL, &signer, 0, Some(T0 - 800)),
    );
    let seeded = rows.rows.len();
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(ctl(), rows.rows);
    let cookie = signed_in(&f, ALICE);
    World {
        f,
        signer,
        answers,
        cookie,
        seeded,
    }
}

/// An object store that reads as `0` does and refuses every write.
struct Refusing(Arc<MemObjects>);

impl ArtifactSource for Refusing {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        self.0.read_artifact(reference)
    }
}

impl ObjectStore for Refusing {
    fn put(&self, _bytes: &[u8]) -> Result<ArtifactRef, ArtifactError> {
        Err(ArtifactError::Unavailable)
    }
}

/// A challenge the challenge route issued: its id and the bytes the authenticator signs.
struct Issued {
    id: String,
    bytes: Vec<u8>,
    issued_at: String,
}

impl World {
    /// Alice's connect challenge for the current disclosure, from the challenge route.
    fn challenge(&self) -> Issued {
        let body = format!(
            r#"{{"action":{{"kind":"connection","digest":"sha256:{}"}}}}"#,
            digest("1").to_hex()
        );
        let cookie = format!("{COOKIE_NAME}={}", self.cookie);
        let reply = send(
            self.f.addr,
            "POST",
            &format!("/v1/workspaces/{WORKSPACE}/step-up/challenges"),
            &[
                ("Origin", APP),
                ("X-Mandate-Request", "1"),
                ("Content-Type", "application/json"),
                ("Cookie", &cookie),
                ("Idempotency-Key", CHALLENGE_KEY),
            ],
            body.as_bytes(),
        );
        assert_eq!(
            reply.status,
            201,
            "{}",
            String::from_utf8_lossy(&reply.body)
        );
        let answer = json(&reply);
        Issued {
            id: text(&answer, &["challenge_id"]).to_owned(),
            bytes: unb64url(text(&answer, &["webauthn_challenge"])),
            issued_at: text(&answer, &["issued_at"]).to_owned(),
        }
    }

    /// The start's body: the example with `method` evidence for `issued`, and `assertion`.
    fn body(&self, issued: &Issued, method: &str, assertion: Option<serde_json::Value>) -> Vec<u8> {
        let mut body: serde_json::Value =
            serde_json::from_slice(&to_canonical(&base())).expect("JSON");
        body["step_up"] = json!({
            "assertion_id": issued.id, "authenticated_at": issued.issued_at, "method": method,
        });
        if let Some(assertion) = assertion {
            body["step_up_assertion"] = assertion;
        }
        serde_json::to_vec(&body).expect("JSON")
    }

    fn start(&self, body: &[u8]) -> common::Reply {
        start(&self.f, Some(&self.cookie), Some(KEY), body)
    }

    /// The rows the start appended after the seeded ones.
    fn appended(&self) -> Vec<StoredEvent> {
        appended(&self.f)
            .into_iter()
            .skip(self.seeded - 1)
            .collect()
    }

    fn held(&self, issued: &Issued) -> bool {
        self.f.challenges.held(&issued.id).is_some()
    }

    fn answers(&self) -> Vec<String> {
        self.answers.lock().expect("unpoisoned").clone()
    }
}

fn payload(row: &StoredEvent) -> Value {
    parse(&row.body)
        .expect("JSON")
        .get("payload")
        .cloned()
        .expect("a payload")
}

fn member<'v>(row: &'v Value, name: &str) -> Option<&'v Value> {
    row.get(name)
}

#[test]
fn a_passkey_start_commits_the_request_then_its_assertion_and_stores_the_raw_assertion() {
    let w = world();
    let issued = w.challenge();
    w.f.clock.set(T0 + 5);
    let signed: Signed = w.signer.assert(&issued.bytes, 9);
    let reply = w.start(&w.body(&issued, "passkey", Some(signed.json(CREDENTIAL))));
    assert_eq!(
        reply.status,
        201,
        "{} after the port answered {:?}",
        String::from_utf8_lossy(&reply.body),
        w.answers()
    );
    let rows = w.appended();
    let kinds: Vec<(&str, u64)> = rows
        .iter()
        .map(|row| (row.event_type.as_str(), row.schema_version))
        .collect();
    assert_eq!(
        kinds,
        [("ConnectionRequested", 2), ("StepUpAsserted", 1)],
        "the request, then its assertion, in one batch"
    );
    assert_eq!(rows[0].event_id, derived("connect"));
    let step_up = format!(
        r#"{{"assertion_id":"{}","authenticated_at":"{}","method":"passkey"}}"#,
        issued.id, issued.issued_at
    );
    assert_eq!(
        member(&payload(&rows[0]), "step_up").map(to_canonical),
        Some(step_up.into_bytes()),
        "the request carries the evidence as sent"
    );
    let canonical = signed.canonical(CREDENTIAL);
    let expected = format!(
        r#"{{"assertion":"sha256:{}","challenge":"{}","credential":"{CREDENTIAL}","sign_count":9}}"#,
        Digest::of(&canonical).to_hex(),
        issued.id
    );
    assert_eq!(to_canonical(&payload(&rows[1])), expected.into_bytes());
    let record = parse(&rows[1].body).expect("JSON");
    assert_eq!(
        record.get("causation_id").and_then(Value::as_str),
        Some(derived("connect").as_str()),
        "rule 253: caused by the request it authorized"
    );
    assert_eq!(
        record
            .get("actor")
            .and_then(|actor| actor.get("id"))
            .and_then(Value::as_str),
        Some(ALICE)
    );
    assert_ne!(rows[1].event_id, rows[0].event_id);
    let objects = w.f.objects.as_ref().expect("an object store");
    assert_eq!(
        get_artifact(objects.as_ref(), &ArtifactRef::of(&canonical)),
        Ok(canonical),
        "the raw assertion is stored under its own digest"
    );
    assert!(!w.held(&issued), "the challenge is taken");
}

#[test]
fn a_refused_passkey_start_appends_and_stores_nothing_and_keeps_the_challenge() {
    let w = world();
    let issued = w.challenge();
    let mut signed = w.signer.assert(&issued.bytes, 9);
    let last = signed.signature.len() - 1;
    signed.signature[last] ^= 1;
    let reply = w.start(&w.body(&issued, "passkey", Some(signed.json(CREDENTIAL))));
    assert_eq!(
        code(&reply),
        (401, ProblemCode::StepUpMissing),
        "the port answered {:?}",
        w.answers()
    );
    assert!(w.appended().is_empty(), "nothing is appended");
    let objects = w.f.objects.as_ref().expect("an object store");
    assert!(
        get_artifact(
            objects.as_ref(),
            &ArtifactRef::of(&signed.canonical(CREDENTIAL))
        )
        .is_err(),
        "no assertion is stored"
    );
    assert!(w.held(&issued), "a refused start keeps the challenge");
    unissued(&w.f, &reply.body);
}

/// Appends Alice's `StepUpAsserted` records `(challenge, counter)` for her passkey to the stream.
fn asserted_before(w: &mut World, records: &[(&str, u32)]) {
    let mut rows = Rows {
        rows: w.f.journal.rows_of(&ctl()),
    };
    for (n, (challenge, counter)) in records.iter().enumerate() {
        let secs = T0 - 100 + i64::try_from(n).expect("small");
        assert_by(
            &mut rows,
            principal(ALICE),
            secs,
            asserted(challenge, CREDENTIAL, *counter),
        );
    }
    w.seeded = rows.rows.len();
    w.f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(ctl(), rows.rows);
}

#[test]
fn a_start_on_a_stream_that_breaks_rule_254_is_step_up_missing_and_appends_nothing() {
    let earlier = "01J9ZQ4B7Y8K3M5N6P7Q8R9S0V";
    for records in [
        vec![(earlier, 3), ("01J9ZQ4B7Y8K3M5N6P7Q8R9S0W", 2)],
        vec![(earlier, 1), (earlier, 2)],
    ] {
        let mut w = world();
        asserted_before(&mut w, &records);
        let issued = w.challenge();
        let signed = w.signer.assert(&issued.bytes, 9);
        let reply = w.start(&w.body(&issued, "passkey", Some(signed.json(CREDENTIAL))));
        assert_eq!(
            code(&reply),
            (401, ProblemCode::StepUpMissing),
            "{records:?}: the port answered {:?}",
            w.answers()
        );
        assert!(w.appended().is_empty(), "{records:?}: nothing is appended");
        assert!(w.held(&issued), "{records:?}");
    }
}

#[test]
fn a_replayed_passkey_start_answers_as_the_first_though_its_challenge_is_gone() {
    let w = world();
    let issued = w.challenge();
    w.f.clock.set(T0 + 5);
    let signed = w.signer.assert(&issued.bytes, 9);
    let body = w.body(&issued, "passkey", Some(signed.json(CREDENTIAL)));
    let first = w.start(&body);
    assert_eq!(
        first.status,
        201,
        "{} after the port answered {:?}",
        String::from_utf8_lossy(&first.body),
        w.answers()
    );
    let committed = w.appended();
    assert!(!w.held(&issued));
    let again = w.start(&body);
    assert_eq!(
        again.status,
        201,
        "{}",
        String::from_utf8_lossy(&again.body)
    );
    assert_eq!(json(&again), json(&first), "the same answer");
    assert_eq!(w.appended(), committed, "nothing more is appended");
}

#[test]
fn a_store_that_refuses_the_raw_assertion_appends_nothing() {
    let w = world_storing(|store| Arc::new(Refusing(store)));
    let issued = w.challenge();
    let signed = w.signer.assert(&issued.bytes, 9);
    let reply = w.start(&w.body(&issued, "passkey", Some(signed.json(CREDENTIAL))));
    assert_eq!(
        code(&reply),
        (503, ProblemCode::JournalUnavailable),
        "the port answered {:?}",
        w.answers()
    );
    assert!(w.appended().is_empty(), "nothing is appended");
    assert!(
        !w.held(&issued),
        "DEC-2108 item 8: the challenge was taken at verification, so the owner signs again"
    );
    unissued(&w.f, &reply.body);
}

#[test]
fn a_credential_stream_the_fold_cannot_read_is_journal_unavailable() {
    let w = world();
    let mut rows = Rows {
        rows: w.f.journal.rows_of(&ctl()),
    };
    rows.hosted(
        "CredentialEnrolled",
        T0 - 500,
        json!({"member": ALICE, "credential": CREDENTIAL, "alg": "rs256"}),
    );
    w.f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(ctl(), rows.rows);
    let issued = w.challenge();
    let signed = w.signer.assert(&issued.bytes, 9);
    let reply = w.start(&w.body(&issued, "passkey", Some(signed.json(CREDENTIAL))));
    assert_eq!(
        code(&reply),
        (503, ProblemCode::JournalUnavailable),
        "the port answered {:?}",
        w.answers()
    );
    assert!(w.held(&issued), "nothing is taken");
    let after_seeded: Vec<String> = w
        .appended()
        .iter()
        .skip(1)
        .map(|row| row.event_type.clone())
        .collect();
    assert!(
        after_seeded.is_empty(),
        "nothing is appended: {after_seeded:?}"
    );
}

#[test]
fn passkey_evidence_without_an_assertion_is_step_up_missing() {
    let w = world();
    let issued = w.challenge();
    let reply = w.start(&w.body(&issued, "passkey", None));
    assert_eq!(
        code(&reply),
        (401, ProblemCode::StepUpMissing),
        "the port answered {:?}",
        w.answers()
    );
    assert!(w.appended().is_empty());
    assert!(w.held(&issued));
}

/// `reply` is `invalid` with one violation, at `/step_up_assertion`.
fn invalid_at_the_assertion(reply: &common::Reply) {
    assert_eq!(code(reply), (422, ProblemCode::Invalid));
    let problem = decode::<Problem>(&reply.body).expect("a problem");
    let paths: Vec<String> = problem
        .violations
        .into_iter()
        .map(|v| match v {
            Violation::Schema { path, .. } => path,
            other => panic!("a body's violation is a schema finding: {other:?}"),
        })
        .collect();
    assert_eq!(paths, ["/step_up_assertion"]);
}

#[test]
fn an_assertion_beside_cli_confirm_evidence_is_invalid_and_appends_nothing() {
    let w = world();
    let issued = w.challenge();
    let signed = w.signer.assert(&issued.bytes, 9);
    let reply = w.start(&w.body(&issued, "cli_confirm", Some(signed.json(CREDENTIAL))));
    invalid_at_the_assertion(&reply);
    assert!(w.appended().is_empty());
    assert!(w.held(&issued));
    assert!(w.answers().is_empty(), "the port is never asked");
}

#[test]
fn an_assertion_beside_null_evidence_is_invalid_before_step_up_required() {
    let w = world();
    let issued = w.challenge();
    let signed = w.signer.assert(&issued.bytes, 9);
    let mut body: serde_json::Value =
        serde_json::from_slice(&w.body(&issued, "passkey", Some(signed.json(CREDENTIAL))))
            .expect("JSON");
    body["step_up"] = serde_json::Value::Null;
    let reply = w.start(&serde_json::to_vec(&body).expect("JSON"));
    invalid_at_the_assertion(&reply);
    assert!(w.appended().is_empty());
    assert!(w.answers().is_empty(), "the port is never asked");
}

/// The test's own Crockford base-32 of the first 40 bits of `bytes` (DEC-2104 item 1).
fn crockford40(bytes: &[u8]) -> String {
    let bits = bytes
        .iter()
        .take(5)
        .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte));
    (0..8)
        .rev()
        .map(|i| char::from(CROCKFORD[usize::try_from((bits >> (i * 5)) & 31).expect("5 bits")]))
        .collect()
}

#[test]
fn a_cli_confirm_start_still_commits_the_request_alone_while_the_interim_lasts() {
    let w = world();
    let issued = w.challenge();
    let mut rows = w.f.journal.rows_of(&ctl());
    let seq = u64::try_from(rows.len()).expect("small") + 1;
    let payload = json!({
        "challenge": issued.id, "action_kind": "connection",
        "action_digest": format!("sha256:{}", digest("1").to_hex()),
        "principal": ALICE, "code": crockford40(&issued.bytes), "confirmed_at": T0 + 2,
    });
    let confirmed = envelope(&ctl(), seq, "paper", "StepUpConfirmed", T0 + 2, payload);
    rows.push(stored(seq, &confirmed));
    w.f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(ctl(), rows);
    w.f.clock.set(T0 + 5);
    let reply = w.start(&w.body(&issued, "cli_confirm", None));
    assert_eq!(
        reply.status,
        201,
        "{} after the port answered {:?}",
        String::from_utf8_lossy(&reply.body),
        w.answers()
    );
    let kinds: Vec<String> = w
        .appended()
        .iter()
        .skip(1)
        .map(|row| row.event_type.clone())
        .collect();
    assert_eq!(kinds, ["ConnectionRequested"], "no assertion record");
    assert!(!w.held(&issued));
}
