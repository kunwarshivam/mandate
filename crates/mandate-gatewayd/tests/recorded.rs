//! The service's `--recorded` mode (DEC-2326 item 3, DEC-2266, DEC-2316): each call is answered
//! with the reply its turn takes, the scripted turn of an agentic call and the recorded final reply
//! otherwise, primed on the recorded transport; and the gateway's settings are the recording's
//! endpoint under the cell's rate caps. The oracle builds each reply body itself from the
//! recording's members.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Key, Value, parse, to_canonical};
use mandate_gateway::fake::{RecordedTransport, openrouter_reply};
use mandate_gateway::guard::GuardContext;
use mandate_gateway::kinds::{KeyOwner, Purpose};
use mandate_gateway::policy::EndpointPolicy;
use mandate_gateway::provider::{Delivery, HttpRequest, ManualClock, Transport};
use mandate_gateway::registry::{Locality, ModelRef};
use mandate_gateway::tools::Turn;
use mandate_gateway::{CallRequest, RateCaps, Settings};
use mandate_gatewayd::recorded::{Replies, settings};
use mandate_research_run::recorded::RecordedModel;
use mandate_research_run::recorded_reply;
use mandate_time::UtcNanos;

const DEMO: &str = include_str!("../../../deploy/cell/demo/research/research.json");
const AGENTIC: &str =
    include_str!("../../mandate-daemon/tests/fixtures/research-agentic/research.json");

/// The demo recording as a single-call one: its entry names no loop, and it scripts no turn and no
/// monitor reply.
fn single_call_demo() -> String {
    let Value::Object(mut demo) = parse(DEMO.as_bytes()).unwrap() else {
        panic!("the recording is an object")
    };
    for member in ["turns", "monitor_reply"] {
        demo.remove(&Key::new(member).unwrap());
    }
    let Some(Value::Object(entry)) = demo.get_mut(&Key::new("entry").unwrap()) else {
        panic!("the entry is an object")
    };
    entry.remove(&Key::new("agentic").unwrap());
    String::from_utf8(to_canonical(&Value::Object(demo))).unwrap()
}

fn model(text: &str) -> RecordedModel {
    RecordedModel::read(&parse(text.as_bytes()).unwrap()).unwrap()
}

fn request(run_kind: Option<&str>, earlier_turns: usize) -> CallRequest {
    CallRequest {
        call_id: mandate_canon::encode_ulid(1 << 90),
        workspace_id: "ws_1".to_owned(),
        agent_id: Some("agent-a".to_owned()),
        purpose: Purpose::Research,
        model: ModelRef {
            model_id: "llm.research_agent".to_owned(),
            model_version: "0.1.0".to_owned(),
            content_hash: Digest::of(b"model"),
        },
        inputs: BTreeMap::new(),
        as_of: UtcNanos::from_parts(1_792_155_600, 0).unwrap(),
        deadline: UtcNanos::from_parts(1_792_155_660, 0).unwrap(),
        observations: vec![],
        key_owner: KeyOwner::Platform,
        run_kind: run_kind.map(str::to_owned),
        turns: vec![
            Turn {
                response: b"{}".to_vec(),
                results: vec![],
            };
            earlier_turns
        ],
        tools: vec![],
        spend_ceiling: None,
    }
}

/// The final reply's body, built from the recording's own members.
fn final_body(model: &RecordedModel) -> Vec<u8> {
    recorded_reply(
        &model.reply.output,
        model.probe.model.as_deref().unwrap(),
        model.probe.provider.as_deref().unwrap(),
        model.reply.usage,
    )
}

fn body(delivery: &Delivery) -> &[u8] {
    match delivery {
        Delivery::Response { status, body, .. } => {
            assert_eq!(*status, 200);
            body
        }
        other => panic!("not a response: {other:?}"),
    }
}

/// DEC-2210: a single call is answered with the recorded final reply, at its latency.
#[test]
fn a_single_call_takes_the_recorded_reply() {
    let demo = model(DEMO);
    let replies = Replies::new(&demo);
    let reply = replies.reply_for(&request(None, 0));
    assert_eq!(body(&reply.delivery), final_body(&demo).as_slice());
    assert_eq!(reply.latency_ms, demo.reply.latency_ms);
    assert_eq!(
        replies.reply_for(&request(None, 3)).delivery,
        reply.delivery,
        "a single call has no turns to script"
    );
    let agentic = model(AGENTIC);
    assert!(!agentic.turns.is_empty(), "the fixture scripts turns");
    assert_eq!(
        body(&Replies::new(&agentic).reply_for(&request(None, 0)).delivery),
        final_body(&agentic).as_slice(),
        "a call with no run kind takes the final reply even where a turn is scripted"
    );
}

