//! The meter (E15-9) against journal spec §9.19's generated vectors: each meter record's payload is
//! the vectors' exactly, rule 158 judges every meter stream as the vectors expect, the fold gives
//! every stated spend, and a reservation is admitted exactly when the vectors' admissions say. Then
//! the caps of DEC-997 item 3.

mod common;

use std::sync::OnceLock;

use common::*;
use mandate_canon::{Digest, Value, to_canonical};
use mandate_gateway::kinds::{KeyOwner, Outcome, Purpose};
use mandate_gateway::meter::{
    CapError, Caps, MemoryMeter, Meter, MeterRecord, MeterRefusal, MeterStore, Period, Reservation,
    Scope, Settlement, StoreError, rule_158, spend,
};
use mandate_gateway::money::{Usage, Usd6};
use mandate_gateway::registry::ModelRef;
use mandate_time::Date;
use proptest::prelude::*;

const VECTORS: &str = include_str!("../../../fixtures/refcases/journal.json");

fn section() -> &'static serde_json::Value {
    static SECTION: OnceLock<serde_json::Value> = OnceLock::new();
    SECTION.get_or_init(|| {
        let all: serde_json::Value = serde_json::from_str(VECTORS).unwrap();
        all["model_gateway"].clone()
    })
}

fn digest(reference: &serde_json::Value) -> Digest {
    Digest::from_hex(reference.as_str().unwrap().strip_prefix("sha256:").unwrap()).unwrap()
}

fn text(v: &serde_json::Value) -> String {
    v.as_str().unwrap().to_owned()
}

fn usage(v: &serde_json::Value) -> Usage {
    Usage {
        input: v["input"].as_u64().unwrap(),
        output: v["output"].as_u64().unwrap(),
        cached: v["cached"].as_u64().unwrap(),
    }
}

fn outcome(code: &str) -> Outcome {
    Outcome::ALL.into_iter().find(|o| o.code() == code).unwrap()
}

fn purpose(code: &str) -> Purpose {
    [
        Purpose::Research,
        Purpose::LlmSignal,
        Purpose::FastSignal,
        Purpose::Compiler,
    ]
    .into_iter()
    .find(|p| p.code() == code)
    .unwrap()
}

/// A vectors draft as the gateway's record, or `None` for the stream's `StreamOpened`.
fn record(draft: &serde_json::Value) -> Option<MeterRecord> {
    let p = &draft["payload"];
    let event_id = text(&draft["event_id"]);
    match draft["event_type"].as_str().unwrap() {
        "MeterReserved" => Some(MeterRecord::Reserved(Reservation {
            event_id,
            call_id: text(&p["call_id"]),
            agent_id: p["agent_id"].as_str().map(str::to_owned),
            purpose: purpose(p["purpose"].as_str().unwrap()),
            model: ModelRef {
                model_id: text(&p["model"]["model_id"]),
                model_version: text(&p["model"]["model_version"]),
                content_hash: digest(&p["model"]["content_hash"]),
            },
            endpoint: text(&p["endpoint"]),
            key_owner: if p["key_owner"] == "platform" {
                KeyOwner::Platform
            } else {
                KeyOwner::Customer
            },
            price_table_ref: digest(&p["price_table_ref"]),
            max_cost: Usd6::parse(p["max_cost_usd"].as_str().unwrap()).unwrap(),
            risk_day: Date::parse(p["risk_day"].as_str().unwrap()).unwrap(),
        })),
        "MeterSettled" => Some(MeterRecord::Settled(Settlement {
            event_id,
            call_id: text(&p["call_id"]),
            reservation: text(&p["reservation"]),
            outcome: outcome(p["outcome"].as_str().unwrap()),
            attempts: u32::try_from(p["attempts"].as_u64().unwrap()).unwrap(),
            tokens: usage(&p["tokens"]),
            cost: Usd6::parse(p["cost_usd"].as_str().unwrap()).unwrap(),
        })),
        _ => None,
    }
}

