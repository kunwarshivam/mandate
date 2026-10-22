//! Journal spec v0.46 §9.19 (DEC-998, E15-8's code half, DEC-2201): the agent stream's closed
//! `ModelInvocationRecorded` and the gateway's meter stream, through the journal's own checks
//! against the vectors' `model_gateway` section. Every base and valid draft parses; every invalid
//! draft is refused with its reason at its path (rules 146 to 152 and 154 to 157, the closed members
//! and their types); and every stream vector is judged by [`check_stream`] as it says (rules 153 and
//! 158), from the stream's start, one record at a time and as one batch. The conformance suite
//! appends the same streams through both backends.

use std::path::Path;

use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_journal::{Draft, StreamId, StreamType, check_stream};

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("model_gateway").cloned().unwrap()
}

fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .unwrap_or_default()
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// A `{path, value}` or `{path, delete: true}` change, dotted from the envelope.
fn apply(draft: &mut Object, change: &Value) {
    let path = text(change, "path");
    let mut names: Vec<&str> = path.split('.').collect();
    let last = names.pop().unwrap();
    let mut node = draft;
    for name in names {
        node = match node.get_mut(name) {
            Some(Value::Object(inner)) => inner,
            other => panic!("`{path}` has no object `{name}`: {other:?}"),
        };
    }
    if change.get("delete") == Some(&Value::Bool(true)) {
        assert!(node.remove(last).is_some(), "`{path}` deletes nothing");
    } else {
        node.insert(
            Key::new(last).unwrap(),
            change.get("value").cloned().unwrap(),
        );
    }
}

/// A case's draft: its named base draft with its changes applied, in canonical bytes.
fn draft(section: &Value, case: &Value) -> Vec<u8> {
    let base = section
        .get("drafts")
        .and_then(|d| d.get(text(case, "base_draft")));
    let mut body = base.and_then(Value::as_object).cloned().unwrap();
    for change in list(case, "changes") {
        apply(&mut body, change);
    }
    to_canonical(&Value::Object(body))
}

/// What the vectors expect of a case: `Valid`, or the reason and path it is refused with, and for
/// a stream the index of the record that breaks it.
fn expected(case: &Value) -> Option<(usize, String, String)> {
    let expect = case.get("expect").unwrap();
    match text(expect, "outcome") {
        "Valid" => None,
        "Invalid" => Some((
            expect
                .get("index")
                .and_then(Value::as_int)
                .and_then(|i| usize::try_from(i).ok())
                .unwrap_or(0),
            text(expect, "reason").to_owned(),
            text(expect, "path").to_owned(),
        )),
        other => panic!("{}: unknown outcome {other}", text(case, "name")),
    }
}

fn parsed(records: &[Value]) -> Vec<Draft> {
    records
        .iter()
        .map(|record| Draft::parse(&to_canonical(record)).unwrap())
        .collect()
}

