//! The deployment's model-spend cap (DEC-2204; journal spec v0.50 §9.19's deployment meter stream):
//! every workspace's reservations and settlements are copied to one deployment stream, each copy its
//! own record, the copy reaching it before the workspace's own stream, so any failure between them
//! only over-counts and the deployment's spend never passes its cap.

mod common;

use std::collections::BTreeMap;

use common::*;
use mandate_canon::encode_ulid;
use mandate_gateway::kinds::{KeyOwner, Outcome, Purpose};
use mandate_gateway::meter::{
    Caps, DeploymentMeter, MemoryMeter, Meter, MeterRecord, MeterRefusal, MeterStore, Period,
    Reservation, Settlement, SharedMeter, StoreError, rule_158,
};
use mandate_gateway::money::{Usage, Usd6};
use mandate_gateway::provider::SequentialIds;
use mandate_time::Date;
use proptest::prelude::*;

const OCTOBER: Period = Period::Month {
    year: 2026,
    month: 10,
};

fn usd(text: &str) -> Usd6 {
    Usd6::parse(text).unwrap()
}

fn day(d: u8) -> Date {
    Date::new(2026, 10, d).unwrap()
}

fn workspace_meter(shared: &SharedMeter, cap: &str, ids_from: u128) -> Meter<MemoryMeter> {
    Meter::new(
        MemoryMeter::new(),
        Caps::new(usd("1000")),
        DeploymentMeter::new(
            Box::new(shared.clone()),
            usd(cap),
            Box::new(SequentialIds::starting_at(ids_from)),
        ),
    )
}

/// Workspace `ws`'s `n`th call.
fn reservation(ws: u8, n: u32, max: &str) -> Reservation {
    let id = (u128::from(ws) << 40) + u128::from(n);
    Reservation {
        event_id: encode_ulid((1 << 110) + id),
        call_id: encode_ulid((1 << 111) + id),
        agent_id: Some(format!("agent_{ws}")),
        purpose: Purpose::Research,
        model: content().model_ref(),
        endpoint: "openrouter:anthropic".to_owned(),
        key_owner: KeyOwner::Platform,
        price_table_ref: prices().version(),
        max_cost: usd(max),
        risk_day: day(16),
    }
}

fn settlement(r: &Reservation, cost: &str, n: u128) -> Settlement {
    Settlement {
        event_id: encode_ulid((1 << 112) + n),
        call_id: r.call_id.clone(),
        reservation: r.event_id.clone(),
        outcome: Outcome::Ok,
        attempts: 1,
        tokens: Usage::default(),
        cost: usd(cost),
    }
}

#[test]
fn one_workspaces_spend_takes_the_deployments_headroom_from_another() {
    let shared = SharedMeter::new();
    let mut a = workspace_meter(&shared, "1", 1 << 100);
    let mut b = workspace_meter(&shared, "1", 1 << 101);
    assert!(a.reserve(reservation(1, 1, "0.7")).is_ok());
    assert_eq!(
        b.reserve(reservation(2, 1, "0.4")),
        Err(MeterRefusal::BudgetExhausted),
        "0.7 + 0.4 passes the deployment's 1"
    );
    assert!(
        b.store().records().is_empty(),
        "the refusal appended nothing to b's stream"
    );
    assert_eq!(shared.records().len(), 1, "nor to the deployment's");
    assert!(
        b.reserve(reservation(2, 2, "0.3")).is_ok(),
        "0.7 + 0.3 fits exactly"
    );
    assert_eq!(a.deployment().spend(OCTOBER), Ok(usd("1")));
    assert_eq!(b.deployment().spend(OCTOBER), Ok(usd("1")));
}

#[test]
fn a_settlement_lowers_the_deployments_spend_to_the_calls_cost() {
    let shared = SharedMeter::new();
    let mut a = workspace_meter(&shared, "1", 1 << 100);
    let mut b = workspace_meter(&shared, "1", 1 << 101);
    let r = a.reserve(reservation(1, 1, "0.7")).unwrap();
    a.settle(settlement(&r, "0.1", 1)).unwrap();
    assert_eq!(a.deployment().spend(OCTOBER), Ok(usd("0.1")));
    assert!(b.reserve(reservation(2, 1, "0.9")).is_ok());
}

