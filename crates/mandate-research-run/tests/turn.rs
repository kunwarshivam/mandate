//! One turn's gateway call (agent harness spec v0.2 §6.9 step 3, §10.1; DEC-2256; backlog E19-13
//! slice 3c): the seed's inputs exactly as the single call renders them, the run's kind, the earlier
//! turns oldest first, the menu, the spend ceiling, the deadline from the lesser of `deadline_ms`
//! and the time left, and every observation; the seed paired with the leading observations, and a
//! transcript that cannot be this turn's refused.

mod common;

use std::collections::BTreeSet;

use common::{
    AAPL_ID, AGENT, CUT_OFF, Fixture, NEWSWIRE, WORKSPACE, at, bars, instrument, news, plan,
    registry, scope, symbol,
};
use mandate_canon::encode_ulid;
use mandate_domain::AssetClass;
use mandate_gateway::money::Usd6;
use mandate_research_run::agentic::{RunKind, Tool, TurnPlan};
use mandate_research_run::{
    CallContext, CallPlan, EarlierTurn, ResearchCall, ResearchRunError, Retrieval, TurnCall,
    retrieve,
};
use mandate_time::UtcNanos;

fn seed() -> Retrieval {
    let fixture = Fixture {
        instruments: vec![instrument("AAPL", AAPL_ID)],
        bars: bars("AAPL", 2),
        news: vec![news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60)],
        ..Fixture::default()
    };
    retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"))
}

fn observation(n: usize) -> String {
    encode_ulid((1 << 101) + u128::try_from(n).unwrap_or(0))
}

fn call_plan(observations: usize) -> CallPlan {
    CallPlan {
        run_id: encode_ulid((1 << 100) + 3),
        call_id: encode_ulid((1 << 100) + 7),
        workspace_id: WORKSPACE.to_owned(),
        agent_id: AGENT.to_owned(),
        model: registry().1,
        as_of: at(CUT_OFF),
        deadline_ms: 120_000,
        max_output_tokens: 4_096,
        observations: (0..observations).map(observation).collect(),
    }
}

fn context() -> CallContext {
    CallContext {
        description: "Long-only US large caps on public news and filings.".to_owned(),
        data_universe: vec![symbol("AAPL"), symbol("MSFT")],
        asset_classes: vec![AssetClass::UsEquity],
        max_instruments: 5,
        working_universe: vec![symbol("AAPL")],
        held: Vec::new(),
        theses: Vec::new(),
    }
}

fn turn_plan(turn: u32, time_left_ms: u64) -> TurnPlan {
    TurnPlan {
        turn,
        tools: BTreeSet::from([Tool::Note, Tool::MarketBars, Tool::NewsSearch]),
        spend_ceiling: Usd6::from_micros(1_500_000),
        time_left_ms,
        turns_left: 9 - turn,
        tool_calls_left: 20,
    }
}

fn earlier(n: u8) -> EarlierTurn {
    EarlierTurn {
        response: format!("{{\"reply\":{n}}}").into_bytes(),
        results: vec![
            format!("{{\"items\":[],\"turn\":{n}}}").into_bytes(),
            b"{}".to_vec(),
        ],
    }
}

fn build(observations: usize, plan: &TurnPlan, transcript: Vec<EarlierTurn>) -> TurnCall {
    TurnCall::build(
        &call_plan(observations),
        plan,
        RunKind::Monitor,
        &seed(),
        context(),
        transcript,
        at(CUT_OFF + 5),
    )
    .unwrap_or_else(|e| panic!("{e:?}"))
}

#[test]
fn a_run_kind_is_spelled_as_the_gateway_and_the_records_spell_it() {
    assert_eq!(RunKind::Ideate.as_str(), "ideate");
    assert_eq!(RunKind::Monitor.as_str(), "monitor");
}

