//! The wire form of one call between the runtime and the model-gateway service (DEC-2265 item 5):
//! every request and every reply reads back as itself, its bytes are canonical JSON and the same
//! when written again, the format is pinned by two frames written out by hand, and a frame that is
//! not exactly what the encoder writes is refused.

use std::collections::BTreeMap;

use mandate_canon::{Digest, Int, Key, Object, Value, parse, to_canonical};
use mandate_gateway::call::{CallRequest, RequestError};
use mandate_gateway::guard::GuardRule;
use mandate_gateway::kinds::{KeyOwner, Outcome, Purpose};
use mandate_gateway::money::{Usage, Usd6};
use mandate_gateway::record::CallRecord;
use mandate_gateway::registry::ModelRef;
use mandate_gateway::tools::{ToolCall, Turn};
use mandate_gateway::wire::{
    Reply, WireError, reply_bytes, reply_from, request_bytes, request_from,
};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

const PURPOSES: [Purpose; 4] = [
    Purpose::Research,
    Purpose::LlmSignal,
    Purpose::FastSignal,
    Purpose::Compiler,
];
const GUARD_RULES: [GuardRule; 5] = [
    GuardRule::Credential,
    GuardRule::AccountIdentifier,
    GuardRule::PersonalData,
    GuardRule::ForeignWorkspace,
    GuardRule::InputContract,
];
const REFUSALS: [RequestError; 9] = [
    RequestError::ForeignWorkspace,
    RequestError::AgentPurpose,
    RequestError::CallId,
    RequestError::UnknownModel,
    RequestError::HashMismatch,
    RequestError::NotAgentic,
    RequestError::RunKind,
    RequestError::Tools,
    RequestError::Transcript,
];

/// A runner of 256 generated cases that saves no failure seed (DEC-381).
fn runner() -> TestRunner {
    TestRunner::new(Config {
        cases: 256,
        failure_persistence: None,
        ..Config::default()
    })
}

fn model() -> ModelRef {
    ModelRef {
        model_id: "llm.research_agent".to_owned(),
        model_version: "0.1.0".to_owned(),
        content_hash: Digest::of(b"model"),
    }
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

fn time() -> impl Strategy<Value = UtcNanos> {
    (0_i64..4_102_444_800, 0_u32..1_000_000_000)
        .prop_map(|(secs, nanos)| UtcNanos::from_parts(secs, nanos).unwrap())
}

fn digest() -> impl Strategy<Value = Digest> {
    any::<[u8; 8]>().prop_map(|seed| Digest::of(&seed))
}

/// Any text, unicode and control characters among it.
fn text() -> impl Strategy<Value = String> {
    "\\PC{0,12}|[\\x00-\\x1f\"\\\\]{0,4}"
}

fn bytes() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 0..64)
}

/// A JSON value the canonical form holds: no floats, integers up to 2^53 − 1, canonical keys.
fn value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        (0_u64..(1 << 53)).prop_map(|n| Value::Int(Int::new(n).unwrap())),
        text().prop_map(Value::Str),
    ];
    leaf.prop_recursive(3, 16, 4, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            proptest::collection::btree_map("[a-z][a-z0-9_]{0,6}", inner, 0..4).prop_map(
                |members| {
                    Value::Object(
                        members
                            .into_iter()
                            .map(|(k, v)| (Key::new(&k).unwrap(), v))
                            .collect::<Object>(),
                    )
                }
            ),
        ]
    })
}

fn request() -> impl Strategy<Value = CallRequest> {
    (
        (text(), text(), proptest::option::of(text()), 0_usize..4),
        (
            proptest::collection::btree_map(text(), text(), 0..4),
            time(),
            time(),
            proptest::collection::vec(text(), 0..3),
            any::<bool>(),
        ),
        (
            proptest::option::of(text()),
            proptest::collection::vec(
                (bytes(), proptest::collection::vec(text(), 0..2))
                    .prop_map(|(response, results)| Turn { response, results }),
                0..3,
            ),
            proptest::collection::vec(text(), 0..3),
            proptest::option::of(any::<u64>().prop_map(Usd6::from_micros)),
        ),
    )
        .prop_map(
            |(
                (call_id, workspace_id, agent_id, purpose),
                (inputs, as_of, deadline, observations, platform),
                (run_kind, turns, tools, spend_ceiling),
            )| CallRequest {
                call_id,
                workspace_id,
                agent_id,
                purpose: PURPOSES[purpose],
                model: model(),
                inputs,
                as_of,
                deadline,
                observations,
                key_owner: if platform {
                    KeyOwner::Platform
                } else {
                    KeyOwner::Customer
                },
                run_kind,
                turns,
                tools,
                spend_ceiling,
            },
        )
}