#[test]
fn each_copy_is_its_own_record_and_a_settlements_copy_names_the_copy_of_its_reservation() {
    let shared = SharedMeter::new();
    let mut a = workspace_meter(&shared, "5", 1 << 100);
    let r = a.reserve(reservation(1, 1, "0.7")).unwrap();
    let s = settlement(&r, "0.2", 2);
    a.settle(s.clone()).unwrap();
    let records = shared.records();
    let [MeterRecord::Reserved(copy), MeterRecord::Settled(settled)] = records.as_slice() else {
        panic!(
            "a reservation copy then a settlement copy: {:?}",
            shared.records()
        );
    };
    assert_ne!(copy.event_id, r.event_id, "a copy has its own event id");
    assert_eq!(
        Reservation {
            event_id: r.event_id.clone(),
            ..copy.clone()
        },
        r,
        "every other member is the workspace record's"
    );
    assert_ne!(settled.event_id, s.event_id);
    assert_eq!(
        settled.reservation, copy.event_id,
        "rule 158 holds on the deployment stream"
    );
    assert_eq!(
        Settlement {
            event_id: s.event_id.clone(),
            reservation: s.reservation.clone(),
            ..settled.clone()
        },
        s
    );
}

#[test]
fn a_workspace_refusal_appends_nothing_to_either_stream() {
    let shared = SharedMeter::new();
    let mut a = Meter::new(
        MemoryMeter::new(),
        Caps::new(usd("0.5")),
        DeploymentMeter::new(
            Box::new(shared.clone()),
            usd("10"),
            Box::new(SequentialIds::starting_at(1 << 100)),
        ),
    );
    assert_eq!(
        a.reserve(reservation(1, 1, "0.6")),
        Err(MeterRefusal::BudgetExhausted)
    );
    assert!(a.store().records().is_empty());
    assert!(
        shared.records().is_empty(),
        "the workspace's caps are checked first"
    );
}

#[test]
fn an_unreachable_deployment_writer_refuses_and_nothing_is_appended() {
    for failure in [StoreError::Unavailable, StoreError::Fenced] {
        let shared = SharedMeter::new();
        shared.fail_with(Some(failure));
        let mut a = workspace_meter(&shared, "5", 1 << 100);
        assert_eq!(
            a.reserve(reservation(1, 1, "0.1")),
            Err(MeterRefusal::Unavailable)
        );
        assert!(a.store().records().is_empty());
        shared.fail_with(None);
        assert!(shared.records().is_empty());
        assert_eq!(a.deployment().spend(OCTOBER), Ok(Usd6::ZERO));
    }
    let shared = SharedMeter::new();
    shared.limit_appends(Some(0));
    let mut a = workspace_meter(&shared, "5", 1 << 100);
    assert_eq!(
        a.reserve(reservation(1, 1, "0.1")),
        Err(MeterRefusal::Unavailable)
    );
    assert!(a.store().records().is_empty());
}

#[test]
fn a_failed_settlement_copy_leaves_the_deployment_counting_the_maximum() {
    let shared = SharedMeter::new();
    let mut a = workspace_meter(&shared, "5", 1 << 100);
    let r = a.reserve(reservation(1, 1, "0.7")).unwrap();
    shared.fail_with(Some(StoreError::Unavailable));
    assert_eq!(
        a.settle(settlement(&r, "0.1", 3)),
        Err(StoreError::Unavailable)
    );
    shared.fail_with(None);
    assert_eq!(
        a.store().records().len(),
        2,
        "the workspace's own settlement is appended"
    );
    assert_eq!(
        a.deployment().spend(OCTOBER),
        Ok(usd("0.7")),
        "over-counts, never under"
    );
    let mut b = workspace_meter(&shared, "5", 1 << 101);
    let r = b.reserve(reservation(2, 1, "0.3")).unwrap();
    b.store_mut().fail_with(Some(StoreError::Unavailable));
    assert_eq!(
        b.settle(settlement(&r, "0.1", 4)),
        Err(StoreError::Unavailable),
        "a failed workspace settlement is reported"
    );
    b.store_mut().fail_with(None);
    assert_eq!(
        b.store().records().len(),
        1,
        "the workspace counts its maximum"
    );
    assert_eq!(
        b.deployment().spend(OCTOBER),
        Ok(usd("1")),
        "and so does the deployment: no copy settles a call its workspace has not (DEC-2204 item 5)"
    );
}

