//! The gateway's invariants over random sequences of calls, provider replies, timeouts, retries,
//! crashes, restarts, and clock jumps across risk days and months (inference spec §2.1):
//!
//! - **Caps** (INF-6, INF-7, DEC-997 item 3): no sequence lets spend exceed any cap. The oracle is a
//!   running accumulator over the meter records in append order, its own code, not the meter's fold.
//! - **Deadline** (INF-3): no attempt starts at or after its deadline, no output completes after it,
//!   a call's deadline is never later than its entry's, and a response completed after it is never
//!   cached. The oracle reads the records' own `deadline` and `completed_at`, never a gateway flag.
//! - **Identity** (INF-1, INF-2): a reply whose reported model or provider differs from the pin, by
//!   plain string equality on each part, is always refused `identity_mismatch` with no output.
//!
//! Each oracle is shown to catch a seeded bug before it is trusted (the `seeded_` tests).

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::*;
use mandate_canon::Digest;
use mandate_gateway::call::{prompt_bytes, render};
use mandate_gateway::fake::{Recorded, RecordedTransport, Sent};
use mandate_gateway::kinds::{KeyOwner, Outcome, Purpose};
use mandate_gateway::meter::{
    Caps, MemoryMeter, Meter, MeterRecord, Period, Reservation, Scope, Settlement,
};
use mandate_gateway::money::{Usage, Usd6};
use mandate_gateway::openrouter::OpenRouter;
use mandate_gateway::provider::{Clock, Delivery, ManualClock, SequentialIds};
use mandate_gateway::registry::ModelRef;
use mandate_gateway::{CallRecord, Gateway};
use mandate_time::{Date, UtcNanos, new_york_date_and_hour};
use proptest::prelude::*;

const AGENTS: [Option<&str>; 3] = [Some("agent_a"), Some("agent_b"), None];

#[derive(Debug, Clone)]
enum Reply {
    Ok {
        input_pct: u64,
        output: u64,
    },
    Late,
    ServerError {
        recovers: bool,
        billed: bool,
    },
    ConnectFailed,
    RateLimited {
        retry_ms: u64,
    },
    TimedOut,
    Reported {
        model: Option<String>,
        provider: Option<String>,
    },
    Misshaped {
        model: Option<String>,
        provider: Option<String>,
        without_choices: bool,
    },
    BadSchema,
    NotProviderShape,
    Refused,
    Unauthorized,
}

#[derive(Debug, Clone)]
enum Step {
    Call {
        agent: usize,
        research: bool,
        input: u8,
        reply: Reply,
        latency_ms: u64,
    },
    Crash {
        agent: usize,
        max_micros: u64,
    },
    Restart,
    Advance {
        ms: u64,
    },
}

fn reported_strategy() -> impl Strategy<Value = Option<String>> {
    prop_oneof![
        Just(None),
        Just(Some(SLUG.to_owned())),
        Just(Some(REPORTED_PROVIDER.to_owned())),
        Just(Some("anthropic/claude-haiku-4.5".to_owned())),
        Just(Some("Amazon Bedrock".to_owned())),
        Just(Some(format!("{SLUG} "))),
        "[a-zA-Z/ .-]{0,12}".prop_map(Some),
    ]
}

fn reply_strategy() -> impl Strategy<Value = Reply> {
    prop_oneof![
        4 => (0u64..=100, 0u64..=1_000).prop_map(|(input_pct, output)| Reply::Ok { input_pct, output }),
        1 => Just(Reply::Late),
        1 => (any::<bool>(), any::<bool>()).prop_map(|(recovers, billed)| Reply::ServerError { recovers, billed }),
        1 => Just(Reply::ConnectFailed),
        1 => (0u64..90_000).prop_map(|retry_ms| Reply::RateLimited { retry_ms }),
        1 => Just(Reply::TimedOut),
        2 => (reported_strategy(), reported_strategy()).prop_map(|(model, provider)| Reply::Reported { model, provider }),
        1 => (reported_strategy(), reported_strategy(), any::<bool>()).prop_map(|(model, provider, without_choices)| Reply::Misshaped { model, provider, without_choices }),
        1 => Just(Reply::BadSchema),
        1 => Just(Reply::NotProviderShape),
        1 => Just(Reply::Refused),
        1 => Just(Reply::Unauthorized),
    ]
}

