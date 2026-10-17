//! The one call (§6.2 step 3, §10.1): built only from journaled items, its deadline from the
//! dispatch instant, and a reply re-checked against the request (INF-2, INF-3, caller side), and
//! §7.3's treatment of each failure kind.

#![allow(
    clippy::field_reassign_with_default,
    reason = "each case starts from the empty fixture and states only the reads it is about"
)]

mod common;

use common::{
    AAPL_ID, CUT_OFF, Fixture, NEWSWIRE, at, bars, instrument, news, plan, scope, symbol,
};
use mandate_canon::Digest;
use mandate_domain::AssetClass;
use mandate_num::Usd;
use mandate_research_run::{
    CallContext, CallFailure, CallPlan, ErrorKind, GatewayReply, ModelPin, ResearchCall,
    ResearchRunError, Retrieval, retrieve, settle,
};

fn pin() -> ModelPin {
    ModelPin {
        model_id: "llm.research_agent".to_owned(),
        model_version: "0.1.0".to_owned(),
        content_hash: Digest::of(b"research-entry"),
    }
}

fn retrieval() -> Retrieval {
    let mut fixture = Fixture::default();
    fixture.instruments = vec![instrument("AAPL", AAPL_ID)];
    fixture.bars = bars("AAPL", 2);
    fixture.news = vec![news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60)];
    retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"))
}

fn call_plan(observations: usize) -> CallPlan {
    CallPlan {
        run_id: "01RUN".to_owned(),
        call_id: "01CALL".to_owned(),
        workspace_id: "ws1".to_owned(),
        agent_id: "agent-a".to_owned(),
        model: pin(),
        as_of: at(CUT_OFF),
        deadline_ms: 120_000,
        max_output_tokens: 4_096,
        observations: (0..observations).map(|n| format!("01OBS{n}")).collect(),
    }
}

fn context() -> CallContext {
    CallContext {
        description: "Long-only US large caps on public news and filings.".to_owned(),
        data_universe: vec![symbol("AAPL"), symbol("MSFT")],
        asset_classes: vec![AssetClass::UsEquity],
        max_instruments: 5,
        working_universe: Vec::new(),
        held: Vec::new(),
        theses: Vec::new(),
    }
}

fn request() -> ResearchCall {
    let retrieved = retrieval();
    ResearchCall::build(
        &call_plan(retrieved.items.len()),
        &retrieved,
        context(),
        at(CUT_OFF + 5),
    )
    .unwrap_or_else(|e| panic!("{e}"))
}

fn reply(request: &ResearchCall) -> GatewayReply {
    GatewayReply {
        call_id: request.call_id.clone(),
        reported: pin(),
        output: br#"{"theses":[],"invalidations":[]}"#.to_vec(),
        request_digest: Digest::of(b"request"),
        prompt_ref: Digest::of(b"prompt"),
        response_ref: Digest::of(b"response"),
        cost_usd: Usd::parse("0.02").unwrap_or_else(|e| panic!("{e}")),
        completed_at: at(CUT_OFF + 30),
    }
}

#[test]
fn the_call_pairs_every_item_with_its_observation_in_order() {
    let retrieved = retrieval();
    let call = request();
    assert_eq!(call.inputs.items.len(), retrieved.items.len());
    for (n, (sent, item)) in call.inputs.items.iter().zip(&retrieved.items).enumerate() {
        assert_eq!(sent.observation, format!("01OBS{n}"));
        assert_eq!(
            sent.item_id,
            item.item_id().unwrap_or_else(|e| panic!("{e}"))
        );
        assert_eq!(sent.kind, item.kind());
        let record = item.record().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            mandate_canon::to_canonical(&sent.body),
            record,
            "the model reads exactly the journaled record"
        );
    }
    assert_eq!(call.call_id, "01CALL");
    assert_eq!(call.run_id, "01RUN");
    assert_eq!(call.model, pin());
    assert_eq!(call.as_of, at(CUT_OFF));
    assert_eq!(call.inputs.cut_off, at(CUT_OFF));
    assert_eq!(call.max_output_tokens, 4_096);
    assert_eq!(call.inputs.max_instruments, 5);
}

#[test]
fn the_deadline_counts_from_dispatch_not_from_the_run_start() {
    let call = request();
    assert_eq!(call.deadline, at(CUT_OFF + 5 + 120));
}

#[test]
fn an_item_without_an_observation_never_reaches_the_call() {
    let retrieved = retrieval();
    for observations in [retrieved.items.len() - 1, retrieved.items.len() + 1] {
        let built =
            ResearchCall::build(&call_plan(observations), &retrieved, context(), at(CUT_OFF));
        assert_eq!(
            built.map(|_| ()),
            Err(ResearchRunError::ObservationsMismatch {
                items: retrieved.items.len(),
                observations,
            })
        );
    }
}