#[test]
fn every_model_gateway_base_and_valid_draft_parses() {
    let section = section();
    let drafts = section.get("drafts").and_then(Value::as_object).unwrap();
    let mut failed = Vec::new();
    for (name, body) in drafts {
        if let Err(e) = Draft::parse(&to_canonical(body)) {
            failed.push(format!("base {}: {e:?}", name.as_str()));
        }
    }
    let valid = list(&section, "valid_drafts");
    for case in valid {
        if let Err(e) = Draft::parse(&draft(&section, case)) {
            failed.push(format!("valid {}: {e:?}", text(case, "name")));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    let names: Vec<&str> = drafts.keys().map(Key::as_str).collect();
    assert_eq!(names, ["call", "opened", "reserved", "settled"]);
    assert!(
        valid.len() >= 32,
        "{} valid drafts; never fewer",
        valid.len()
    );
}

#[test]
fn every_invalid_model_gateway_draft_is_refused_with_its_reason_at_its_path() {
    let section = section();
    let invalid = list(&section, "invalid_drafts");
    let mut failed = Vec::new();
    for case in invalid {
        let (_, reason, path) = expected(case).unwrap();
        let got = Draft::parse(&draft(&section, case)).err();
        let got = got.as_ref().map(|e| (e.reason.code(), e.path.as_str()));
        if got != Some((reason.as_str(), path.as_str())) {
            failed.push(format!(
                "{}: expected {:?}, got {got:?}",
                text(case, "name"),
                (reason, path)
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        invalid.len() >= 153,
        "{} invalid drafts; never fewer",
        invalid.len()
    );
    for rule in 146..=157 {
        if rule == 153 {
            continue;
        }
        let clause = format!("rule {rule}");
        assert!(
            invalid
                .iter()
                .any(|c| text(c, "clause").starts_with(&clause)),
            "{clause} has an invalid draft"
        );
    }
}

/// Rules 153 and 158 over each stream vector from its start: the first record that breaks one is
/// the vector's, whether the stream is judged as one batch or one record after the earlier ones.
#[test]
fn every_model_gateway_stream_is_judged_from_its_start_as_its_vector_says() {
    let section = section();
    let streams: Vec<&Value> = list(&section, "agent_streams")
        .iter()
        .chain(list(&section, "streams"))
        .collect();
    let mut failed = Vec::new();
    for case in &streams {
        let name = text(case, "name");
        let records = parsed(list(case, "records"));
        let want = expected(case);
        let as_batch = check_stream(&[], &records)
            .err()
            .map(|(i, e)| (i, e.reason.code().to_owned(), e.path));
        let one_by_one = (0..records.len()).find_map(|i| {
            check_stream(&records[..i], &records[i..=i])
                .err()
                .map(|(at, e)| (i + at, e.reason.code().to_owned(), e.path))
        });
        if as_batch != want || one_by_one != want {
            failed.push(format!(
                "{name}: expected {want:?}, as one batch {as_batch:?}, one by one {one_by_one:?}"
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert_eq!(
        streams.len(),
        11,
        "three agent streams and eight meter streams"
    );
    for rule in ["rule 153", "rule 158"] {
        assert!(
            streams
                .iter()
                .any(|c| text(c, "clause").starts_with(rule) && expected(c).is_some()),
            "{rule} has a breaking stream"
        );
    }
}

/// Rule 153 reads only `ModelInvocationRecorded`: a second record of a call is refused whatever
/// lies between, and another stream's record of the same call, or another event type on this
/// stream naming it, never is.
#[test]
fn a_call_recorded_once_per_agent_stream_whatever_lies_between() {
    let section = section();
    let call = base(&section, "call");
    let again = renamed(&section, "call", "01J9GW10A000000000000000M9");
    let refused = check_stream(std::slice::from_ref(&call), std::slice::from_ref(&again));
    let refused = refused.err().map(|(i, e)| (i, e.reason.code(), e.path));
    assert_eq!(
        refused,
        Some((0, "schema", "payload.call_id".to_owned())),
        "rule 153: the same call_id twice on one agent stream"
    );
    let other_agent = elsewhere(&section, "call", "agent:ws_01J8Z2:agent_b", "agent_b");
    assert_eq!(
        check_stream(
            std::slice::from_ref(&call),
            std::slice::from_ref(&other_agent)
        ),
        Ok(()),
        "a stream rule reads one stream's records"
    );
}

/// Rule 158 against the meter stream's own records only: a settlement names a reservation of its
/// stream, so an agent-stream record of the call is not one.
#[test]
fn a_settlement_names_a_reservation_of_its_own_meter_stream() {
    let section = section();
    let opened = base(&section, "opened");
    let reserved = base(&section, "reserved");
    let settled = base(&section, "settled");
    assert_eq!(
        check_stream(&[], &[opened.clone(), reserved.clone(), settled.clone()]),
        Ok(())
    );
    let call = base(&section, "call");
    let refused = check_stream(&[opened, call], std::slice::from_ref(&settled));
    let refused = refused.err().map(|(i, e)| (i, e.reason.code(), e.path));
    assert_eq!(
        refused,
        Some((0, "schema", "payload.reservation".to_owned())),
        "an unreserved settlement"
    );
}

#[test]
fn the_meter_stream_is_its_own_stream_type() {
    let meter = StreamId::parse("meter:ws_01J8Z2").unwrap();
    assert_eq!(meter.stream_type(), StreamType::Meter);
    assert_eq!(meter.as_str(), "meter:ws_01J8Z2");
    for refused in ["meter:", "meter:ws:extra", "meter:ws 1", "meters:ws_1"] {
        assert_eq!(StreamId::parse(refused), None, "{refused}");
    }
}

/// The catalogue answers what `mandate-runtime`'s `JournalCatalog` reads (agent harness spec §6.2):
/// the agent stream's `ModelInvocationRecorded` is closed at version 1, with its `verdicts`; at no
/// other version, and on no stream it does not belong to.
#[test]
fn the_catalogue_closes_the_agent_streams_model_invocation_at_version_1() {
    use mandate_journal::closes;
    assert!(closes("ModelInvocationRecorded", 1, StreamType::Agent));
    assert!(closes("ModelInvocationRecorded", 1, StreamType::Control));
    assert!(!closes("ModelInvocationRecorded", 2, StreamType::Agent));
    assert!(!closes("ModelInvocationRecorded", 1, StreamType::Account));
    for record in ["StreamOpened", "MeterReserved", "MeterSettled"] {
        assert!(closes(record, 1, StreamType::Meter), "{record}");
        assert!(!closes(record, 2, StreamType::Meter), "{record} v2");
    }
    for record in ["MeterReserved", "MeterSettled"] {
        for stream in [
            StreamType::Account,
            StreamType::Agent,
            StreamType::Control,
            StreamType::Scheduler,
            StreamType::Notice,
        ] {
            assert!(!closes(record, 1, stream), "{record} on {stream:?}");
        }
    }
    assert!(!closes("MarkUpdated", 1, StreamType::Meter));
}

fn base(section: &Value, name: &str) -> Draft {
    let body = section.get("drafts").and_then(|d| d.get(name)).unwrap();
    Draft::parse(&to_canonical(body)).unwrap()
}

/// The base draft `name` under another event id.
fn renamed(section: &Value, name: &str, event_id: &str) -> Draft {
    let mut body = section
        .get("drafts")
        .and_then(|d| d.get(name))
        .and_then(Value::as_object)
        .cloned()
        .unwrap();
    body.insert(
        Key::new("event_id").unwrap(),
        Value::Str(event_id.to_owned()),
    );
    Draft::parse(&to_canonical(&Value::Object(body))).unwrap()
}

/// The base draft `name` on another agent's stream, written by that agent.
fn elsewhere(section: &Value, name: &str, stream: &str, agent: &str) -> Draft {
    let mut body = section
        .get("drafts")
        .and_then(|d| d.get(name))
        .and_then(Value::as_object)
        .cloned()
        .unwrap();
    body.insert(
        Key::new("stream_id").unwrap(),
        Value::Str(stream.to_owned()),
    );
    apply(
        &mut body,
        &object(&[("path", "actor.id"), ("value", agent)]),
    );
    Draft::parse(&to_canonical(&Value::Object(body))).unwrap()
}

fn object(members: &[(&str, &str)]) -> Value {
    let mut object = Object::new();
    for (key, value) in members {
        object.insert(Key::new(key).unwrap(), Value::Str((*value).to_owned()));
    }
    Value::Object(object)
}

/// `closes` answers for the records the agent stream's own module closes too, at their versions
/// only, and nowhere the catalogue does not admit them.
#[test]
fn the_catalogue_answers_for_the_agent_streams_own_records() {
    use mandate_journal::closes;
    assert!(closes("ObservationRecorded", 1, StreamType::Agent));
    assert!(!closes("ObservationRecorded", 2, StreamType::Agent));
    assert!(!closes("ObservationRecorded", 1, StreamType::Control));
    assert!(closes("ApprovalRequested", 2, StreamType::Agent));
}

/// `append` reads a stream's earlier records exactly when a draft of the batch is one rules 153
/// and 158 judge: an agent stream's `ModelInvocationRecorded` or a meter stream's reservation or
/// settlement, and no other record or stream.
#[test]
fn a_batch_reads_its_stream_only_for_the_records_the_stream_rules_judge() {
    use mandate_journal::reads_stream;
    let section = section();
    let call = base(&section, "call");
    let opened = base(&section, "opened");
    let reserved = base(&section, "reserved");
    let settled = base(&section, "settled");
    assert!(reads_stream(std::slice::from_ref(&call)));
    assert!(reads_stream(std::slice::from_ref(&reserved)));
    assert!(reads_stream(std::slice::from_ref(&settled)));
    assert!(reads_stream(&[opened.clone(), reserved]));
    assert!(!reads_stream(std::slice::from_ref(&opened)));
    assert!(!reads_stream(&[]));
}

const DEPLOYMENT: &str = "dep_01J8Z2";

/// The base draft `name` moved to the deployment meter stream, the copy's own record (journal
/// spec v0.53 §9.19): `StreamOpened` takes the deployment stream's own payload.
fn on_deployment_stream(section: &Value, name: &str, deployment: &str) -> Draft {
    parsed_on_deployment_stream(section, name, deployment).unwrap()
}

/// As [`on_deployment_stream`], before `Draft::parse`'s answer is taken.
fn parsed_on_deployment_stream(
    section: &Value,
    name: &str,
    deployment: &str,
) -> Result<Draft, mandate_journal::Invalid> {
    let mut body = section
        .get("drafts")
        .and_then(|d| d.get(name))
        .and_then(Value::as_object)
        .cloned()
        .unwrap();
    body.insert(
        Key::new("stream_id").unwrap(),
        Value::Str(format!("dmeter:{DEPLOYMENT}")),
    );
    if name == "opened" {
        let mut payload = Object::new();
        payload.insert(
            Key::new("deployment_id").unwrap(),
            Value::Str(deployment.to_owned()),
        );
        payload.insert(
            Key::new("stream_type").unwrap(),
            Value::Str("deployment_meter".to_owned()),
        );
        body.insert(Key::new("payload").unwrap(), Value::Object(payload));
    }
    Draft::parse(&to_canonical(&Value::Object(body)))
}

/// Journal spec v0.53 §2 (DEC-2207): `dmeter:{deployment_id}` is a stream type of its own, one
/// identifier segment, never a workspace's meter stream.
#[test]
fn the_deployment_meter_stream_is_its_own_stream_type() {
    let stream = StreamId::parse("dmeter:dep_01J8Z2").unwrap();
    assert_eq!(stream.stream_type(), StreamType::DeploymentMeter);
    assert_eq!(stream.as_str(), "dmeter:dep_01J8Z2");
    for refused in [
        "dmeter:",
        "dmeter:dep:extra",
        "dmeter:dep 1",
        "dmeters:dep_1",
    ] {
        assert_eq!(StreamId::parse(refused), None, "{refused}");
    }
}

/// The catalogue closes the deployment meter stream's `StreamOpened`, `MeterReserved` and
/// `MeterSettled` at version 1, and nothing else there.
#[test]
fn the_catalogue_closes_the_deployment_meter_stream_s_records_at_version_1() {
    use mandate_journal::closes;
    for record in ["StreamOpened", "MeterReserved", "MeterSettled"] {
        assert!(closes(record, 1, StreamType::DeploymentMeter), "{record}");
        assert!(
            !closes(record, 2, StreamType::DeploymentMeter),
            "{record} v2"
        );
    }
    assert!(!closes("MarkUpdated", 1, StreamType::DeploymentMeter));
    assert!(!closes(
        "ModelInvocationRecorded",
        1,
        StreamType::DeploymentMeter
    ));
}

/// Journal spec v0.53 §9.19 on the deployment meter stream: it opens naming itself (rule 154), its
/// copies keep rules 155 to 158 as the workspace's stream does, and the workspace stream's
/// `StreamOpened` is not its own.
#[test]
fn the_deployment_meter_stream_opens_naming_itself_and_keeps_rule_158() {
    let section = section();
    let opened = on_deployment_stream(&section, "opened", DEPLOYMENT);
    let reserved = on_deployment_stream(&section, "reserved", DEPLOYMENT);
    let settled = on_deployment_stream(&section, "settled", DEPLOYMENT);
    assert_eq!(
        check_stream(&[], &[opened.clone(), reserved.clone(), settled.clone()]),
        Ok(())
    );
    let judged = |batch: &[Draft]| {
        check_stream(&[], batch)
            .err()
            .map(|(i, e)| (i, e.reason.code(), e.path))
    };
    let mismatched = parsed_on_deployment_stream(&section, "opened", "dep_other")
        .err()
        .map(|e| (e.reason.code(), e.path));
    assert_eq!(
        mismatched,
        Some(("stream_mismatch", "stream_id".to_owned())),
        "rule 154"
    );
    assert_eq!(
        judged(&[opened.clone(), settled.clone()]),
        Some((1, "schema", "payload.reservation".to_owned())),
        "rule 158: an unreserved settlement"
    );
    assert_eq!(
        judged(&[opened.clone(), reserved.clone(), settled.clone(), settled]),
        Some((3, "schema", "payload.reservation".to_owned())),
        "rule 158: a second settlement"
    );
    let mut workspace_opened = section
        .get("drafts")
        .and_then(|d| d.get("opened"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap();
    workspace_opened.insert(
        Key::new("stream_id").unwrap(),
        Value::Str(format!("dmeter:{DEPLOYMENT}")),
    );
    assert!(
        Draft::parse(&to_canonical(&Value::Object(workspace_opened))).is_err(),
        "a workspace meter stream's opening is not the deployment stream's"
    );
}

/// Journal spec §2 and v0.53 §2: a stream is a workspace's exactly when its second segment is the
/// workspace's id whole, never by prefix, and the deployment meter stream is no workspace's, even
/// when its deployment id is the workspace's own segment.
#[test]
fn the_deployment_meter_stream_is_no_workspace_s_whatever_its_id() {
    use mandate_journal::workspace_owns;
    let stream = |id: &str| StreamId::parse(id).unwrap();
    for (id, owned) in [
        ("ctl:ws_1", true),
        ("agent:ws_1:agent_a", true),
        ("meter:ws_1", true),
        ("ctl:ws_10", false),
        ("acct:ws_2:acct_1", false),
        ("meter:ws_2", false),
    ] {
        assert_eq!(workspace_owns("ws_1", &stream(id)), owned, "{id}");
    }
    assert!(!workspace_owns("ws_1", &stream("dmeter:ws_1")));
    assert!(!workspace_owns("dep_01", &stream("dmeter:dep_01")));
}
