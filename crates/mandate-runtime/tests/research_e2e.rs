//! The research loop end to end on recorded fixtures (agent harness spec §6.2, E19-2, E19-3,
//! E17-2): recorded bars and news, retrieved at a cut-off and journaled; one recorded model
//! response; a thesis checked, filled with platform facts, and admitted by `mandate-research`'s
//! ordered checks; the executor's copy of the admission; and the trading loop's next tick sizing
//! the thesis through the real `conviction_linear` builder, the gate's dry run, and the autonomy
//! rules, to an ASK raised by the admission ceiling.

mod common;

use common::research::{
    AAPL, BuilderPlan, CUT_OFF, FIXTURE_KEY, FixtureVault, HostedDouble, Loop, MAX_ORDER_USD, MSFT,
    RESPONSE_AAPL, RESPONSE_INJECTION, RecordedPlane, UPSTREAM, World, copy_admissions, demo_parts,
    full_run, instant, recorded, scripted, view_of, worker_run_on,
};
use common::{ACCOUNT_STREAM, AllowGate, TestIds, clock, event, ports, text, with_clock};
use mandate_canon::Value;
use mandate_gateway::provider::NoKeys;
use mandate_num::{Price, Qty, Usd};
use mandate_research_run::{Backend, NoHostedTransport, Reason, research_gateway};
use mandate_runtime::{Effect, Input, Mode, WriterEpoch};

fn boot(lp: &mut Loop, ports: &mandate_runtime::Ports<'_>) {
    lp.shell.run(Input::Started(WriterEpoch(1)), ports);
    lp.shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            1,
            "ReconciliationRun",
            with_clock(&[("result", text("clean"))], CUT_OFF - 120),
        ))
        .unwrap_or_else(|e| panic!("{e}"));
    lp.shell.run(Input::Tick(clock(CUT_OFF - 60)), ports);
    assert_eq!(lp.shell.state.effective_mode(), Mode::Normal);
}

fn member<'a>(value: &'a Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{key} is text"))
}

fn notional(payload: &Value) -> Usd {
    let qty = Qty::parse(member(payload, "qty")).unwrap_or_else(|e| panic!("{e}"));
    let limit = Price::parse(member(payload, "limit_price")).unwrap_or_else(|e| panic!("{e}"));
    qty.notional(limit).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn recorded_bars_and_news_become_an_asked_opening_inside_the_envelope() {
    let ids = TestIds;
    let silent = common::FixedPlan::silent();
    let empty = common::universe(&[]);
    let research_ports = ports(&ids, &AllowGate, &silent, &empty);
    let mut lp = Loop::new(1);
    boot(&mut lp, &research_ports);
    let mut world = World::open();
    let envelope_before = world.mandate.clone();
    let overlay_before = world.overlay.clone();
    let plane = RecordedPlane::load();
    let gateway = recorded(RESPONSE_AAPL);

    full_run(
        &mut lp,
        clock(CUT_OFF),
        &research_ports,
        &world,
        &plane,
        &gateway,
    );

    let observed = lp.events("ObservationRecorded");
    let call_record = &lp.events("ModelInvocationRecorded")[0];
    let context = call_record
        .payload
        .get("observations")
        .and_then(Value::as_array)
        .unwrap_or_default();
    assert_eq!(
        context.len(),
        observed.len(),
        "the call record cites every observation"
    );
    assert_eq!(member(&call_record.payload, "outcome"), "ok");
    let thesis = &lp.events("ThesisProposed")[0].payload;
    assert_eq!(member(thesis, "instrument_id"), AAPL);
    assert_eq!(thesis.get("admitted"), Some(&Value::Bool(true)));

    copy_admissions(&lp, &mut world);
    let view = view_of(&world);
    let builder = BuilderPlan::new();
    let trading_ports = ports(&ids, &AllowGate, &builder, &view);
    let ran = lp
        .shell
        .run(Input::Tick(clock(CUT_OFF + 60)), &trading_ports);

    let types = ran.draft_types();
    assert_eq!(
        types,
        vec!["DecisionMade", "ApprovalRequested", "ApprovalDelivered"],
        "the first order in a newly admitted instrument is asked"
    );
    let decision = &ran.drafts[0].payload;
    assert_eq!(member(decision, "dry_run"), "allow");
    assert_eq!(member(decision, "autonomy"), "ask");
    assert_eq!(member(decision, "decided_by"), "admission_ceiling");
    assert_eq!(member(decision, "instrument_id"), AAPL);
    assert_eq!(member(decision, "side"), "buy");
    assert_eq!(member(decision, "purpose"), "open");
    let used = decision
        .get("outputs_used")
        .and_then(Value::as_array)
        .unwrap_or_default();
    let output_event = &lp.events("ModelOutputRecorded")[0].event_id.0;
    assert_eq!(
        used,
        &[Value::Str(output_event.clone())],
        "the decision cites the thesis's output"
    );
    let limit = Usd::parse(MAX_ORDER_USD).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        notional(decision) <= limit,
        "the builder clips to max_order_usd"
    );
    assert!(
        ran.handed.is_empty(),
        "an ASK hands nothing to the executor"
    );
    assert!(
        ran.effects
            .iter()
            .any(|effect| matches!(effect, Effect::NotifyApproval(_))),
        "the owner is asked"
    );
    assert_eq!(
        world.mandate, envelope_before,
        "HI-5: the envelope does not move"
    );
    assert_eq!(world.overlay, overlay_before);
}