fn step_strategy() -> impl Strategy<Value = Step> {
    prop_oneof![
        8 => (0usize..3, any::<bool>(), 0u8..6, reply_strategy(), 0u64..70_000)
            .prop_map(|(agent, research, input, reply, latency_ms)| Step::Call { agent, research, input, reply, latency_ms }),
        1 => (0usize..3, 0u64..40_000).prop_map(|(agent, max_micros)| Step::Crash { agent, max_micros }),
        1 => Just(Step::Restart),
        2 => prop_oneof![0u64..120_000, 0u64..(40 * 86_400_000)].prop_map(|ms| Step::Advance { ms }),
    ]
}

/// The caps one scenario runs under, from cents to a few dimes so that both admissions and
/// refusals happen.
#[derive(Debug, Clone)]
struct CapModel {
    workspace_monthly: u64,
    agent_a_monthly: u64,
    research_daily_a: u64,
}

fn caps_strategy() -> impl Strategy<Value = CapModel> {
    (5_000u64..200_000, 0u64..=100, 1_000u64..80_000).prop_map(|(ws, pct, daily)| CapModel {
        workspace_monthly: ws,
        agent_a_monthly: ws * pct / 100,
        research_daily_a: daily.min(ws),
    })
}

impl CapModel {
    fn caps(&self) -> Caps {
        Caps::new(Usd6::from_micros(self.workspace_monthly))
            .with_agent_monthly("agent_a", Usd6::from_micros(self.agent_a_monthly))
            .unwrap()
            .with_research_daily("agent_a", Usd6::from_micros(self.research_daily_a))
            .unwrap()
    }
}

fn inputs_for(n: u8) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "bars".to_owned(),
            format!("bars set {n} {}", "x".repeat(usize::from(n) * 40)),
        ),
        ("filings".to_owned(), format!("filings set {n}")),
    ])
}

fn body(reply: &Reply, prompt_len: u64) -> Option<(u16, Vec<u8>, Option<u64>)> {
    let ok = |input: u64, output: u64| {
        reply_body(
            GOOD_OUTPUT,
            Some(SLUG),
            Some(REPORTED_PROVIDER),
            input,
            output,
        )
    };
    Some(match reply {
        Reply::Ok { input_pct, output } => (200, ok(prompt_len * input_pct / 100, *output), None),
        Reply::Late => (200, ok(prompt_len / 2, 300), None),
        Reply::ServerError { .. } => (503, vec![], None),
        Reply::RateLimited { retry_ms } => (429, vec![], Some(*retry_ms)),
        Reply::Reported { model, provider } => (
            200,
            reply_body(
                GOOD_OUTPUT,
                model.as_deref(),
                provider.as_deref(),
                prompt_len / 2,
                200,
            ),
            None,
        ),
        Reply::Misshaped {
            model,
            provider,
            without_choices,
        } => {
            let mut v: serde_json::Value = serde_json::from_slice(&reply_body(
                GOOD_OUTPUT,
                model.as_deref(),
                provider.as_deref(),
                prompt_len / 2,
                200,
            ))
            .unwrap();
            let object = v.as_object_mut().unwrap();
            object.remove("usage");
            if *without_choices {
                object.remove("choices");
            }
            (200, serde_json::to_vec(&v).unwrap(), None)
        }
        Reply::BadSchema => (
            200,
            reply_body(
                r#"{"theses":"none"}"#,
                Some(SLUG),
                Some(REPORTED_PROVIDER),
                prompt_len / 3,
                50,
            ),
            None,
        ),
        Reply::NotProviderShape => (200, b"[1,2]".to_vec(), None),
        Reply::Refused => {
            let mut v: serde_json::Value = serde_json::from_slice(&ok(prompt_len / 4, 10)).unwrap();
            v["choices"][0]["finish_reason"] = serde_json::json!("content_filter");
            (200, serde_json::to_vec(&v).unwrap(), None)
        }
        Reply::Unauthorized => (401, vec![], None),
        Reply::ConnectFailed | Reply::TimedOut => return None,
    })
}

fn script(reply: &Reply, latency_ms: u64, prompt_len: u64) -> Vec<Recorded> {
    let latency = latency_ms.min(59_000);
    match reply {
        Reply::Late => vec![delivered(
            200,
            body(reply, prompt_len).unwrap().1,
            60_001 + latency_ms,
        )],
        Reply::ConnectFailed => vec![
            Recorded {
                latency_ms: latency,
                delivery: Delivery::ConnectFailed
            };
            3
        ],
        Reply::TimedOut => vec![Recorded {
            latency_ms: 60_000,
            delivery: Delivery::TimedOut,
        }],
        Reply::ServerError { recovers, billed } => {
            let failed = if *billed {
                reply_body(
                    GOOD_OUTPUT,
                    Some(SLUG),
                    Some(REPORTED_PROVIDER),
                    prompt_len,
                    u64::from(content().max_output_tokens),
                )
            } else {
                vec![]
            };
            let mut out = vec![delivered(503, failed.clone(), latency / 3); 2];
            out.push(if *recovers {
                ok_reply(latency / 3)
            } else {
                delivered(503, failed, latency / 3)
            });
            out
        }
        Reply::RateLimited { retry_ms } => vec![
            Recorded {
                latency_ms: latency / 2,
                delivery: Delivery::Response {
                    status: 429,
                    body: vec![],
                    retry_after_ms: Some(*retry_ms),
                },
            },
            ok_reply(latency / 2),
        ],
        other => {
            let (status, b, _) = body(other, prompt_len).unwrap();
            vec![delivered(status, b, latency)]
        }
    }
}