#[test]
fn a_turn_s_request_is_the_seed_s_call_with_the_turn_s_members() {
    let items = seed().items.len();
    let plan = turn_plan(3, 3_600_000);
    let turn = build(items + 2, &plan, vec![earlier(1), earlier(2)]);
    let request = turn.request().unwrap_or_else(|e| panic!("{e:?}"));

    let single = ResearchCall::build(&call_plan(items), &seed(), context(), at(CUT_OFF + 5))
        .and_then(|call| call.request())
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        request.inputs, single.inputs,
        "the seed's inputs exactly as the single call renders them, and no per-turn input"
    );
    assert_eq!(
        (&request.call_id, &request.workspace_id, &request.agent_id),
        (&single.call_id, &single.workspace_id, &single.agent_id)
    );
    assert_eq!(
        (request.as_of, request.purpose, &request.model),
        (single.as_of, single.purpose, &single.model)
    );
    assert_eq!(request.run_kind.as_deref(), Some("monitor"));
    assert_eq!(
        request.tools,
        ["market.bars", "news.search", "note"],
        "the menu by name, in the menu's order"
    );
    assert_eq!(request.spend_ceiling, Some(Usd6::from_micros(1_500_000)));
    let turns: Vec<(Vec<u8>, Vec<String>)> = request
        .turns
        .iter()
        .map(|turn| (turn.response.clone(), turn.results.clone()))
        .collect();
    assert_eq!(
        turns,
        [
            (
                b"{\"reply\":1}".to_vec(),
                vec![r#"{"items":[],"turn":1}"#.to_owned(), "{}".to_owned()]
            ),
            (
                b"{\"reply\":2}".to_vec(),
                vec![r#"{"items":[],"turn":2}"#.to_owned(), "{}".to_owned()]
            ),
        ],
        "oldest first, each result as the model was given it, unwrapped"
    );
    let all: Vec<String> = (0..items + 2).map(observation).collect();
    assert_eq!(
        request.observations, all,
        "every observation the prompt holds"
    );
}

#[test]
fn the_deadline_is_dispatch_plus_the_lesser_of_deadline_ms_and_the_time_left() {
    let items = seed().items.len();
    let short = build(items, &turn_plan(1, 5_000), Vec::new());
    assert_eq!(
        short.deadline,
        at(CUT_OFF + 5 + 5),
        "the run's time left binds"
    );
    let long = build(items, &turn_plan(1, 3_600_000), Vec::new());
    assert_eq!(
        long.deadline,
        at(CUT_OFF + 5 + 120),
        "the entry's deadline_ms binds"
    );
    let request = long.request().unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(request.deadline, at(CUT_OFF + 5 + 120));
    let exact = build(items, &turn_plan(1, 120_000), Vec::new());
    assert_eq!(exact.deadline, at(CUT_OFF + 5 + 120));
    let fraction = build(items, &turn_plan(1, 1_500), Vec::new());
    assert_eq!(
        fraction.deadline,
        UtcNanos::from_parts(CUT_OFF + 6, 500_000_000).unwrap_or_else(|e| panic!("{e}"))
    );
}

#[test]
fn the_seed_pairs_with_the_leading_observations() {
    let items = seed().items.len();
    let turn = build(items + 3, &turn_plan(2, 3_600_000), vec![earlier(1)]);
    let paired: Vec<String> = turn
        .seed
        .inputs
        .items
        .iter()
        .map(|item| item.observation.clone())
        .collect();
    let leading: Vec<String> = (0..items).map(observation).collect();
    assert_eq!(paired, leading);
    assert_eq!(turn.observations.len(), items + 3);

    let built = TurnCall::build(
        &call_plan(items - 1),
        &turn_plan(1, 3_600_000),
        RunKind::Monitor,
        &seed(),
        context(),
        Vec::new(),
        at(CUT_OFF + 5),
    );
    assert_eq!(
        built.map(|_| ()),
        Err(ResearchRunError::ObservationsMismatch {
            items,
            observations: items - 1,
        }),
        "a plan naming fewer observations than the seed holds items"
    );
}

#[test]
fn the_first_turn_carries_no_transcript() {
    let items = seed().items.len();
    let request = build(items, &turn_plan(1, 3_600_000), Vec::new())
        .request()
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(request.turns.is_empty());
    assert_eq!(request.run_kind.as_deref(), Some("monitor"));
}

#[test]
fn a_transcript_that_cannot_be_this_turn_s_is_refused() {
    let items = seed().items.len();
    let fitting = build(items, &turn_plan(2, 3_600_000), vec![earlier(1)]);
    assert_eq!(fitting.transcript.len(), 1, "turn 2 carries turn 1");
    let attempt = |plan: TurnPlan, transcript: Vec<EarlierTurn>| {
        TurnCall::build(
            &call_plan(items),
            &plan,
            RunKind::Ideate,
            &seed(),
            context(),
            transcript,
            at(CUT_OFF + 5),
        )
        .map(|_| ())
    };
    for (plan, transcript, why) in [
        (turn_plan(3, 3_600_000), vec![earlier(1)], "one short"),
        (
            turn_plan(2, 3_600_000),
            vec![earlier(1), earlier(2)],
            "one long",
        ),
        (
            turn_plan(1, 3_600_000),
            vec![earlier(1)],
            "a first turn with history",
        ),
    ] {
        assert!(
            matches!(
                attempt(plan, transcript),
                Err(ResearchRunError::LoopOrder { .. })
            ),
            "{why}"
        );
    }
    let mut broken = earlier(1);
    broken.results[1] = vec![0xff, 0xfe];
    assert!(
        matches!(
            attempt(turn_plan(2, 3_600_000), vec![broken]),
            Err(ResearchRunError::InvalidId { .. })
        ),
        "a result that is not UTF-8"
    );
}