fn record() -> impl Strategy<Value = CallRecord> {
    (
        (
            text(),
            proptest::option::of(text()),
            0_usize..4,
            text(),
            proptest::option::of(digest()),
            proptest::option::of(0_usize..5),
            proptest::option::of(any::<u64>()),
        ),
        (
            proptest::option::of(digest()),
            proptest::option::of(digest()),
            proptest::option::of(text()),
            proptest::option::of(text()),
            0_usize..Outcome::ALL.len(),
            any::<u32>(),
            (any::<u64>(), any::<u64>(), any::<u64>()),
        ),
        (
            any::<u64>(),
            digest(),
            any::<bool>(),
            time(),
            time(),
            proptest::option::of(text()),
            proptest::collection::vec(text(), 0..3),
        ),
        (
            proptest::option::of(value()),
            proptest::collection::vec(
                (any::<u32>(), text(), value()).prop_map(|(index, name, arguments)| ToolCall {
                    index,
                    name,
                    arguments,
                }),
                0..3,
            ),
            proptest::option::of(bytes()),
            proptest::option::of(bytes()),
        ),
    )
        .prop_map(
            |(
                (call_id, agent_id, purpose, endpoint, request_digest, guard, seed),
                (
                    prompt_ref,
                    response_ref,
                    reported_identity,
                    provider_request_id,
                    outcome,
                    attempts,
                    (input, output, cached),
                ),
                (
                    cost,
                    price_table_ref,
                    cache_hit,
                    deadline,
                    completed_at,
                    reservation,
                    observations,
                ),
                (output_value, tool_calls, prompt, response),
            )| CallRecord {
                call_id,
                agent_id,
                purpose: PURPOSES[purpose],
                model: model(),
                endpoint,
                request_digest,
                guard_rule: guard.map(|g| GUARD_RULES[g]),
                seed,
                prompt_ref,
                response_ref,
                reported_identity,
                provider_request_id,
                outcome: Outcome::ALL[outcome],
                attempts,
                tokens: Usage {
                    input,
                    output,
                    cached,
                },
                cost: Usd6::from_micros(cost),
                price_table_ref,
                cache_hit,
                deadline,
                completed_at,
                reservation,
                observations,
                output: output_value,
                tool_calls,
                prompt,
                response,
            },
        )
}

fn reply() -> impl Strategy<Value = Reply> {
    prop_oneof![
        record().prop_map(Ok),
        (0_usize..REFUSALS.len()).prop_map(|i| Err(REFUSALS[i])),
    ]
}

/// DEC-2265 item 5: any request reads back from its bytes as itself, and its bytes are canonical
/// JSON that encode the same again.
#[test]
fn any_request_reads_back_as_itself_from_canonical_bytes() {
    runner()
        .run(&request(), |request| {
            let bytes = request_bytes(&request);
            prop_assert_eq!(to_canonical(&parse(&bytes).unwrap()), bytes.clone());
            let read = request_from(&bytes);
            prop_assert_eq!(read.as_ref(), Ok(&request));
            prop_assert_eq!(request_bytes(&read.unwrap()), bytes);
            Ok(())
        })
        .unwrap_or_else(|failure| panic!("{failure}"));
}

/// DEC-2265 items 5 and 6: any reply, a whole record with its prompt and response bytes or a
/// refusal, reads back from its bytes as itself, and its bytes are canonical JSON.
#[test]
fn any_reply_reads_back_as_itself_from_canonical_bytes() {
    runner()
        .run(&reply(), |reply| {
            let bytes = reply_bytes(&reply);
            prop_assert_eq!(to_canonical(&parse(&bytes).unwrap()), bytes.clone());
            prop_assert_eq!(reply_from(&bytes), Ok(reply));
            Ok(())
        })
        .unwrap_or_else(|failure| panic!("{failure}"));
}

fn small_request() -> CallRequest {
    CallRequest {
        call_id: "01J8Z3MR000000000000000001".to_owned(),
        workspace_id: "ws1".to_owned(),
        agent_id: Some("agent-a".to_owned()),
        purpose: Purpose::Research,
        model: model(),
        inputs: BTreeMap::from([
            ("description".to_owned(), "Long-only".to_owned()),
            ("not a key!".to_owned(), "x".to_owned()),
        ]),
        as_of: at(1_792_159_200),
        deadline: at(1_792_159_320),
        observations: vec!["obs-1".to_owned()],
        key_owner: KeyOwner::Platform,
        run_kind: None,
        turns: vec![Turn {
            response: vec![0x0a, 0xff],
            results: vec!["r".to_owned()],
        }],
        tools: Vec::new(),
        spend_ceiling: Some(Usd6::from_micros(250_000)),
    }
}