/// One call as the oracles see it: when it started, what reply the provider was scripted to
/// report, and the record.
struct Observed {
    start: UtcNanos,
    reported: Option<(Option<String>, Option<String>)>,
    late: bool,
    record: CallRecord,
    cached_after: bool,
}

struct Run {
    calls: Vec<Observed>,
    sent: Vec<Sent>,
    meter: Vec<MeterRecord>,
    final_spend: Vec<((Scope, Period), Usd6)>,
}

fn new_gateway(store: MemoryMeter, caps: Caps, clock: ManualClock, ids_from: u128) -> TestGateway {
    let (registry, _) = registry();
    Gateway::new(
        settings(hosted_policy()),
        registry,
        Box::new(OpenRouter),
        RecordedTransport::new(),
        Box::new(TestKey),
        Meter::new(store, caps, common::deployment()),
        clock,
        Box::new(SequentialIds::starting_at(ids_from)),
    )
}

fn risk_day(at: UtcNanos) -> Date {
    new_york_date_and_hour(at).unwrap().0
}

fn run(caps: &CapModel, steps: &[Step]) -> Run {
    let model: ModelRef = content().model_ref();
    let mut ids_from: u128 = 1 << 80;
    let mut gateway = new_gateway(
        MemoryMeter::new(),
        caps.caps(),
        ManualClock::at(at(START)),
        ids_from,
    );
    let mut calls = Vec::new();
    let mut sent = Vec::new();
    let mut crash_ids: u128 = 1 << 100;
    for (n, step) in steps.iter().enumerate() {
        match step {
            Step::Call {
                agent,
                research,
                input,
                reply,
                latency_ms,
            } => {
                let agent = AGENTS[*agent];
                let purpose = match (agent, research) {
                    (None, _) => Purpose::Compiler,
                    (Some(_), true) => Purpose::Research,
                    (Some(_), false) => Purpose::LlmSignal,
                };
                let inputs = inputs_for(*input);
                let prompt = render(&content().template, &inputs).unwrap();
                let prompt_len = u64::try_from(prompt_bytes(&prompt).len()).unwrap();
                let start = gateway.clock_mut().now();
                let request = mandate_gateway::CallRequest {
                    call_id: mandate_canon::encode_ulid((1 << 90) + u128::try_from(n).unwrap()),
                    workspace_id: WORKSPACE.to_owned(),
                    agent_id: agent.map(str::to_owned),
                    purpose,
                    model: model.clone(),
                    inputs,
                    as_of: at("2026-10-16T13:55:00.000000000Z"),
                    deadline: mandate_gateway::provider::after_ms(start, 60_000).unwrap(),
                    observations: vec![],
                    key_owner: KeyOwner::Platform,
                    run_kind: None,
                    turns: vec![],
                    tools: vec![],
                    spend_ceiling: None,
                };
                gateway.transport_mut().clear_script();
                gateway
                    .transport_mut()
                    .script(script(reply, *latency_ms, prompt_len));
                let record = gateway.call(&request).unwrap();
                let cached_after = gateway.cached(model.content_hash, record.request_digest);
                if record.outcome == Outcome::IdentityMismatch {
                    gateway
                        .set_endpoint_healthy(&model, "openrouter:anthropic", true)
                        .unwrap();
                }
                let reported = match reply {
                    Reply::Reported { model, provider }
                    | Reply::Misshaped {
                        model, provider, ..
                    } => Some((model.clone(), provider.clone())),
                    Reply::NotProviderShape => Some((None, None)),
                    _ => None,
                };
                calls.push(Observed {
                    start,
                    reported,
                    late: matches!(reply, Reply::Late),
                    record,
                    cached_after,
                });
            }
            Step::Crash { agent, max_micros } => {
                let now = gateway.clock_mut().now();
                crash_ids += 1;
                let agent = AGENTS[*agent];
                let _ = gateway.meter_mut().reserve(Reservation {
                    event_id: mandate_canon::encode_ulid(crash_ids),
                    call_id: mandate_canon::encode_ulid(crash_ids + (1 << 70)),
                    agent_id: agent.map(str::to_owned),
                    purpose: if agent.is_some() {
                        Purpose::Research
                    } else {
                        Purpose::Compiler
                    },
                    model: model.clone(),
                    endpoint: "openrouter:anthropic".to_owned(),
                    key_owner: KeyOwner::Platform,
                    price_table_ref: prices().version(),
                    max_cost: Usd6::from_micros(*max_micros),
                    risk_day: risk_day(now),
                });
            }
            Step::Restart => {
                sent.extend(gateway.transport().sent().iter().cloned());
                let store = gateway.meter().store().clone();
                let clock = *gateway.clock_mut();
                ids_from += 1 << 40;
                gateway = new_gateway(store, caps.caps(), clock, ids_from);
            }
            Step::Advance { ms } => gateway.clock_mut().advance_ms(*ms),
        }
    }
    sent.extend(gateway.transport().sent().iter().cloned());
    let meter = gateway.meter().store().records().to_vec();
    let mut final_spend = Vec::new();
    for key in spend_keys(&meter) {
        let value = gateway.meter().spend(&key.0, key.1).unwrap();
        final_spend.push((key, value));
    }
    Run {
        calls,
        sent,
        meter,
        final_spend,
    }
}