/// DEC-2266: an agentic call takes the scripted turn after the turns its transcript holds, and the
/// recorded final reply once the script is spent.
#[test]
fn an_agentic_call_takes_its_turn_s_scripted_reply_then_the_final_one() {
    let agentic = model(AGENTIC);
    let replies = Replies::new(&agentic);
    assert!(agentic.turns.len() >= 2, "the fixture scripts turns");
    for (earlier, turn) in agentic.turns.iter().enumerate() {
        let calls: Vec<(&str, &str)> = turn
            .calls
            .iter()
            .map(|(name, arguments)| (name.as_str(), arguments.as_str()))
            .collect();
        let reply = replies.reply_for(&request(Some("discovery"), earlier));
        assert_eq!(
            body(&reply.delivery),
            openrouter_reply(None, &calls, &agentic.probe, turn.usage).as_slice(),
            "turn {}",
            earlier + 1
        );
        assert_eq!(reply.latency_ms, turn.latency_ms);
    }
    let last = replies.reply_for(&request(Some("discovery"), agentic.turns.len()));
    assert_eq!(body(&last.delivery), final_body(&agentic).as_slice());
}

/// Priming replaces any reply a call never took, so each call is sent exactly its own reply.
#[test]
fn priming_replaces_a_reply_no_call_took() {
    let agentic = model(AGENTIC);
    let replies = Replies::new(&agentic);
    let mut transport = RecordedTransport::new();
    let mut clock = ManualClock::at(UtcNanos::from_parts(1_792_155_600, 0).unwrap());
    let send = |transport: &mut RecordedTransport, clock: &mut ManualClock| {
        transport.send(
            &HttpRequest {
                host: "openrouter.ai".to_owned(),
                path: "/api/v1/chat/completions".to_owned(),
                body: b"{}".to_vec(),
            },
            None,
            UtcNanos::from_parts(1_792_155_660, 0).unwrap(),
            clock,
        )
    };
    replies.prime(&mut transport, &request(Some("discovery"), 0));
    replies.prime(&mut transport, &request(None, 0));
    assert_eq!(
        body(&send(&mut transport, &mut clock)),
        final_body(&agentic).as_slice(),
        "the later priming's reply, not the earlier one's"
    );
    assert_eq!(
        send(&mut transport, &mut clock),
        Delivery::ConnectFailed,
        "nothing left scripted"
    );
}

/// DEC-2316: the gateway's settings are the recording's endpoint, hosted and through its one
/// upstream, with the workspace's rate fixed at 12 a minute and the agent's at the turns one run
/// may take plus two.
#[test]
fn the_settings_are_the_recording_s_endpoint_under_the_cell_s_rate_caps() {
    for (text, turns) in [
        (single_call_demo(), 1),
        (
            DEMO.to_owned(),
            model(DEMO).entry.agentic.unwrap().max_turns,
        ),
        (
            AGENTIC.to_owned(),
            model(AGENTIC).entry.agentic.unwrap().max_turns,
        ),
    ] {
        let recorded = model(&text);
        assert_eq!(
            settings("ws_1", &recorded),
            Settings {
                workspace_id: "ws_1".to_owned(),
                policy: EndpointPolicy::new(
                    BTreeSet::from([Locality::Hosted]),
                    Some(BTreeSet::from([recorded.endpoint.route.upstream.clone()])),
                    None,
                ),
                guard: GuardContext::default(),
                rate: RateCaps {
                    per_workspace: 12,
                    per_agent: turns + 2,
                },
            }
        );
    }
}

const INVALIDATION: &str =
    r#"{\"invalidations\":[{\"thesis_id\":\"01ACTIVE\",\"evidence\":[\"nw-1\"]}]}"#;

/// `text`, a recording, with a `monitor_reply` whose output is [`INVALIDATION`].
fn with_monitor_reply(text: &str) -> String {
    let end = text.rfind('}').unwrap();
    format!(
        r#"{},"monitor_reply":{{"output":"{INVALIDATION}","latency_ms":7,"input_tokens":900,"output_tokens":40,"cached_tokens":0}}}}"#,
        &text[..end]
    )
}