#[test]
fn the_gateway_refuses_a_call_the_deployment_cannot_afford() {
    let shared = SharedMeter::new();
    let (registry, model) = registry();
    let mut gateway = mandate_gateway::Gateway::new(
        settings(hosted_policy()),
        registry,
        Box::new(mandate_gateway::openrouter::OpenRouter),
        mandate_gateway::fake::RecordedTransport::new(),
        Box::new(TestKey),
        Meter::new(
            MemoryMeter::new(),
            caps(),
            DeploymentMeter::new(
                Box::new(shared.clone()),
                usd("0.000001"),
                Box::new(SequentialIds::starting_at(1 << 100)),
            ),
        ),
        mandate_gateway::provider::ManualClock::at(at(START)),
        Box::new(SequentialIds::starting_at(1 << 80)),
    );
    gateway.transport_mut().script(vec![ok_reply(10)]);
    let record = gateway.call(&request(&model, 1)).unwrap();
    assert_eq!(record.outcome, Outcome::BudgetExhausted);
    assert_eq!(record.reservation, None);
    assert!(gateway.transport().sent().is_empty());
    assert!(gateway.meter().store().records().is_empty());
    assert!(shared.records().is_empty());
}

/// The independent oracle: spend folded from records by its own accumulator, one pass, each
/// reservation counted at its settlement's cost when one names it and at its maximum otherwise.
fn oracle_spend(records: &[MeterRecord]) -> BTreeMap<String, i128> {
    let mut cost: BTreeMap<String, i128> = BTreeMap::new();
    let mut by_reservation: BTreeMap<String, String> = BTreeMap::new();
    for record in records {
        match record {
            MeterRecord::Reserved(r) => {
                by_reservation.insert(r.event_id.clone(), r.call_id.clone());
                cost.insert(r.call_id.clone(), i128::from(r.max_cost.micros()));
            }
            MeterRecord::Settled(s) => {
                if let Some(call) = by_reservation.get(&s.reservation) {
                    cost.insert(call.clone(), i128::from(s.cost.micros()));
                }
            }
        }
    }
    cost
}