#[test]
fn an_on_time_reply_from_the_pin_settles_unchanged() {
    let call = request();
    let answer = reply(&call);
    assert_eq!(settle(&call, Ok(answer.clone())), Ok(answer));
}

#[test]
fn a_reply_at_the_deadline_is_on_time_and_one_nanosecond_after_is_late() {
    let call = request();
    let mut answer = reply(&call);
    answer.completed_at = call.deadline;
    assert!(settle(&call, Ok(answer.clone())).is_ok());
    answer.completed_at = mandate_time::UtcNanos::from_parts(call.deadline.secs(), 1)
        .unwrap_or_else(|e| panic!("{e}"));
    let late = settle(&call, Ok(answer.clone()));
    assert_eq!(
        late,
        Err(CallFailure {
            call_id: call.call_id.clone(),
            kind: ErrorKind::DeadlineExceeded,
            completed_at: answer.completed_at,
        })
    );
}

#[test]
fn a_reply_from_another_identity_is_an_identity_mismatch() {
    let call = request();
    for change in 0..3 {
        let mut answer = reply(&call);
        match change {
            0 => answer.reported.model_id = "llm.other".to_owned(),
            1 => answer.reported.model_version = "0.2.0".to_owned(),
            _ => answer.reported.content_hash = Digest::of(b"other"),
        }
        assert_eq!(
            settle(&call, Ok(answer)).map_err(|f| f.kind),
            Err(ErrorKind::IdentityMismatch),
            "change {change}"
        );
    }
}

#[test]
fn a_reply_for_another_call_is_never_used() {
    let call = request();
    let mut answer = reply(&call);
    answer.call_id = "01OTHER".to_owned();
    let settled = settle(&call, Ok(answer));
    assert_eq!(
        settled.as_ref().map_err(|f| f.kind),
        Err(ErrorKind::IdentityMismatch)
    );
    assert_eq!(settled.map_err(|f| f.call_id), Err("01CALL".to_owned()));
}

#[test]
fn a_failure_passes_through_under_the_request_s_call_id() {
    let call = request();
    let failure = CallFailure {
        call_id: "whatever".to_owned(),
        kind: ErrorKind::BudgetExhausted,
        completed_at: at(CUT_OFF + 1),
    };
    assert_eq!(
        settle(&call, Err(failure)),
        Err(CallFailure {
            call_id: "01CALL".to_owned(),
            kind: ErrorKind::BudgetExhausted,
            completed_at: at(CUT_OFF + 1),
        })
    );
}

#[test]
fn every_kind_has_its_treatment_and_none_is_retried() {
    let table = [
        (ErrorKind::PolicyDenied, true, false, false),
        (ErrorKind::CredentialInvalid, true, false, false),
        (ErrorKind::BudgetExhausted, false, false, false),
        (ErrorKind::RateLimitedLocal, false, false, false),
        (ErrorKind::MeterUnavailable, true, false, false),
        (ErrorKind::InputRejected, true, false, false),
        (ErrorKind::ModelWithdrawn, false, true, false),
        (ErrorKind::DeadlineExceeded, false, false, true),
        (ErrorKind::ProviderUnavailable, false, false, false),
        (ErrorKind::RateLimitedProvider, false, false, false),
        (ErrorKind::ContentRefused, false, false, false),
        (ErrorKind::SchemaInvalid, false, false, false),
        (ErrorKind::IdentityMismatch, false, false, false),
    ];
    for (kind, alert, disable, streak) in table {
        let treatment = kind.treatment();
        assert_eq!(treatment.alert_operator, alert, "{}", kind.as_str());
        assert_eq!(treatment.disable_research, disable, "{}", kind.as_str());
        assert_eq!(treatment.deadline_streak, streak, "{}", kind.as_str());
    }
}

#[test]
fn every_kind_writes_inference_spec_s_name() {
    let names: Vec<&str> = [
        ErrorKind::PolicyDenied,
        ErrorKind::BudgetExhausted,
        ErrorKind::RateLimitedLocal,
        ErrorKind::MeterUnavailable,
        ErrorKind::InputRejected,
        ErrorKind::ModelWithdrawn,
        ErrorKind::DeadlineExceeded,
        ErrorKind::ProviderUnavailable,
        ErrorKind::RateLimitedProvider,
        ErrorKind::CredentialInvalid,
        ErrorKind::ContentRefused,
        ErrorKind::SchemaInvalid,
        ErrorKind::IdentityMismatch,
    ]
    .iter()
    .map(|kind| kind.as_str())
    .collect();
    assert_eq!(
        names,
        vec![
            "policy_denied",
            "budget_exhausted",
            "rate_limited_local",
            "meter_unavailable",
            "input_rejected",
            "model_withdrawn",
            "deadline_exceeded",
            "provider_unavailable",
            "rate_limited_provider",
            "credential_invalid",
            "content_refused",
            "schema_invalid",
            "identity_mismatch",
        ]
    );
}
