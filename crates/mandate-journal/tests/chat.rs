//! Journal spec §9.39 (DEC-2293): the chat's records on the control stream, with rules 330 to 334.
//! Each well-formed record parses; each rule refuses its case.

use mandate_canon::{parse, to_canonical};
use mandate_journal::Draft;

const HASH: &str = "sha256:5555555555555555555555555555555555555555555555555555555555555555";

fn envelope(event_type: &str, actor: &str, payload: &str, refs: &[&str]) -> Vec<u8> {
    let refs: Vec<String> = refs.iter().map(|r| format!("\"{r}\"")).collect();
    let build = if actor == "user" {
        "null"
    } else {
        r#""sha256:0000000000000000000000000000000000000000000000000000000000000000""#
    };
    let text = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"01JAA3P4A000000000000000E1","stream_id":"ctl:ws_01J8Z2","event_type":"{event_type}","schema_version":1,"event_time":"2026-10-29T15:00:05.250000000Z","clock_source":"local","causation_id":null,"correlation_id":null,"actor":{{"kind":"{actor}","id":"01JAA2N0A000000000000000P1","version":"1","build":{build}}},"config_refs":{{}},"payload":{payload},"artifact_refs":[{}],"pii_refs":[]}}"#,
        refs.join(",")
    );
    to_canonical(&parse(text.as_bytes()).unwrap())
}

fn owner_message(text: &str, card: &str, proactive: bool) -> String {
    format!(
        r#"{{"agent_id":"agent_a","setup_id":null,"member":"01JAA2N0A000000000000000P1","author":"owner","text":{text},"card":{card},"sources":[],"run_id":null,"proactive":{proactive}}}"#
    )
}

fn agent_message(text: &str, card: &str) -> String {
    format!(
        r#"{{"agent_id":"agent_a","setup_id":null,"member":"01JAA2N0A000000000000000P1","author":"agent","text":{text},"card":{card},"sources":[{{"label":"Your order","ref":"ev:01JAA3P4A000000000000000E9"}}],"run_id":"run_1","proactive":true}}"#
    )
}

fn turn(kind: &str, agent: &str, setup: &str) -> String {
    format!(
        r#"{{"agent_id":{agent},"setup_id":{setup},"member":"01JAA2N0A000000000000000P1","run_id":"run_1","turn":0,"kind":"{kind}","call_id":"01JAA3P4A000000000000000C1","model":{{"model_id":"llm.chat_sonnet","model_version":"1.0.0","content_hash":"{HASH}"}},"outcome":"ok","cost_usd":"0.0123","prompt_ref":null,"response_ref":null,"calls":[{{"name":"order.buy","arguments":{{"quantity":"10","symbol":"AAPL"}}}}]}}"#
    )
}

fn parses(event_type: &str, actor: &str, payload: &str, refs: &[&str]) -> Result<(), String> {
    Draft::parse(&envelope(event_type, actor, payload, refs))
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn every_chat_record_parses() {
    let cases = [
        ("ChatMessagePosted", "user", owner_message(r#""buy 10 AAPL""#, "null", false), vec![]),
        (
            "ChatMessagePosted",
            "system",
            agent_message(r#""Bought 10 AAPL.""#, r#"{"kind":"order","state":"filled"}"#),
            vec![],
        ),
        ("ChatTurnRecorded", "system", turn("agent", r#""agent_a""#, "null"), vec![HASH]),
        ("ChatTurnRecorded", "system", turn("setup", "null", r#""setup_1""#), vec![HASH]),
        (
            "ChatToolResultsRecorded",
            "system",
            r#"{"agent_id":"agent_a","setup_id":null,"member":"m","run_id":"run_1","turn":0,"results":[{"name":"order.buy","result":"{\"ok\":true}"}]}"#.to_owned(),
            vec![],
        ),
        (
            "ChatRunEnded",
            "system",
            r#"{"agent_id":"agent_a","setup_id":null,"member":"m","run_id":"run_1","reason":"answered"}"#.to_owned(),
            vec![],
        ),
        (
            "ChatRunStopRequested",
            "user",
            r#"{"agent_id":"agent_a","setup_id":null,"member":"m","run_id":"run_1"}"#.to_owned(),
            vec![],
        ),
        (
            "ChatSettingsChanged",
            "user",
            r#"{"agent_id":"agent_a","chat_model":"llm.chat_haiku","daily_cap_usd":"5"}"#.to_owned(),
            vec![],
        ),
        (
            "AgentGoalsUpdated",
            "system",
            r#"{"agent_id":"agent_a","run_id":"run_1","goals":[{"text":"Buy 5 NVDA under 120","status":"active"}],"plan":null,"watching":["NVDA"]}"#.to_owned(),
            vec![],
        ),
    ];
    let failed: Vec<String> = cases
        .iter()
        .filter_map(|(event, actor, payload, refs)| {
            parses(event, actor, payload, refs)
                .err()
                .map(|e| format!("{event}: {e}"))
        })
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn a_thread_names_one_agent_or_one_setup() {
    for (agent, setup) in [("null", "null"), (r#""agent_a""#, r#""setup_1""#)] {
        let payload = format!(
            r#"{{"agent_id":{agent},"setup_id":{setup},"member":"m","run_id":"run_1","reason":"answered"}}"#
        );
        assert!(
            parses("ChatRunEnded", "system", &payload, &[]).is_err(),
            "{agent} {setup}"
        );
    }
}

#[test]
fn only_the_owner_writes_the_owners_records_and_only_the_system_the_agents() {
    assert!(
        parses(
            "ChatMessagePosted",
            "system",
            &owner_message(r#""hi""#, "null", false),
            &[]
        )
        .is_err()
    );
    assert!(
        parses(
            "ChatMessagePosted",
            "user",
            &agent_message(r#""hi""#, "null"),
            &[]
        )
        .is_err()
    );
    assert!(
        parses(
            "ChatTurnRecorded",
            "user",
            &turn("agent", r#""agent_a""#, "null"),
            &[HASH]
        )
        .is_err()
    );
    let settings = r#"{"agent_id":"agent_a","chat_model":"llm.chat_haiku","daily_cap_usd":"5"}"#;
    assert!(parses("ChatSettingsChanged", "system", settings, &[]).is_err());
}

#[test]
fn an_owners_message_is_text_only_and_an_agents_carries_something() {
    assert!(
        parses(
            "ChatMessagePosted",
            "user",
            &owner_message("null", "null", false),
            &[]
        )
        .is_err()
    );
    assert!(
        parses(
            "ChatMessagePosted",
            "user",
            &owner_message(r#""hi""#, r#"{"kind":"order"}"#, false),
            &[]
        )
        .is_err(),
        "an owner never posts a card"
    );
    assert!(
        parses(
            "ChatMessagePosted",
            "user",
            &owner_message(r#""hi""#, "null", true),
            &[]
        )
        .is_err()
    );
    assert!(
        parses(
            "ChatMessagePosted",
            "system",
            &agent_message("null", "null"),
            &[]
        )
        .is_err()
    );
}

#[test]
fn a_setup_turn_is_the_setup_kind_and_an_agents_is_not() {
    assert!(
        parses(
            "ChatTurnRecorded",
            "system",
            &turn("setup", r#""agent_a""#, "null"),
            &[HASH]
        )
        .is_err()
    );
    assert!(
        parses(
            "ChatTurnRecorded",
            "system",
            &turn("agent", "null", r#""setup_1""#),
            &[HASH]
        )
        .is_err()
    );
}