/// DEC-2265 item 5, pinned: a request and a refusal written out by hand, member by member, are the
/// encoder's bytes exactly.
#[test]
fn the_wire_form_is_the_one_written_out_here() {
    let hash = Digest::of(b"model").to_hex();
    let request = format!(
        concat!(
            r#"{{"agent_id":"agent-a","as_of":"2026-10-16T14:00:00.000000000Z","#,
            r#""call_id":"01J8Z3MR000000000000000001","deadline":"2026-10-16T14:02:00.000000000Z","#,
            r#""inputs":[["description","Long-only"],["not a key!","x"]],"key_owner":"platform","#,
            r#""model":{{"content_hash":"sha256:{hash}","model_id":"llm.research_agent","model_version":"0.1.0"}},"#,
            r#""observations":["obs-1"],"purpose":"research","run_kind":null,"spend_ceiling":"0.25","#,
            r#""tools":[],"turns":[{{"response":"0aff","results":["r"]}}],"wire_version":1,"workspace_id":"ws1"}}"#
        ),
        hash = hash
    );
    assert_eq!(
        String::from_utf8(request_bytes(&small_request())).unwrap(),
        request
    );
    assert_eq!(request_from(request.as_bytes()), Ok(small_request()));
    let refused = r#"{"refused":"unknown_model","wire_version":1}"#;
    assert_eq!(
        reply_bytes(&Err(RequestError::UnknownModel)),
        refused.as_bytes()
    );
    assert_eq!(
        reply_from(refused.as_bytes()),
        Ok(Err(RequestError::UnknownModel))
    );
}

fn reencoded(object: &Object) -> Vec<u8> {
    to_canonical(&Value::Object(object.clone()))
}

/// DEC-2265 item 5: a frame that is not exactly what the encoder writes reads as nothing: not JSON,
/// not canonical, another version, a member missing or unlisted, or a value out of range.
#[test]
fn a_frame_the_encoder_would_not_write_reads_as_nothing() {
    let bytes = request_bytes(&small_request());
    let Value::Object(members) = parse(&bytes).unwrap() else {
        unreachable!("a request is an object")
    };
    assert!(request_from(b"not json").is_err());
    assert_eq!(
        request_from(&[b" ".as_slice(), &bytes].concat()),
        Err(WireError::NotCanonical),
        "leading whitespace is not the canonical form"
    );
    for member in members.keys() {
        let mut missing = members.clone();
        missing.remove(member);
        assert!(
            request_from(&reencoded(&missing)).is_err(),
            "without {}",
            member.as_str()
        );
    }
    let mut extra = members.clone();
    extra.insert(Key::new("note").unwrap(), Value::Null);
    assert!(
        request_from(&reencoded(&extra)).is_err(),
        "an unlisted member"
    );
    for (member, value) in [
        ("wire_version", Value::Int(Int::new(2).unwrap())),
        ("purpose", Value::Str("trading".to_owned())),
        ("key_owner", Value::Str("nobody".to_owned())),
        ("as_of", Value::Str("2026-10-16T14:00:00Z".to_owned())),
        ("spend_ceiling", Value::Str("0.250".to_owned())),
        ("spend_ceiling", Value::Str("-1".to_owned())),
        ("inputs", Value::Array(vec![Value::Str("solo".to_owned())])),
        ("turns", Value::Array(vec![Value::Null])),
    ] {
        let mut bad = members.clone();
        bad.insert(Key::new(member).unwrap(), value.clone());
        assert!(
            request_from(&reencoded(&bad)).is_err(),
            "{member} = {value:?}"
        );
    }
    let mut odd_hex = members.clone();
    odd_hex.insert(
        Key::new("turns").unwrap(),
        parse(br#"[{"response":"0AFF","results":[]}]"#).unwrap(),
    );
    assert!(request_from(&reencoded(&odd_hex)).is_err(), "uppercase hex");
    for refused in [
        r#"{"refused":"budget_exhausted","wire_version":1}"#,
        r#"{"refused":"unknown_model","wire_version":2}"#,
        r#"{"wire_version":1}"#,
        r#"{"record":null,"refused":"tools","wire_version":1}"#,
    ] {
        assert!(reply_from(refused.as_bytes()).is_err(), "{refused}");
    }
}
