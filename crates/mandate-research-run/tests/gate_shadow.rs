//! The service's record of a gate run (DEC-2217 item 4, DEC-2218): an honest call for each turn
//! fits, and a call that differs in any member but the runtime's own journal ids, or comes out of
//! turn, is unmatched; the record drives the loop from the service's own replies, dispatching each
//! read over the run's corpus slice itself.

use mandate_canon::{Value, encode_ulid, parse};
use mandate_gateway::CallRequest;
use mandate_gateway::kinds::Purpose;
use mandate_gateway::money::Usd6;
use mandate_gateway::tools::ToolCall;
use mandate_research_run::ResearchEntry;
use mandate_research_run::agentic::{EndReason, RunKind};
use mandate_research_run::gate::ProbeSet;
use mandate_research_run::shadow::{Answered, Shadow, Step, Unmatched};

const WORKSPACE: &str = "ws_gate";

fn entry() -> ResearchEntry {
    let text = r#"{"agentic":"v0_2","deadline_ms":120000,"max_output_tokens":2000,
        "model_id":"llm.research_sonnet","model_name":"anthropic/claude-sonnet-5.5",
        "model_version":"1.0.0","provider":"anthropic","seed":7,"snapshot":"claude-sonnet-5-5"}"#;
    ResearchEntry::recorded(&parse(text.as_bytes()).unwrap()).unwrap()
}

fn ids(from: u128, count: usize) -> Vec<String> {
    (0..count)
        .map(|n| encode_ulid(from + u128::try_from(n).unwrap()))
        .collect()
}

/// The honest call for the record's next turn: earlier observations kept, one new one for each
/// item due, and the run's thesis ids.
fn honest(shadow: &Shadow, earlier: &[String], from: u128, theses: &[String]) -> CallRequest {
    let mut observations = earlier.to_vec();
    observations.extend(ids(from, shadow.observations_due()));
    shadow
        .expected(&encode_ulid(1 << 100), observations, theses, at())
        .unwrap()
}

fn at() -> mandate_time::UtcNanos {
    mandate_time::UtcNanos::parse("2026-10-20T13:30:01.000000000Z").unwrap()
}