#[test]
fn injected_news_yields_at_most_a_thesis_and_an_ask_inside_the_limits() {
    let ids = TestIds;
    let silent = common::FixedPlan::silent();
    let empty = common::universe(&[]);
    let research_ports = ports(&ids, &AllowGate, &silent, &empty);
    let mut lp = Loop::new(1);
    boot(&mut lp, &research_ports);
    let mut world = World::open();
    let envelope_before = world.mandate.clone();
    let plane = RecordedPlane::load();
    let (mut gateway, selection) = research_gateway(
        demo_parts(instant(CUT_OFF)),
        Box::new(NoKeys),
        None::<NoHostedTransport>,
        scripted(RESPONSE_INJECTION),
        UPSTREAM,
    );
    assert_eq!(selection.backend, Backend::RecordedFake);

    worker_run_on(
        &mut lp,
        clock(CUT_OFF),
        &research_ports,
        &world,
        &plane,
        &mut gateway,
        clock(CUT_OFF + 30),
    );

    let sent = gateway
        .transport()
        .recorded()
        .unwrap_or_else(|| panic!("the recorded fake"))
        .sent();
    assert_eq!(sent.len(), 1, "one call, through the fake");
    let records = lp.events("ModelInvocationRecorded");
    assert_eq!(records.len(), 1);
    let observed = lp.events("ObservationRecorded");
    assert!(
        observed
            .iter()
            .any(|event| member(&event.payload, "source") == "src.newswire"),
        "the injection fixture was retrieved and journaled, as untrusted data"
    );
    let records = lp.events("ThesisProposed");
    let msft = records
        .iter()
        .find(|record| member(&record.payload, "instrument_id") == MSFT)
        .unwrap_or_else(|| panic!("the injected thesis is journaled with its verdict"));
    assert_eq!(msft.payload.get("admitted"), Some(&Value::Bool(false)));
    assert_eq!(
        member(&msft.payload, "reason"),
        "no_corroboration",
        "one source cannot corroborate itself"
    );

    copy_admissions(&lp, &mut world);
    let view = view_of(&world);
    let builder = BuilderPlan::new();
    let trading_ports = ports(&ids, &AllowGate, &builder, &view);
    let ran = lp
        .shell
        .run(Input::Tick(clock(CUT_OFF + 60)), &trading_ports);
    let limit = Usd::parse(MAX_ORDER_USD).unwrap_or_else(|e| panic!("{e}"));
    for draft in &ran.drafts {
        if draft.event_type == "DecisionMade" {
            assert_ne!(member(&draft.payload, "instrument_id"), MSFT);
            assert!(
                notional(&draft.payload) <= limit,
                "HI-10: no limit is breached"
            );
            assert_eq!(member(&draft.payload, "autonomy"), "ask");
        }
        assert_ne!(
            draft.event_type, "IntentProposed",
            "nothing is proposed without the owner"
        );
    }
    assert!(ran.handed.is_empty());
    assert_eq!(
        world.mandate, envelope_before,
        "HI-5: the envelope does not move"
    );
}

/// The demo's config path (DEC-1120 item 5): the same run through the recorded fake when the vault
/// holds no key, and through the hosted transport when it holds one, with the same journal either
/// way and a selection that says which it used. The hosted transport here is a double with no
/// network; no test reaches a model provider.
#[test]
fn the_demo_runs_either_way_and_says_which_gateway_it_used() {
    let mut journals = Vec::new();
    for vaulted in [false, true] {
        let ids = TestIds;
        let silent = common::FixedPlan::silent();
        let empty = common::universe(&[]);
        let research_ports = ports(&ids, &AllowGate, &silent, &empty);
        let mut lp = Loop::new(1);
        boot(&mut lp, &research_ports);
        let world = World::open();
        let plane = RecordedPlane::load();
        let mut hosted = HostedDouble::default();
        hosted.inner = scripted(RESPONSE_AAPL);
        let (vault, recorded_reply): (Box<dyn mandate_gateway::provider::SecretSource>, _) =
            if vaulted {
                (
                    Box::new(FixtureVault),
                    mandate_gateway::fake::RecordedTransport::new(),
                )
            } else {
                (Box::new(NoKeys), scripted(RESPONSE_AAPL))
            };
        let (mut gateway, selection) = research_gateway(
            demo_parts(instant(CUT_OFF)),
            vault,
            Some(hosted),
            recorded_reply,
            UPSTREAM,
        );
        let line = selection.describe();
        if vaulted {
            assert_eq!(
                (selection.backend, selection.reason),
                (Backend::OpenRouter, Reason::VaultedKey)
            );
            assert!(line.contains("OpenRouter"), "{line}");
        } else {
            assert_eq!(
                (selection.backend, selection.reason),
                (Backend::RecordedFake, Reason::NoVaultedKey)
            );
            assert!(line.contains("recorded fake"), "{line}");
        }
        assert!(!line.contains(FIXTURE_KEY), "the line never names the key");
        worker_run_on(
            &mut lp,
            clock(CUT_OFF),
            &research_ports,
            &world,
            &plane,
            &mut gateway,
            clock(CUT_OFF + 30),
        );
        match gateway.transport() {
            mandate_research_run::ResearchTransport::Hosted(double) => {
                assert!(vaulted);
                assert_eq!(double.inner.sent().len(), 1, "the call went out hosted");
                assert_eq!(
                    double.keys,
                    vec![FIXTURE_KEY.to_owned()],
                    "with the vault's key, read per attempt"
                );
            }
            mandate_research_run::ResearchTransport::Recorded(fake) => {
                assert!(!vaulted);
                assert_eq!(fake.sent().len(), 1, "the call went to the fake");
            }
        }
        let thesis = &lp.events("ThesisProposed")[0].payload;
        assert_eq!(thesis.get("admitted"), Some(&Value::Bool(true)));
        journals.push(
            lp.shell
                .agent_journal
                .iter()
                .map(|event| (event.event_type.clone(), event.payload.clone()))
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(
        journals[0], journals[1],
        "the transport changes nothing the journal records"
    );
}