fn records(case: &serde_json::Value) -> Vec<MeterRecord> {
    case["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(record)
        .collect()
}

fn canonical_json(value: &serde_json::Value) -> Vec<u8> {
    to_canonical(&mandate_canon::parse(serde_json::to_string(value).unwrap().as_bytes()).unwrap())
}

fn scope(text: &str) -> Scope {
    if text == "workspace" {
        Scope::Workspace
    } else {
        Scope::Agent(text.to_owned())
    }
}

fn period(text: &str) -> Period {
    match text.split_once(':').unwrap() {
        ("day", d) => Period::Day(Date::parse(d).unwrap()),
        ("month", m) => {
            let (y, mo) = m.split_once('-').unwrap();
            Period::Month {
                year: y.parse().unwrap(),
                month: mo.parse().unwrap(),
            }
        }
        _ => panic!("unknown period {text}"),
    }
}

#[test]
fn each_meter_record_payload_is_the_vectors_payload() {
    let drafts = &section()["drafts"];
    for name in ["reserved", "settled"] {
        let draft = &drafts[name];
        let ours = record(draft).unwrap();
        assert_eq!(ours.event_type(), draft["event_type"]);
        assert_eq!(ours.event_id(), draft["event_id"]);
        assert_eq!(
            to_canonical(&ours.payload()),
            canonical_json(&draft["payload"]),
            "{name}"
        );
    }
    let mut seen = 0;
    for case in section()["folds"].as_array().unwrap() {
        for draft in case["records"].as_array().unwrap() {
            if let Some(ours) = record(draft) {
                assert_eq!(
                    to_canonical(&ours.payload()),
                    canonical_json(&draft["payload"])
                );
                seen += 1;
            }
        }
    }
    assert!(seen > 20);
}

#[test]
fn rule_158_judges_every_vectors_meter_stream() {
    let streams = section()["streams"].as_array().unwrap();
    assert!(streams.len() >= 8);
    for case in streams {
        let all: Vec<MeterRecord> = records(case);
        let first_failure = (0..all.len()).find(|&i| rule_158(&all[..i], &all[i]).is_err());
        let expect = &case["expect"];
        match expect["outcome"].as_str().unwrap() {
            "Valid" => assert_eq!(first_failure, None, "{}", case["name"]),
            _ => {
                let index = usize::try_from(expect["index"].as_u64().unwrap()).unwrap() - 1;
                assert_eq!(first_failure, Some(index), "{}", case["name"]);
            }
        }
        let mut store = MemoryMeter::new();
        let appended = all
            .iter()
            .take_while(|r| store.append((*r).clone()).is_ok())
            .count();
        assert_eq!(
            appended,
            first_failure.unwrap_or(all.len()),
            "the store keeps rule 158: {}",
            case["name"]
        );
        if let Some(i) = first_failure {
            assert!(matches!(
                rule_158(&all[..i], &all[i]),
                Err(StoreError::Rule158(_))
            ));
        }
    }
}

#[test]
fn the_fold_gives_every_spend_the_vectors_state() {
    for case in section()["folds"].as_array().unwrap() {
        let all = records(case);
        for e in case["expect"].as_array().unwrap() {
            let want = Usd6::parse(e["spend_usd"].as_str().unwrap()).unwrap();
            let got = spend(
                &all,
                &scope(e["scope"].as_str().unwrap()),
                period(e["period"].as_str().unwrap()),
            )
            .unwrap();
            assert_eq!(got, want, "{} {e}", case["name"]);
        }
    }
}

#[test]
fn a_reservation_is_admitted_exactly_when_the_vectors_admissions_say() {
    for case in section()["folds"].as_array().unwrap() {
        let all = records(case);
        for (n, a) in case["admissions"].as_array().unwrap().iter().enumerate() {
            let cap = Usd6::parse(a["cap_usd"].as_str().unwrap()).unwrap();
            let max_cost = Usd6::parse(a["max_cost_usd"].as_str().unwrap()).unwrap();
            let (caps, agent, day) = match (
                scope(a["scope"].as_str().unwrap()),
                period(a["period"].as_str().unwrap()),
            ) {
                (Scope::Workspace, Period::Month { year, month }) => {
                    (Caps::new(cap), None, Date::new(year, month, 20).unwrap())
                }
                (Scope::Agent(agent), Period::Day(day)) => (
                    Caps::new(Usd6::dollars(1_000).unwrap())
                        .with_research_daily(&agent, cap)
                        .unwrap(),
                    Some(agent),
                    day,
                ),
                other => panic!("no cap of shape {other:?}"),
            };
            let mut store = MemoryMeter::new();
            for r in &all {
                store.append(r.clone()).unwrap();
            }
            let mut meter = Meter::new(store, caps, common::deployment());
            let reservation = Reservation {
                event_id: mandate_canon::encode_ulid(1 << 100),
                call_id: mandate_canon::encode_ulid((1 << 101) + u128::try_from(n).unwrap()),
                agent_id: agent.clone(),
                purpose: if agent.is_some() {
                    Purpose::Research
                } else {
                    Purpose::Compiler
                },
                model: content().model_ref(),
                endpoint: "openrouter:anthropic".to_owned(),
                key_owner: KeyOwner::Platform,
                price_table_ref: prices().version(),
                max_cost,
                risk_day: day,
            };
            let admitted = meter.reserve(reservation).is_ok();
            assert_eq!(
                admitted,
                a["admitted"].as_bool().unwrap(),
                "{} {a}",
                case["name"]
            );
            let length = meter.store().records().len();
            assert_eq!(
                length,
                all.len() + usize::from(admitted),
                "a refused reservation is not recorded"
            );
        }
    }
}

fn reservation(
    n: u128,
    agent: Option<&str>,
    purpose: Purpose,
    max: &str,
    day: Date,
) -> Reservation {
    Reservation {
        event_id: mandate_canon::encode_ulid((1 << 110) + n),
        call_id: mandate_canon::encode_ulid((1 << 111) + n),
        agent_id: agent.map(str::to_owned),
        purpose,
        model: content().model_ref(),
        endpoint: "openrouter:anthropic".to_owned(),
        key_owner: KeyOwner::Platform,
        price_table_ref: prices().version(),
        max_cost: Usd6::parse(max).unwrap(),
        risk_day: day,
    }
}

fn day(d: u8) -> Date {
    Date::new(2026, 10, d).unwrap()
}

#[test]
fn a_configured_monthly_budget_admits_up_to_itself() {
    let caps = Caps::new(Usd6::dollars(50).unwrap());
    assert_eq!(caps.workspace_monthly(), Usd6::dollars(50).unwrap());
    let mut meter = Meter::new(MemoryMeter::new(), caps, common::deployment());
    assert!(
        meter
            .reserve(reservation(
                1,
                Some("a"),
                Purpose::Research,
                "49.99",
                day(1)
            ))
            .is_ok()
    );
    assert!(
        meter
            .reserve(reservation(
                2,
                Some("b"),
                Purpose::LlmSignal,
                "0.01",
                day(31)
            ))
            .is_ok(),
        "exactly at the cap"
    );
    assert_eq!(
        meter.reserve(reservation(3, None, Purpose::Compiler, "0.000001", day(16))),
        Err(MeterRefusal::BudgetExhausted)
    );
    assert!(
        meter
            .reserve(reservation(
                4,
                Some("a"),
                Purpose::Research,
                "1",
                Date::new(2026, 11, 1).unwrap()
            ))
            .is_ok(),
        "a new calendar month starts from zero"
    );
}

#[test]
fn per_agent_caps_sit_under_the_workspace_cap() {
    let caps = common::caps()
        .with_agent_monthly("a", Usd6::dollars(5).unwrap())
        .unwrap()
        .with_agent_monthly_default(Usd6::dollars(10).unwrap())
        .unwrap();
    assert_eq!(
        common::caps().with_agent_monthly("a", Usd6::dollars(51).unwrap()),
        Err(CapError::AboveWorkspace)
    );
    assert_eq!(
        common::caps().with_agent_monthly_default(Usd6::dollars(51).unwrap()),
        Err(CapError::AboveWorkspace)
    );
    assert!(
        common::caps()
            .with_agent_monthly_default(Usd6::dollars(50).unwrap())
            .is_ok(),
        "the workspace cap itself"
    );
    assert!(
        common::caps()
            .with_agent_monthly("a", Usd6::dollars(50).unwrap())
            .is_ok()
    );
    let mut meter = Meter::new(MemoryMeter::new(), caps, common::deployment());
    assert!(
        meter
            .reserve(reservation(1, Some("a"), Purpose::LlmSignal, "5", day(2)))
            .is_ok()
    );
    assert_eq!(
        meter.reserve(reservation(
            2,
            Some("a"),
            Purpose::LlmSignal,
            "0.01",
            day(3)
        )),
        Err(MeterRefusal::BudgetExhausted)
    );
    assert!(
        meter
            .reserve(reservation(3, Some("b"), Purpose::LlmSignal, "10", day(3)))
            .is_ok(),
        "the default cap"
    );
    assert_eq!(
        meter.reserve(reservation(
            4,
            Some("b"),
            Purpose::LlmSignal,
            "0.01",
            day(3)
        )),
        Err(MeterRefusal::BudgetExhausted)
    );
    assert!(
        meter
            .reserve(reservation(5, None, Purpose::Compiler, "35", day(4)))
            .is_ok(),
        "the compiler has only the workspace cap"
    );
    assert_eq!(
        meter.reserve(reservation(
            6,
            Some("c"),
            Purpose::LlmSignal,
            "0.01",
            day(4)
        )),
        Err(MeterRefusal::BudgetExhausted)
    );
}

#[test]
fn the_research_daily_cap_counts_one_risk_day_of_the_agents_calls() {
    let caps = common::caps()
        .with_research_daily("a", Usd6::parse("0.5").unwrap())
        .unwrap();
    let mut meter = Meter::new(MemoryMeter::new(), caps, common::deployment());
    assert!(
        meter
            .reserve(reservation(1, Some("a"), Purpose::Research, "0.4", day(5)))
            .is_ok()
    );
    assert!(
        meter
            .reserve(reservation(
                2,
                Some("a"),
                Purpose::LlmSignal,
                "0.05",
                day(5)
            ))
            .is_ok(),
        "not a research call"
    );
    assert_eq!(
        meter.reserve(reservation(3, Some("a"), Purpose::Research, "0.06", day(5))),
        Err(MeterRefusal::BudgetExhausted)
    );
    assert!(
        meter
            .reserve(reservation(4, Some("a"), Purpose::Research, "0.05", day(5)))
            .is_ok(),
        "exactly at the cap"
    );
    assert!(
        meter
            .reserve(reservation(5, Some("a"), Purpose::Research, "0.5", day(6)))
            .is_ok(),
        "the next risk day"
    );
    assert_eq!(
        meter.research_spend("a", day(5)),
        Ok(Usd6::parse("0.5").unwrap())
    );
    assert_eq!(meter.research_spend("b", day(5)), Ok(Usd6::ZERO));
}

#[test]
fn settling_releases_what_a_call_did_not_spend_and_an_open_reservation_counts_whole() {
    let mut meter = Meter::new(
        MemoryMeter::new(),
        Caps::new(Usd6::dollars(1).unwrap()),
        common::deployment(),
    );
    let r = meter
        .reserve(reservation(1, Some("a"), Purpose::Research, "0.8", day(7)))
        .unwrap();
    assert_eq!(
        meter.reserve(reservation(2, Some("a"), Purpose::Research, "0.3", day(7))),
        Err(MeterRefusal::BudgetExhausted)
    );
    meter
        .settle(Settlement {
            event_id: mandate_canon::encode_ulid(1 << 120),
            call_id: r.call_id.clone(),
            reservation: r.event_id.clone(),
            outcome: Outcome::Ok,
            attempts: 1,
            tokens: Usage {
                input: 1,
                output: 1,
                cached: 0,
            },
            cost: Usd6::parse("0.1").unwrap(),
        })
        .unwrap();
    assert!(
        meter
            .reserve(reservation(2, Some("a"), Purpose::Research, "0.3", day(7)))
            .is_ok()
    );
    let month = Period::Month {
        year: 2026,
        month: 10,
    };
    assert_eq!(
        meter.spend(&Scope::Workspace, month),
        Ok(Usd6::parse("0.4").unwrap())
    );
    assert_eq!(meter.reservations_of("a").unwrap().len(), 2);
    assert!(meter.reservations_of("b").unwrap().is_empty());
}

#[test]
fn a_meter_that_cannot_be_read_or_appended_refuses() {
    for failure in [StoreError::Unavailable, StoreError::Fenced] {
        let mut store = MemoryMeter::new();
        store.fail_with(Some(failure));
        let mut meter = Meter::new(store, common::caps(), common::deployment());
        assert_eq!(
            meter.reserve(reservation(1, Some("a"), Purpose::Research, "0.1", day(8))),
            Err(MeterRefusal::Unavailable)
        );
        assert_eq!(
            meter.spend(&Scope::Workspace, Period::Day(day(8))),
            Err(MeterRefusal::Unavailable)
        );
        assert_eq!(meter.reservations_of("a"), Err(MeterRefusal::Unavailable));
    }
    let mut store = MemoryMeter::new();
    store.limit_appends(Some(0));
    let mut meter = Meter::new(store, common::caps(), common::deployment());
    assert_eq!(
        meter.reserve(reservation(1, Some("a"), Purpose::Research, "0.1", day(8))),
        Err(MeterRefusal::Orphaned),
        "a workspace append that fails after the copy leaves an orphan (DEC-2207 item 4)"
    );
    assert!(meter.store().records().is_empty());
    assert_eq!(
        meter.deployment().spend(Period::Month {
            year: 2026,
            month: 10
        }),
        Ok(Usd6::parse("0.1").unwrap()),
        "the copy appended before the failed workspace append is an orphan, counted at its maximum (DEC-2207 item 4)"
    );
    meter.store_mut().limit_appends(None);
    assert_eq!(
        meter.reserve(reservation(1, Some("a"), Purpose::Research, "0.1", day(8))),
        Err(MeterRefusal::Unavailable),
        "the orphan holds its call id on the deployment stream, so trying again needs a new call (rule 158)"
    );
    assert!(
        meter
            .reserve(reservation(2, Some("a"), Purpose::Research, "0.1", day(8)))
            .is_ok()
    );
    assert_eq!(
        meter.reserve(reservation(2, Some("a"), Purpose::Research, "0.1", day(8))),
        Err(MeterRefusal::Unavailable),
        "a second reservation of one call is refused (rule 158)"
    );
}

#[test]
fn an_empty_stream_spends_nothing() {
    assert_eq!(
        spend(&[], &Scope::Workspace, Period::Day(day(1))),
        Ok(Usd6::ZERO)
    );
    let _ = Value::Null;
}

#[test]
fn a_research_cap_sits_under_the_workspace_cap_too() {
    assert_eq!(
        common::caps().with_research_daily("a", Usd6::parse("50.000001").unwrap()),
        Err(CapError::AboveWorkspace)
    );
    assert!(
        common::caps()
            .with_research_daily("a", Usd6::dollars(50).unwrap())
            .is_ok()
    );
}

#[test]
fn a_reservation_settled_twice_counts_its_higher_cost() {
    let r = reservation(1, Some("a"), Purpose::Research, "0.5", day(9));
    let settle = |n: u128, cost: &str| {
        MeterRecord::Settled(Settlement {
            event_id: mandate_canon::encode_ulid((1 << 121) + n),
            call_id: r.call_id.clone(),
            reservation: r.event_id.clone(),
            outcome: Outcome::Ok,
            attempts: 1,
            tokens: Usage::default(),
            cost: Usd6::parse(cost).unwrap(),
        })
    };
    let records = vec![
        MeterRecord::Reserved(r.clone()),
        settle(1, "0.3"),
        settle(2, "0.1"),
    ];
    assert_eq!(
        spend(&records, &Scope::Workspace, Period::Day(day(9))),
        Ok(Usd6::parse("0.3").unwrap()),
        "a store that let a second settlement through never lowers spend"
    );
}

fn canon(value: &serde_json::Value) -> Value {
    mandate_canon::parse(serde_json::to_string(value).unwrap().as_bytes()).unwrap()
}

/// DEC-2258 item 3: every meter draft of journal spec §9.19's vectors reads back as the record the
/// vectors' own fields give, and the stream's `StreamOpened` as none.
#[test]
fn every_vectors_meter_draft_reads_back_as_its_record() {
    let mut drafts: Vec<serde_json::Value> = ["reserved", "settled"]
        .iter()
        .map(|name| section()["drafts"][*name].clone())
        .collect();
    for case in section()["folds"].as_array().unwrap() {
        drafts.extend(case["records"].as_array().unwrap().iter().cloned());
    }
    let mut opened = 0;
    for draft in &drafts {
        let event_type = draft["event_type"].as_str().unwrap();
        let read = MeterRecord::from_journal(
            event_type,
            draft["event_id"].as_str().unwrap(),
            &canon(&draft["payload"]),
        );
        assert_eq!(read, record(draft), "{event_type} {}", draft["event_id"]);
        opened += usize::from(event_type == "StreamOpened");
    }
    assert!(drafts.len() > 20 && opened > 0, "{} {opened}", drafts.len());
}

/// DEC-2258 item 3: a meter payload with a member missing, an unlisted member, or a value of the
/// wrong kind or out of its range reads back as nothing, as does an event type of no meter record.
#[test]
fn a_payload_it_cannot_read_back_is_no_record() {
    for name in ["reserved", "settled"] {
        let draft = &section()["drafts"][name];
        let event_type = draft["event_type"].as_str().unwrap();
        let event_id = draft["event_id"].as_str().unwrap();
        let payload = &draft["payload"];
        assert!(MeterRecord::from_journal(event_type, event_id, &canon(payload)).is_some());
        assert_eq!(
            MeterRecord::from_journal("ModelInvocationRecorded", event_id, &canon(payload)),
            None
        );
        for member in payload.as_object().unwrap().keys() {
            let mut missing = payload.clone();
            missing.as_object_mut().unwrap().remove(member);
            assert_eq!(
                MeterRecord::from_journal(event_type, event_id, &canon(&missing)),
                None,
                "{name} without {member}"
            );
        }
        let mut extra = payload.clone();
        extra["note"] = serde_json::json!("x");
        assert_eq!(
            MeterRecord::from_journal(event_type, event_id, &canon(&extra)),
            None,
            "{name} with an unlisted member"
        );
    }
    let reserved = &section()["drafts"]["reserved"];
    let settled = &section()["drafts"]["settled"];
    for (draft, member, value) in [
        (reserved, "max_cost_usd", serde_json::json!("-1")),
        (reserved, "max_cost_usd", serde_json::json!("0.0000001")),
        (reserved, "purpose", serde_json::json!("trading")),
        (reserved, "key_owner", serde_json::json!("nobody")),
        (reserved, "risk_day", serde_json::json!("2026-13-01")),
        (reserved, "price_table_ref", serde_json::json!("md5:00")),
        (reserved, "agent_id", serde_json::json!(7)),
        (settled, "outcome", serde_json::json!("budget_exhausted")),
        (settled, "attempts", serde_json::json!("1")),
        (settled, "attempts", serde_json::json!(4_294_967_296_u64)),
        (settled, "cost_usd", serde_json::json!("-0.5")),
        (settled, "tokens", serde_json::json!({"input": 1})),
    ] {
        let mut bad = draft["payload"].clone();
        bad[member] = value.clone();
        assert_eq!(
            MeterRecord::from_journal(
                draft["event_type"].as_str().unwrap(),
                draft["event_id"].as_str().unwrap(),
                &canon(&bad)
            ),
            None,
            "{member} = {value}"
        );
    }
}

/// The outcomes a `MeterSettled` may carry (journal spec §9.19): every outcome but a refusal.
const SETTLED_OUTCOMES: [Outcome; 8] = [
    Outcome::Ok,
    Outcome::DeadlineExceeded,
    Outcome::ProviderUnavailable,
    Outcome::RateLimitedProvider,
    Outcome::CredentialInvalid,
    Outcome::ContentRefused,
    Outcome::SchemaInvalid,
    Outcome::IdentityMismatch,
];

fn generated_record() -> impl Strategy<Value = MeterRecord> {
    let model = || ModelRef {
        model_id: "llm.research_agent".to_owned(),
        model_version: "0.1.0".to_owned(),
        content_hash: Digest::of(b"model"),
    };
    let reserved = (
        1_u128..1 << 80,
        proptest::option::of("[a-z][a-z0-9-]{0,11}"),
        0_usize..4,
        0_u64..1 << 40,
        0_u32..3_360,
        any::<bool>(),
    )
        .prop_map(move |(n, agent, purpose, max, days, platform)| {
            let purpose = [
                Purpose::Research,
                Purpose::LlmSignal,
                Purpose::FastSignal,
                Purpose::Compiler,
            ][purpose];
            MeterRecord::Reserved(Reservation {
                event_id: mandate_canon::encode_ulid(n),
                call_id: mandate_canon::encode_ulid(n + 1),
                agent_id: if purpose == Purpose::Compiler {
                    None
                } else {
                    agent
                },
                purpose,
                model: model(),
                endpoint: format!("endpoint-{n}"),
                key_owner: if platform {
                    KeyOwner::Platform
                } else {
                    KeyOwner::Customer
                },
                price_table_ref: Digest::of(&n.to_be_bytes()),
                max_cost: Usd6::from_micros(max),
                risk_day: Date::new(
                    2026 + u16::try_from(days / 336).unwrap(),
                    u8::try_from(days % 12).unwrap() + 1,
                    u8::try_from(days % 28).unwrap() + 1,
                )
                .unwrap(),
            })
        });
    let settled = (
        1_u128..1 << 80,
        0_usize..SETTLED_OUTCOMES.len(),
        0_u32..5,
        (0_u64..1 << 30, 0_u64..1 << 30, 0_u64..1 << 30),
        0_u64..1 << 40,
    )
        .prop_map(|(n, outcome, attempts, (input, output, cached), cost)| {
            MeterRecord::Settled(Settlement {
                event_id: mandate_canon::encode_ulid(n + 2),
                call_id: mandate_canon::encode_ulid(n + 1),
                reservation: mandate_canon::encode_ulid(n),
                outcome: SETTLED_OUTCOMES[outcome],
                attempts,
                tokens: Usage {
                    input,
                    output,
                    cached,
                },
                cost: Usd6::from_micros(cost),
            })
        });
    prop_oneof![reserved, settled]
}

/// DEC-2258 item 3: any meter record reads back from its own payload, at its own event id, as
/// itself, over 256 generated records.
#[test]
fn any_meter_record_reads_back_from_its_payload() {
    let config = ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    let run = proptest::test_runner::TestRunner::new(config).run(&generated_record(), |record| {
        prop_assert_eq!(
            MeterRecord::from_journal(record.event_type(), record.event_id(), &record.payload()),
            Some(record.clone())
        );
        Ok(())
    });
    if let Err(failure) = run {
        panic!("{failure}");
    }
}