/// Agent harness spec §6.9: a `monitor` run's call takes the recording's monitor reply, at any
/// turn, with no scripted turn, and an `ideate` run's calls take their scripted turns and the final
/// reply as before. A recording with no monitor reply answers a monitor call as before.
#[test]
fn a_monitor_call_takes_the_monitor_reply_and_an_ideate_call_its_script() {
    let recording = model(&with_monitor_reply(AGENTIC));
    let monitor = recording.monitor_reply.clone().unwrap();
    assert_eq!(
        monitor.output,
        r#"{"invalidations":[{"thesis_id":"01ACTIVE","evidence":["nw-1"]}]}"#
    );
    let replies = Replies::new(&recording);
    let monitor_body = recorded_reply(
        &monitor.output,
        recording.probe.model.as_deref().unwrap(),
        recording.probe.provider.as_deref().unwrap(),
        monitor.usage,
    );
    for earlier in [0, 1, recording.turns.len()] {
        let reply = replies.reply_for(&request(Some("monitor"), earlier));
        assert_eq!(body(&reply.delivery), monitor_body.as_slice(), "{earlier}");
        assert_eq!(reply.latency_ms, 7);
    }
    let first = &recording.turns[0];
    let calls: Vec<(&str, &str)> = first
        .calls
        .iter()
        .map(|(name, arguments)| (name.as_str(), arguments.as_str()))
        .collect();
    assert_eq!(
        body(&replies.reply_for(&request(Some("ideate"), 0)).delivery),
        openrouter_reply(None, &calls, &recording.probe, first.usage).as_slice()
    );
    assert_eq!(
        body(
            &replies
                .reply_for(&request(Some("ideate"), recording.turns.len()))
                .delivery
        ),
        final_body(&recording).as_slice()
    );
    assert_eq!(
        body(&replies.reply_for(&request(None, 0)).delivery),
        final_body(&recording).as_slice()
    );
    let plain = model(AGENTIC);
    assert_eq!(plain.monitor_reply, None);
    assert_eq!(
        body(
            &Replies::new(&plain)
                .reply_for(&request(Some("monitor"), 0))
                .delivery
        ),
        openrouter_reply(None, &calls, &plain.probe, first.usage).as_slice(),
        "no monitor reply: the script, as before"
    );
}

/// A monitor reply needs an agentic entry, whose runs alone can be `monitor` runs, and every
/// member a reply has.
#[test]
fn a_monitor_reply_is_refused_on_a_single_call_entry_or_short_a_member() {
    let refused = |text: &str| RecordedModel::read(&parse(text.as_bytes()).unwrap()).err();
    assert_eq!(
        refused(&with_monitor_reply(&single_call_demo())),
        Some(mandate_research_run::recorded::RecordedError::Malformed(
            "monitor_reply"
        ))
    );
    let short = with_monitor_reply(AGENTIC).replace(r#","latency_ms":7"#, "");
    assert_eq!(
        refused(&short),
        Some(mandate_research_run::recorded::RecordedError::Malformed(
            "latency_ms"
        ))
    );
}

/// The monitor reply names the least thesis the call's `memory` input shows, so the recorded double
/// invalidates a thesis the runtime actually holds.
#[test]
fn a_monitor_reply_names_the_thesis_the_call_showed() {
    let recording = model(&with_monitor_reply(AGENTIC).replace("01ACTIVE", "$THESIS_ID"));
    let replies = Replies::new(&recording);
    let mut monitor = request(Some("monitor"), 0);
    monitor.inputs.insert(
        "memory".to_owned(),
        r#"[{"expires_at":"2026-10-20T00:00:00.000000000Z","symbol":"SPY","thesis_id":"01THESISB"},{"expires_at":"2026-10-20T00:00:00.000000000Z","symbol":"SPY","thesis_id":"01THESISA"}]"#
            .to_owned(),
    );
    let reply = replies.reply_for(&monitor);
    let expected = recorded_reply(
        r#"{"invalidations":[{"thesis_id":"01THESISA","evidence":["nw-1"]}]}"#,
        recording.probe.model.as_deref().unwrap(),
        recording.probe.provider.as_deref().unwrap(),
        recording.monitor_reply.as_ref().unwrap().usage,
    );
    assert_eq!(body(&reply.delivery), expected.as_slice());
}