fn month(day: Date) -> Period {
    Period::Month {
        year: day.year(),
        month: day.month(),
    }
}

/// Every (scope, period) a cap reads, for the records' reservations.
fn spend_keys(records: &[MeterRecord]) -> BTreeSet<(Scope, Period)> {
    let mut keys = BTreeSet::new();
    for r in records {
        if let MeterRecord::Reserved(x) = r {
            keys.insert((Scope::Workspace, month(x.risk_day)));
            if let Some(agent) = &x.agent_id {
                keys.insert((Scope::Agent(agent.clone()), month(x.risk_day)));
                keys.insert((Scope::Agent(agent.clone()), Period::Day(x.risk_day)));
            }
        }
    }
    keys
}

/// The cap oracle: walks the meter records in append order, adding each reservation's maximum to
/// every total it counts toward and, on its settlement, taking back what the call did not spend.
/// After each reservation, every cap that applied to it must hold. At the end, its totals must
/// equal the meter's fold.
fn cap_oracle(caps: &CapModel, run: &Run) -> Result<(), String> {
    let mut totals: BTreeMap<(String, String), i128> = BTreeMap::new();
    let mut open: BTreeMap<String, (i128, Vec<(String, String)>)> = BTreeMap::new();
    for record in &run.meter {
        match record {
            MeterRecord::Reserved(r) => {
                let ym = format!("{}-{:02}", r.risk_day.year(), r.risk_day.month());
                let mut keys = vec![("workspace".to_owned(), ym.clone())];
                let mut checked =
                    vec![(("workspace".to_owned(), ym.clone()), caps.workspace_monthly)];
                if let Some(agent) = &r.agent_id {
                    keys.push((agent.clone(), ym.clone()));
                    keys.push((agent.clone(), r.risk_day.to_string()));
                    let monthly = if agent == "agent_a" {
                        caps.agent_a_monthly
                    } else {
                        caps.workspace_monthly
                    };
                    checked.push(((agent.clone(), ym.clone()), monthly));
                    if agent == "agent_a" && r.purpose == Purpose::Research {
                        checked.push((
                            (agent.clone(), r.risk_day.to_string()),
                            caps.research_daily_a,
                        ));
                    }
                }
                let max = i128::from(r.max_cost.micros());
                for key in &keys {
                    *totals.entry(key.clone()).or_default() += max;
                }
                for (key, cap) in checked {
                    let total = totals[&key];
                    if total > i128::from(cap) {
                        return Err(format!(
                            "{key:?} reached {total} over its cap {cap} at {}",
                            r.event_id
                        ));
                    }
                }
                open.insert(r.event_id.clone(), (max, keys));
            }
            MeterRecord::Settled(s) => {
                let (max, keys) = open
                    .remove(&s.reservation)
                    .ok_or_else(|| format!("{} settles nothing open", s.event_id))?;
                let cost = i128::from(s.cost.micros());
                if cost > max {
                    return Err(format!(
                        "{} cost {cost} above its reservation {max}",
                        s.event_id
                    ));
                }
                for key in keys {
                    *totals.entry(key).or_default() -= max - cost;
                }
            }
        }
    }
    for ((scope, period), value) in &run.final_spend {
        let who = match scope {
            Scope::Workspace => "workspace".to_owned(),
            Scope::Agent(a) => a.clone(),
            Scope::Deployment => "deployment".to_owned(),
        };
        let when = match period {
            Period::Day(d) => d.to_string(),
            Period::Month { year, month } => format!("{year}-{month:02}"),
        };
        let mine = totals
            .get(&(who.clone(), when.clone()))
            .copied()
            .unwrap_or(0);
        if mine != i128::from(value.micros()) {
            return Err(format!(
                "the meter folds {value} for {who} {when}; the accumulator {mine}"
            ));
        }
    }
    Ok(())
}