/// Every claim of DEC-2204 items 3 to 5 over the records: the deployment's spend is at most its cap,
/// and every call a workspace stream counts the deployment counts at least as high.
fn check(
    deployment: &[MeterRecord],
    workspaces: &[Vec<MeterRecord>],
    cap: i128,
) -> Result<(), String> {
    let dmeter = oracle_spend(deployment);
    let total: i128 = dmeter.values().sum();
    if total > cap {
        return Err(format!("deployment spend {total} passes its cap {cap}"));
    }
    for records in workspaces {
        for (call, counted) in oracle_spend(records) {
            match dmeter.get(&call) {
                Some(d) if *d >= counted => {}
                other => {
                    return Err(format!(
                        "call {call}: workspace {counted}, deployment {other:?}"
                    ));
                }
            }
        }
    }
    let mut earlier = Vec::new();
    for record in deployment {
        rule_158(&earlier, record)
            .map_err(|e| format!("rule 158 on the deployment stream: {e}"))?;
        earlier.push(record.clone());
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum Step {
    Reserve {
        ws: usize,
        max_micros: u64,
    },
    Settle {
        ws: usize,
        cost_micros: u64,
    },
    FailWorkspace {
        ws: usize,
        failure: Option<StoreError>,
    },
    LimitWorkspace {
        ws: usize,
        more: Option<usize>,
    },
    FailDeployment(Option<StoreError>),
}

fn step() -> impl Strategy<Value = Step> {
    let failure = prop_oneof![
        Just(None),
        Just(Some(StoreError::Unavailable)),
        Just(Some(StoreError::Fenced)),
    ];
    prop_oneof![
        6 => (0usize..3, 0u64..400_000).prop_map(|(ws, max_micros)| Step::Reserve { ws, max_micros }),
        4 => (0usize..3, 0u64..500_000).prop_map(|(ws, cost_micros)| Step::Settle { ws, cost_micros }),
        1 => (0usize..3, failure.clone()).prop_map(|(ws, failure)| Step::FailWorkspace { ws, failure }),
        1 => (0usize..3, proptest::option::of(0usize..2)).prop_map(|(ws, more)| Step::LimitWorkspace { ws, more }),
        1 => failure.prop_map(Step::FailDeployment),
    ]
}

/// Runs `steps` over three workspaces sharing one deployment meter with `cap`, and returns the
/// deployment's records and each workspace's.
fn run(steps: &[Step], cap: u64) -> (Vec<MeterRecord>, Vec<Vec<MeterRecord>>) {
    let shared = SharedMeter::new();
    let mut meters: Vec<Meter<MemoryMeter>> = (0..3u128)
        .map(|i| {
            Meter::new(
                MemoryMeter::new(),
                Caps::new(usd("0.8")),
                DeploymentMeter::new(
                    Box::new(shared.clone()),
                    Usd6::from_micros(cap),
                    Box::new(SequentialIds::starting_at((1 << 100) + (i << 40))),
                ),
            )
        })
        .collect();
    let mut open: Vec<Vec<Reservation>> = vec![Vec::new(); 3];
    let mut n = 0u32;
    for step in steps {
        match step {
            Step::Reserve { ws, max_micros } => {
                n += 1;
                let mut r = reservation(u8::try_from(*ws).unwrap(), n, "0");
                r.max_cost = Usd6::from_micros(*max_micros);
                if let Ok(r) = meters[*ws].reserve(r) {
                    open[*ws].push(r);
                }
            }
            Step::Settle { ws, cost_micros } => {
                if let Some(r) = open[*ws].pop() {
                    n += 1;
                    let s = Settlement {
                        event_id: encode_ulid((1 << 113) + u128::from(n)),
                        call_id: r.call_id.clone(),
                        reservation: r.event_id.clone(),
                        outcome: Outcome::Ok,
                        attempts: 1,
                        tokens: Usage::default(),
                        cost: Usd6::from_micros((*cost_micros).min(r.max_cost.micros())),
                    };
                    let _ = meters[*ws].settle(s);
                }
            }
            Step::FailWorkspace { ws, failure } => meters[*ws].store_mut().fail_with(*failure),
            Step::LimitWorkspace { ws, more } => {
                let now = meters[*ws].store().records().len();
                meters[*ws].store_mut().limit_appends(more.map(|m| now + m));
            }
            Step::FailDeployment(failure) => shared.fail_with(*failure),
        }
    }
    shared.fail_with(None);
    let workspaces = meters
        .iter_mut()
        .map(|m| {
            m.store_mut().fail_with(None);
            m.store().read().unwrap()
        })
        .collect();
    (shared.records(), workspaces)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn the_deployment_never_passes_its_cap_and_never_counts_a_call_below_a_workspace(
        steps in proptest::collection::vec(step(), 1..60),
        cap in 0u64..1_200_000,
    ) {
        let (deployment, workspaces) = run(&steps, cap);
        prop_assert_eq!(check(&deployment, &workspaces, i128::from(cap)), Ok(()));
    }
}

#[test]
fn the_oracle_catches_each_seeded_bug() {
    let steps: Vec<Step> = (0..3)
        .flat_map(|ws| {
            [
                Step::Reserve {
                    ws,
                    max_micros: 300_000,
                },
                Step::Settle {
                    ws,
                    cost_micros: 100_000,
                },
                Step::Reserve {
                    ws,
                    max_micros: 200_000,
                },
            ]
        })
        .collect();
    let cap = 1_000_000;
    let (deployment, workspaces) = run(&steps, cap);
    assert_eq!(check(&deployment, &workspaces, i128::from(cap)), Ok(()));
    let mut over = deployment.clone();
    let mut extra = reservation(9, 1, "0.9");
    extra.event_id = encode_ulid(1 << 120);
    over.push(MeterRecord::Reserved(extra));
    assert!(
        check(&over, &workspaces, i128::from(cap)).is_err(),
        "a reservation past the cap"
    );
    let dropped: Vec<MeterRecord> = deployment
        .iter()
        .filter(|r| !matches!(r, MeterRecord::Reserved(x) if x.call_id == workspaces[1].first().map(|w| match w { MeterRecord::Reserved(y) => y.call_id.clone(), MeterRecord::Settled(y) => y.call_id.clone() }).unwrap()))
        .cloned()
        .collect();
    assert!(
        check(&dropped, &workspaces, i128::from(cap)).is_err(),
        "a copy that never reached the deployment"
    );
    let settled_low: Vec<MeterRecord> = deployment
        .iter()
        .map(|r| match r {
            MeterRecord::Settled(s) => MeterRecord::Settled(Settlement {
                cost: Usd6::ZERO,
                ..s.clone()
            }),
            other => other.clone(),
        })
        .collect();
    assert!(
        check(&settled_low, &workspaces, i128::from(cap)).is_err(),
        "a copy settled below the workspace's"
    );
    let mut unlinked = deployment.clone();
    if let Some(MeterRecord::Settled(s)) = unlinked
        .iter_mut()
        .find(|r| matches!(r, MeterRecord::Settled(_)))
    {
        s.reservation = encode_ulid(1 << 121);
    }
    assert!(
        check(&unlinked, &workspaces, i128::from(cap)).is_err(),
        "a settlement copy naming no copy"
    );
}

#[test]
fn a_settlement_copy_names_the_copy_of_its_own_calls_reservation() {
    let shared = SharedMeter::new();
    let mut a = workspace_meter(&shared, "5", 1 << 100);
    let first = a.reserve(reservation(1, 1, "0.4")).unwrap();
    let second = a.reserve(reservation(1, 2, "0.6")).unwrap();
    a.settle(settlement(&second, "0.1", 9)).unwrap();
    let records = shared.records();
    let copy_of = |call: &str| {
        records.iter().find_map(|r| match r {
            MeterRecord::Reserved(x) if x.call_id == call => Some(x.event_id.clone()),
            _ => None,
        })
    };
    let Some(MeterRecord::Settled(settled)) = records.last() else {
        panic!("the settlement's copy last");
    };
    assert_eq!(Some(settled.reservation.clone()), copy_of(&second.call_id));
    assert_ne!(Some(settled.reservation.clone()), copy_of(&first.call_id));
    assert_eq!(a.deployment().spend(OCTOBER), Ok(usd("0.5")));
}

#[test]
fn the_deployment_meter_shows_its_cap_and_never_its_store() {
    let shared = SharedMeter::new();
    let a = workspace_meter(&shared, "50", 1 << 100);
    assert_eq!(a.deployment().monthly(), usd("50"));
    let shown = format!("{:?}", a.deployment());
    assert!(shown.starts_with("DeploymentMeter"), "{shown}");
    assert!(shown.contains("monthly"), "{shown}");
}

#[test]
fn the_deployment_cap_counts_its_month_alone_and_every_key_owner() {
    let shared = SharedMeter::new();
    let mut a = workspace_meter(&shared, "1", 1 << 100);
    let mut september = reservation(1, 1, "0.9");
    september.risk_day = Date::new(2026, 9, 30).unwrap();
    assert!(a.reserve(september).is_ok());
    let mut customer = reservation(1, 2, "0.6");
    customer.key_owner = KeyOwner::Customer;
    assert!(
        a.reserve(customer).is_ok(),
        "September's spend does not count in October"
    );
    assert_eq!(
        a.reserve(reservation(1, 3, "0.5")),
        Err(MeterRefusal::BudgetExhausted),
        "a customer's own key counts toward the deployment's cap too (DEC-2204 item 3)"
    );
    assert!(a.reserve(reservation(1, 4, "0.4")).is_ok());
}

#[test]
fn an_orphan_is_counted_for_the_operator() {
    let shared = SharedMeter::new();
    let (registry, model) = registry();
    let mut store = MemoryMeter::new();
    store.limit_appends(Some(0));
    let mut gateway = mandate_gateway::Gateway::new(
        settings(hosted_policy()),
        registry,
        Box::new(mandate_gateway::openrouter::OpenRouter),
        mandate_gateway::fake::RecordedTransport::new(),
        Box::new(TestKey),
        Meter::new(
            store,
            caps(),
            DeploymentMeter::new(
                Box::new(shared.clone()),
                usd("50"),
                Box::new(SequentialIds::starting_at(1 << 100)),
            ),
        ),
        mandate_gateway::provider::ManualClock::at(at(START)),
        Box::new(SequentialIds::starting_at(1 << 80)),
    );
    gateway.transport_mut().script(vec![ok_reply(10)]);
    assert_eq!(gateway.orphans(), 0, "no orphan before the call");
    let record = gateway.call(&request(&model, 1)).unwrap();
    assert_eq!(record.outcome, Outcome::MeterUnavailable);
    assert_eq!(record.reservation, None);
    assert!(gateway.transport().sent().is_empty(), "nothing is sent");
    assert_eq!(
        gateway.orphans(),
        1,
        "the orphan is counted, for the operator's alert"
    );
    assert_eq!(
        shared.records().len(),
        1,
        "and stays open on the deployment's stream"
    );
    let mut clean = workspace_meter(&SharedMeter::new(), "1", 1 << 101);
    clean.store_mut().fail_with(Some(StoreError::Unavailable));
    assert_eq!(
        clean.reserve(reservation(2, 1, "0.1")),
        Err(MeterRefusal::Unavailable),
        "a workspace that cannot be read leaves no orphan"
    );
    let mut orphaning = workspace_meter(&SharedMeter::new(), "1", 1 << 102);
    orphaning.store_mut().limit_appends(Some(0));
    assert_eq!(
        orphaning.reserve(reservation(3, 1, "0.1")),
        Err(MeterRefusal::Orphaned)
    );
}