fn reads() -> Answered {
    Answered {
        ok: true,
        cost: Usd6::parse("0.01").unwrap(),
        tool_calls: vec![ToolCall {
            index: 0,
            name: "market.bars".to_owned(),
            arguments: parse(br#"{"count":5,"symbol":"SPY"}"#).unwrap(),
        }],
        response: b"{\"turn\":1}".to_vec(),
        output: None,
    }
}

fn final_output() -> Answered {
    Answered {
        ok: true,
        cost: Usd6::parse("0.02").unwrap(),
        tool_calls: Vec::new(),
        response: b"{\"turn\":2}".to_vec(),
        output: Some(parse(br#"{"theses":[]}"#).unwrap()),
    }
}

fn started(index: usize) -> Shadow {
    Shadow::start(&ProbeSet::built_in().unwrap(), index, &entry(), WORKSPACE).unwrap()
}

/// An honest ideate run: turn 1 fits, a read is dispatched over the corpus by the record itself,
/// turn 2 carries that read's result and fits, and the final output ends the run `done`.
#[test]
fn an_honest_run_fits_turn_by_turn_and_ends_on_its_final_output() {
    let mut shadow = started(0);
    assert_eq!(shadow.kind(), RunKind::Ideate);
    assert!(shadow.observations_due() > 0, "the seed is observed first");
    let first = honest(&shadow, &[], 1 << 90, &[]);
    assert_eq!(first.purpose, Purpose::Evaluation);
    assert_eq!(first.agent_id, None);
    shadow.check(&first).unwrap();
    assert_eq!(shadow.reply(&reads()), Ok(Step::Turn));
    let second = honest(&shadow, &first.observations, 1 << 91, &[]);
    assert_eq!(second.turns.len(), 1, "the transcript holds turn 1");
    assert_eq!(second.turns[0].response, reads().response);
    assert!(
        second.turns[0].results[0].contains("SPY"),
        "the read's result is the record's own dispatch over the corpus"
    );
    shadow.check(&second).unwrap();
    assert_eq!(
        shadow.reply(&final_output()),
        Ok(Step::Ended {
            reason: EndReason::Done(None),
            output: final_output().output,
        })
    );
    assert!(shadow.ended());
    assert_eq!(
        shadow.check(&second),
        Err(Unmatched("turn")),
        "nothing after the end"
    );
}

/// Every member a Job could steer the model with is held to the record's: a call that differs in
/// any of them is unmatched, naming it.
#[test]
fn a_call_that_differs_in_any_member_is_unmatched() {
    let shadow = started(0);
    let base = honest(&shadow, &[], 1 << 90, &[]);
    let edits: [(&str, fn(&mut CallRequest)); 11] = [
        ("purpose", |r| r.purpose = Purpose::Research),
        ("agent_id", |r| r.agent_id = Some("agent".to_owned())),
        ("workspace_id", |r| r.workspace_id = "ws_other".to_owned()),
        ("as_of", |r| r.as_of = mandate_time::UtcNanos::EPOCH),
        ("run_kind", |r| r.run_kind = Some("monitor".to_owned())),
        ("inputs", |r| {
            let context = r.inputs.get_mut("context").unwrap();
            *context = context.replace("Long-only", "Leveraged");
        }),
        ("tools", |r| r.tools.push("trade.place".to_owned())),
        ("spend_ceiling", |r| {
            r.spend_ceiling = Some(Usd6::parse("100").unwrap())
        }),
        ("observations", |r| {
            r.observations[0] = "not-a-ulid".to_owned()
        }),
        ("observations", |r| {
            let first = r.observations[0].clone();
            r.observations[1] = first;
        }),
        ("observations", |r| {
            r.observations.pop();
        }),
    ];
    for (member, edit) in edits {
        let mut request = base.clone();
        edit(&mut request);
        let mut fresh = shadow.clone();
        assert_eq!(fresh.check(&request), Err(Unmatched(member)), "{member}");
    }
}

/// The transcript is the service's own: a later turn whose earlier response or result differs
/// from the record's is unmatched, and a call before the reply, or a second one for a turn, is out
/// of turn.
#[test]
fn a_later_turn_must_carry_the_records_own_transcript() {
    let mut shadow = started(0);
    let first = honest(&shadow, &[], 1 << 90, &[]);
    shadow.check(&first).unwrap();
    assert_eq!(
        shadow.check(&first),
        Err(Unmatched("turn")),
        "a second call for turn 1"
    );
    shadow.reply(&reads()).unwrap();
    let second = honest(&shadow, &first.observations, 1 << 91, &[]);
    let mut response = second.clone();
    response.turns[0].response = b"{\"turn\":\"edited\"}".to_vec();
    assert_eq!(shadow.clone().check(&response), Err(Unmatched("turns")));
    let mut result = second.clone();
    result.turns[0].results[0] = "{\"injected\":true}".to_owned();
    assert_eq!(shadow.clone().check(&result), Err(Unmatched("turns")));
    let mut renamed = second;
    renamed.observations[0] = encode_ulid(7);
    assert_eq!(
        shadow.check(&renamed),
        Err(Unmatched("observations")),
        "an earlier turn's observation is kept"
    );
}

/// A monitoring run's memory holds its probe theses, each under the runtime's own id, a ULID, and
/// the same id on every turn.
#[test]
fn a_monitoring_runs_thesis_ids_are_the_runtimes_and_stay_the_same() {
    let mut shadow = started(5);
    assert_eq!(shadow.kind(), RunKind::Monitor);
    let theses = ids(1 << 95, 1);
    let first = honest(&shadow, &[], 1 << 90, &theses);
    assert!(first.inputs["memory"].contains(&theses[0]));
    let mut not_ulid = first.clone();
    let memory = not_ulid.inputs.get_mut("memory").unwrap();
    *memory = memory.replace(&theses[0], "th-probe-1");
    assert_eq!(shadow.clone().check(&not_ulid), Err(Unmatched("inputs")));
    shadow.check(&first).unwrap();
    shadow.reply(&reads()).unwrap();
    let other = ids(1 << 96, 1);
    let renamed = honest(&shadow, &first.observations, 1 << 91, &other);
    assert_eq!(
        shadow.clone().check(&renamed),
        Err(Unmatched("inputs")),
        "the thesis keeps its id across turns"
    );
    let kept = honest(&shadow, &first.observations, 1 << 91, &theses);
    shadow.check(&kept).unwrap();
}

/// A reply that failed, or names a tool off the run's menu, ends the run `error`.
#[test]
fn a_failed_reply_or_one_off_the_menu_ends_the_run_in_error() {
    for answered in [
        Answered {
            ok: false,
            ..reads()
        },
        Answered {
            tool_calls: vec![ToolCall {
                index: 0,
                name: "trade.place".to_owned(),
                arguments: Value::Null,
            }],
            ..reads()
        },
    ] {
        let mut shadow = started(0);
        shadow.check(&honest(&shadow, &[], 1 << 90, &[])).unwrap();
        assert_eq!(
            shadow.reply(&answered),
            Ok(Step::Ended {
                reason: EndReason::Error,
                output: None,
            })
        );
    }
    let mut early = started(0);
    assert_eq!(
        early.reply(&reads()),
        Err(Unmatched("turn")),
        "no reply before a call"
    );
}