/// The deadline oracle, from the records' journaled `deadline` and `completed_at` and the
/// transport's log.
fn deadline_oracle(run: &Run) -> Result<(), String> {
    for sent in &run.sent {
        if sent.at >= sent.deadline {
            return Err(format!(
                "an attempt started at {} on or after its deadline {}",
                sent.at, sent.deadline
            ));
        }
    }
    let mut late_digests = BTreeSet::new();
    for call in &run.calls {
        let r = &call.record;
        let latest =
            mandate_gateway::provider::after_ms(call.start, content().deadline_ms).unwrap();
        if r.deadline > latest {
            return Err(format!(
                "{}: deadline {} past the entry's {latest}",
                r.call_id, r.deadline
            ));
        }
        if call.late && !r.cache_hit && r.outcome == Outcome::Ok {
            return Err(format!(
                "{}: an ok from a reply scripted past the deadline",
                r.call_id
            ));
        }
        let late = r.completed_at > r.deadline;
        if late && r.outcome == Outcome::Ok {
            return Err(format!(
                "{}: an ok completed at {} after {}",
                r.call_id, r.completed_at, r.deadline
            ));
        }
        if r.output.is_some() != (r.outcome == Outcome::Ok) {
            return Err(format!("{}: output present for {:?}", r.call_id, r.outcome));
        }
        if late {
            if call.cached_after {
                return Err(format!("{}: a late response is in the cache", r.call_id));
            }
            late_digests.insert(r.request_digest);
        } else if r.cache_hit && late_digests.contains(&r.request_digest) {
            return Err(format!(
                "{}: a hit on a request whose only response was late",
                r.call_id
            ));
        }
        if r.outcome == Outcome::Ok && !r.cache_hit {
            late_digests.remove(&r.request_digest);
        }
    }
    Ok(())
}

/// The identity oracle: plain string equality on each reported part against the pin.
fn identity_oracle(run: &Run) -> Result<(), String> {
    for call in &run.calls {
        let Some((model, provider)) = &call.reported else {
            continue;
        };
        let r = &call.record;
        if r.cache_hit {
            continue;
        }
        let matches =
            model.as_deref() == Some(SLUG) && provider.as_deref() == Some(REPORTED_PROVIDER);
        let read_as_content = matches!(
            r.outcome,
            Outcome::Ok | Outcome::SchemaInvalid | Outcome::ContentRefused
        );
        if !matches && (read_as_content || r.output.is_some()) {
            return Err(format!(
                "{}: reported {model:?} {provider:?} gave {:?}",
                r.call_id, r.outcome
            ));
        }
        if matches && r.outcome == Outcome::IdentityMismatch {
            return Err(format!("{}: the pin's own identity was refused", r.call_id));
        }
    }
    Ok(())
}

/// The journal-rule oracle: every record the gateway returns keeps journal spec §9.19's rules 72 to
/// 75 and 148, 149, and 152 on its own fields, read here by plain comparisons rather than by the
/// gateway's payload check, and agrees with the meter record of its call.
fn record_oracle(run: &Run) -> Result<(), String> {
    let unsent_kinds = [Outcome::DeadlineExceeded, Outcome::CredentialInvalid];
    for call in &run.calls {
        let r = &call.record;
        let id = &r.call_id;
        let rejected = r.outcome == Outcome::InputRejected;
        if r.prompt_ref.is_none() != rejected
            || r.request_digest.is_none() != rejected
            || r.guard_rule.is_some() != rejected
        {
            return Err(format!("{id}: rule 148 for {:?}", r.outcome));
        }
        let refusal = r.outcome.is_refusal();
        if r.reservation.is_none() != (refusal || r.cache_hit) {
            return Err(format!(
                "{id}: rule 149 for {:?}, hit {}",
                r.outcome, r.cache_hit
            ));
        }
        let nothing_sent = r.response_ref.is_none()
            && r.reported_identity.is_none()
            && r.provider_request_id.is_none()
            && r.tokens == Usage::default()
            && r.cost == Usd6::ZERO;
        if refusal && !(r.attempts == 0 && nothing_sent) {
            return Err(format!("{id}: rule 73 for {:?}", r.outcome));
        }
        if r.cache_hit
            && !(r.outcome == Outcome::Ok
                && r.attempts == 0
                && r.cost == Usd6::ZERO
                && r.provider_request_id.is_none())
        {
            return Err(format!("{id}: rule 74 for a hit"));
        }
        if !refusal
            && !r.cache_hit
            && r.attempts == 0
            && !(unsent_kinds.contains(&r.outcome) && nothing_sent)
        {
            return Err(format!(
                "{id}: rule 152 for {:?} with no attempt",
                r.outcome
            ));
        }
        if r.outcome == Outcome::Ok && (r.response_ref.is_none() || r.reported_identity.is_none()) {
            return Err(format!("{id}: rule 72"));
        }
        if let Some(reservation) = &r.reservation {
            let reserved = run.meter.iter().any(|m| matches!(m, MeterRecord::Reserved(x) if &x.event_id == reservation && &x.call_id == id));
            if !reserved {
                return Err(format!("{id}: its reservation is not on the meter stream"));
            }
            let settled = run.meter.iter().find_map(|m| match m {
                MeterRecord::Settled(s) if &s.reservation == reservation => Some(s),
                _ => None,
            });
            if let Some(s) = settled
                && (s.cost != r.cost
                    || s.attempts != r.attempts
                    || s.outcome != r.outcome
                    || s.tokens != r.tokens)
            {
                return Err(format!("{id}: its settlement disagrees with its record"));
            }
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn no_sequence_exceeds_a_cap_and_the_meter_folds_what_the_accumulator_counts(
        caps in caps_strategy(), steps in prop::collection::vec(step_strategy(), 1..60),
    ) {
        let run = run(&caps, &steps);
        prop_assert_eq!(cap_oracle(&caps, &run), Ok(()));
    }

    #[test]
    fn no_sequence_uses_or_caches_anything_past_its_deadline(
        caps in caps_strategy(), steps in prop::collection::vec(step_strategy(), 1..60),
    ) {
        let run = run(&caps, &steps);
        prop_assert_eq!(deadline_oracle(&run), Ok(()));
    }

    #[test]
    fn a_mismatched_reported_model_or_provider_is_always_refused(
        steps in prop::collection::vec(step_strategy(), 1..40),
    ) {
        let run = run(&CapModel { workspace_monthly: 50_000_000, agent_a_monthly: 50_000_000, research_daily_a: 50_000_000 }, &steps);
        prop_assert_eq!(identity_oracle(&run), Ok(()));
    }

    #[test]
    fn every_record_keeps_the_journal_rules_and_agrees_with_its_meter_record(
        caps in caps_strategy(), steps in prop::collection::vec(step_strategy(), 1..60),
    ) {
        let run = run(&caps, &steps);
        prop_assert_eq!(record_oracle(&run), Ok(()));
        for call in &run.calls {
            if call.record.purpose != Purpose::Compiler {
                prop_assert!(call.record.agent_payload(&[]).is_ok(), "{:?}", call.record.outcome);
            }
        }
    }
}

fn reserved(n: u128, agent: Option<&str>, purpose: Purpose, max: u64, day: Date) -> MeterRecord {
    MeterRecord::Reserved(Reservation {
        event_id: mandate_canon::encode_ulid(n),
        call_id: mandate_canon::encode_ulid(n + 1_000),
        agent_id: agent.map(str::to_owned),
        purpose,
        model: content().model_ref(),
        endpoint: "e".to_owned(),
        key_owner: KeyOwner::Platform,
        price_table_ref: Digest::ZERO,
        max_cost: Usd6::from_micros(max),
        risk_day: day,
    })
}

fn settled(n: u128, cost: u64) -> MeterRecord {
    MeterRecord::Settled(Settlement {
        event_id: mandate_canon::encode_ulid(n + 2_000),
        call_id: mandate_canon::encode_ulid(n + 1_000),
        reservation: mandate_canon::encode_ulid(n),
        outcome: Outcome::Ok,
        attempts: 1,
        tokens: Usage::default(),
        cost: Usd6::from_micros(cost),
    })
}

fn empty_run(meter: Vec<MeterRecord>) -> Run {
    Run {
        calls: vec![],
        sent: vec![],
        meter,
        final_spend: vec![],
    }
}

#[test]
fn seeded_the_cap_oracle_catches_a_reservation_past_a_cap() {
    let caps = CapModel {
        workspace_monthly: 100,
        agent_a_monthly: 50,
        research_daily_a: 30,
    };
    let d = Date::new(2026, 10, 16).unwrap();
    assert!(
        cap_oracle(
            &caps,
            &empty_run(vec![reserved(1, None, Purpose::Compiler, 100, d)])
        )
        .is_ok()
    );
    assert!(
        cap_oracle(
            &caps,
            &empty_run(vec![reserved(1, None, Purpose::Compiler, 101, d)])
        )
        .is_err(),
        "workspace"
    );
    assert!(
        cap_oracle(
            &caps,
            &empty_run(vec![reserved(
                1,
                Some("agent_a"),
                Purpose::LlmSignal,
                51,
                d
            )])
        )
        .is_err(),
        "agent"
    );
    assert!(
        cap_oracle(
            &caps,
            &empty_run(vec![reserved(1, Some("agent_a"), Purpose::Research, 31, d)])
        )
        .is_err(),
        "daily"
    );
    let settled_first = vec![
        reserved(1, None, Purpose::Compiler, 90, d),
        settled(1, 10),
        reserved(2, None, Purpose::Compiler, 90, d),
    ];
    assert!(
        cap_oracle(&caps, &empty_run(settled_first)).is_ok(),
        "a settlement releases its unspent part"
    );
    let open = vec![
        reserved(1, None, Purpose::Compiler, 90, d),
        reserved(2, None, Purpose::Compiler, 11, d),
    ];
    assert!(
        cap_oracle(&caps, &empty_run(open)).is_err(),
        "an open reservation counts whole"
    );
    let over = vec![reserved(1, None, Purpose::Compiler, 10, d), settled(1, 11)];
    assert!(
        cap_oracle(&caps, &empty_run(over)).is_err(),
        "a settlement above its reservation"
    );
    let mut fold_wrong = empty_run(vec![reserved(1, None, Purpose::Compiler, 10, d)]);
    fold_wrong.final_spend = vec![(
        (
            Scope::Workspace,
            Period::Month {
                year: 2026,
                month: 10,
            },
        ),
        Usd6::from_micros(9),
    )];
    assert!(
        cap_oracle(&caps, &fold_wrong).is_err(),
        "a fold that under-counts an open reservation"
    );
}

fn observed(start: &str, record: CallRecord, cached_after: bool) -> Observed {
    Observed {
        start: at(start),
        reported: None,
        late: false,
        record,
        cached_after,
    }
}

fn base_record() -> CallRecord {
    let (mut gateway, model) = gateway();
    gateway.transport_mut().script(vec![ok_reply(10)]);
    gateway.call(&request(&model, 1)).unwrap()
}

#[test]
fn seeded_the_deadline_oracle_catches_late_use_and_late_caching() {
    let good = base_record();
    let run_of = |calls: Vec<Observed>, sent: Vec<Sent>| Run {
        calls,
        sent,
        meter: vec![],
        final_spend: vec![],
    };
    assert!(deadline_oracle(&run_of(vec![observed(START, good.clone(), true)], vec![])).is_ok());
    let mut late_ok = good.clone();
    late_ok.completed_at = at("2026-10-16T14:01:00.000000001Z");
    assert!(
        deadline_oracle(&run_of(vec![observed(START, late_ok, false)], vec![])).is_err(),
        "a late ok"
    );
    let mut late = good.clone();
    late.outcome = Outcome::DeadlineExceeded;
    late.output = None;
    late.completed_at = at("2026-10-16T14:01:30.000000000Z");
    assert!(
        deadline_oracle(&run_of(vec![observed(START, late.clone(), true)], vec![])).is_err(),
        "a late response cached"
    );
    let mut hit = good.clone();
    hit.cache_hit = true;
    let seq = vec![
        observed(START, late.clone(), false),
        observed(START, hit, true),
    ];
    assert!(
        deadline_oracle(&run_of(seq, vec![])).is_err(),
        "a later hit on a late response"
    );
    let mut stale_stamp = observed(START, good.clone(), false);
    stale_stamp.late = true;
    assert!(
        deadline_oracle(&run_of(vec![stale_stamp], vec![])).is_err(),
        "an ok from a late reply stamped with a stale clock reading"
    );
    let mut far = good.clone();
    far.deadline = at("2026-10-16T14:01:00.000000001Z");
    assert!(
        deadline_oracle(&run_of(vec![observed(START, far, true)], vec![])).is_err(),
        "a deadline past the entry's"
    );
    let mut no_output = good.clone();
    no_output.output = None;
    assert!(
        deadline_oracle(&run_of(vec![observed(START, no_output, true)], vec![])).is_err(),
        "an ok without output"
    );
    let attempt = Sent {
        host: HOST.to_owned(),
        body_digest: Digest::ZERO,
        body: vec![],
        at: at("2026-10-16T14:01:00.000000000Z"),
        deadline: at("2026-10-16T14:01:00.000000000Z"),
        with_key: true,
    };
    assert!(
        deadline_oracle(&run_of(vec![], vec![attempt])).is_err(),
        "an attempt at its deadline"
    );
}

#[test]
fn seeded_the_identity_oracle_catches_a_substituted_model() {
    let good = base_record();
    let other = Some((
        Some("anthropic/claude-haiku-4.5".to_owned()),
        Some(REPORTED_PROVIDER.to_owned()),
    ));
    let pinned = Some((Some(SLUG.to_owned()), Some(REPORTED_PROVIDER.to_owned())));
    let one = |reported, record| Run {
        calls: vec![Observed {
            start: at(START),
            reported,
            late: false,
            record,
            cached_after: false,
        }],
        sent: vec![],
        meter: vec![],
        final_spend: vec![],
    };
    assert!(identity_oracle(&one(pinned.clone(), good.clone())).is_ok());
    assert!(
        identity_oracle(&one(other.clone(), good.clone())).is_err(),
        "another model's output accepted"
    );
    let mut refused = good.clone();
    refused.outcome = Outcome::IdentityMismatch;
    refused.output = None;
    assert!(identity_oracle(&one(other.clone(), refused.clone())).is_ok());
    assert!(
        identity_oracle(&one(pinned, refused.clone())).is_err(),
        "the pin's own identity refused"
    );
    let mut leaked = refused.clone();
    leaked.output = good.output.clone();
    assert!(
        identity_oracle(&one(other.clone(), leaked)).is_err(),
        "a refused call that kept its output"
    );
    for outcome in [Outcome::SchemaInvalid, Outcome::ContentRefused] {
        let mut misread = refused.clone();
        misread.outcome = outcome;
        assert!(
            identity_oracle(&one(other.clone(), misread.clone())).is_err(),
            "a substituted model's misshaped reply read as {outcome:?}"
        );
        assert!(
            identity_oracle(&one(Some((None, None)), misread)).is_err(),
            "a reply with no identity read as {outcome:?}"
        );
    }
}

#[test]
fn seeded_the_record_oracle_catches_a_record_the_journal_would_refuse() {
    let (mut gateway, model) = gateway();
    gateway.transport_mut().script(vec![ok_reply(10)]);
    let good = gateway.call(&request(&model, 1)).unwrap();
    let meter = gateway.meter().store().records().to_vec();
    let one = |record: CallRecord| Run {
        calls: vec![Observed {
            start: at(START),
            reported: None,
            late: false,
            record,
            cached_after: false,
        }],
        sent: vec![],
        meter: meter.clone(),
        final_spend: vec![],
    };
    assert!(record_oracle(&one(good.clone())).is_ok(), "the base call");
    let mut cheaper = good.clone();
    cheaper.cost = Usd6::ZERO;
    assert!(
        record_oracle(&one(cheaper)).is_err(),
        "a record that disagrees with its settlement"
    );
    let mut fine = good.clone();
    fine.reservation = None;
    fine.cache_hit = true;
    fine.attempts = 0;
    fine.cost = Usd6::ZERO;
    fine.provider_request_id = None;
    assert!(record_oracle(&one(fine)).is_ok(), "a hit");
    let mut no_prompt = good.clone();
    no_prompt.prompt_ref = None;
    assert!(
        record_oracle(&one(no_prompt)).is_err(),
        "rule 148: a sent call without its prompt"
    );
    let mut kept = good.clone();
    kept.outcome = Outcome::InputRejected;
    kept.guard_rule = Some(mandate_gateway::guard::GuardRule::Credential);
    kept.prompt_ref = None;
    assert!(
        record_oracle(&one(kept)).is_err(),
        "rule 148: a refused request's digest kept"
    );
    let mut reserved_refusal = good.clone();
    reserved_refusal.outcome = Outcome::PolicyDenied;
    assert!(record_oracle(&one(reserved_refusal)).is_err(), "rule 149");
    let mut unreserved = good.clone();
    unreserved.reservation = None;
    assert!(
        record_oracle(&one(unreserved)).is_err(),
        "rule 149: sent without a reservation"
    );
    let mut unsent_ok = good.clone();
    unsent_ok.attempts = 0;
    assert!(record_oracle(&one(unsent_ok)).is_err(), "rule 152");
    let mut orphan = good.clone();
    orphan.reservation = Some("01J9GW3R000000000000000999".to_owned());
    assert!(
        record_oracle(&one(orphan)).is_err(),
        "a reservation missing from the meter stream"
    );
}
