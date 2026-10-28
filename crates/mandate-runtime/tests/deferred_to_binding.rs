//! The runtime's half of `DecisionMade` version 3 (journal spec v0.85 rules 269 and 270; DEC-2388
//! items 10 and 12): an opening the dry run allows with checks deferred to the executor's binding
//! gate is journaled naming each of them, in byte order, and the owner is asked, never proposed to
//! the executor unasked, so no check the dry run could not see is ever reported passed (DEC-2378
//! item 11 conditions 1 and 4). An exit defers nothing (`AGENTS.md` rule 13).
//!
//! The journal's own `Draft::parse` is the oracle for the schema and rules 269 and 270: every
//! decision the runtime drafts must parse at the version the test computes on its own, from the
//! verdict and the purpose, never from the runtime's payload.

mod common;

use std::collections::BTreeSet;

use common::escalation::{ASKED_AT, Answer, asking_shell, next_control_seq, reconciliation, tail};
use common::{
    AGENT_STREAM, AllowGate, FixedPlan, Ran, Shell, TestIds, clock, fresh_output, ports, price,
    qty, universe,
};
use mandate_canon::{Value, to_canonical};
use mandate_journal::{Draft, check_batch};
use mandate_num::Usd;
use mandate_runtime::{
    Autonomy, Deferral, DryRunVerdict, EventDraft, GateDryRun, Input, LiquidityRead, MandateView,
    Ports, Proposal,
};
use mandate_time::UtcNanos;
use proptest::prelude::*;

/// The liquidity floor's and participation's names, which a version 3 carries exactly when no
/// figures were evaluated (rule 270).
const LIQUIDITY_PARTS: [&str; 2] = ["liquidity_floor", "participation"];

/// A dry run answering every proposal with one fixed verdict.
struct FixedGate(DryRunVerdict);

impl GateDryRun for FixedGate {
    fn check(&self, _proposal: &Proposal) -> DryRunVerdict {
        self.0.clone()
    }
}

fn read() -> LiquidityRead {
    LiquidityRead {
        prior_close: price("150.5"),
        median_dollar_volume_20d: Usd::parse("2500000000").unwrap(),
        adv_20d: qty("17000000"),
        trailing_5m_volume: qty("41000"),
        read_at: UtcNanos::parse("2026-09-21T14:00:00.000000123Z").unwrap(),
    }
}

fn deferring(deferred: &[Deferral], liquidity: Option<LiquidityRead>) -> DryRunVerdict {
    DryRunVerdict::AllowDeferring {
        deferred: deferred.iter().copied().collect(),
        liquidity,
    }
}

/// An exit as the builder classifies one: risk reducing, never asked (mandate spec §6.2 step 3).
fn exiting(autonomy: Autonomy) -> FixedPlan {
    FixedPlan {
        decided_by: "builtin_risk_reducing",
        ..FixedPlan::exiting(autonomy)
    }
}

/// One started shell's evaluation of `plan` under `gate`.
fn evaluated(gate: &dyn GateDryRun, plan: &FixedPlan) -> Ran {
    let (ids, view) = (TestIds, versioned(universe(&["AAPL"])));
    let ports = ports(&ids, gate, plan, &view);
    let mut shell = Shell::new(1);
    shell
        .fold_one(&reconciliation(1, 0))
        .unwrap_or_else(|e| panic!("reconciliation: {e}"));
    let (mut shell, _) = shell.restart(&ports);
    shell.run(Input::ModelOutput(fresh_output(100)), &ports);
    shell.run(Input::Tick(clock(100)), &ports)
}

fn decision(ran: &Ran) -> &EventDraft {
    ran.drafts
        .iter()
        .find(|draft| draft.event_type == "DecisionMade")
        .unwrap_or_else(|| panic!("the evaluation decided: {:?}", ran.draft_types()))
}

fn member<'a>(draft: &'a EventDraft, key: &str) -> Option<&'a Value> {
    draft.payload.get(key)
}

fn names(draft: &EventDraft) -> Option<Vec<&str>> {
    member(draft, "deferred_to_binding")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
}

/// The mandate version every draft here names, a digest as the journal's references need.
fn mandate_version() -> String {
    format!("sha256:{}", "4".repeat(64))
}

fn versioned(mut view: MandateView) -> MandateView {
    view.version = mandate_version();
    view
}

fn digest_refs(value: &Value, refs: &mut BTreeSet<String>) {
    match value {
        Value::Str(text)
            if text
                .strip_prefix("sha256:")
                .is_some_and(|hex| hex.len() == 64) =>
        {
            refs.insert(text.clone());
        }
        Value::Array(items) => items.iter().for_each(|item| digest_refs(item, refs)),
        Value::Object(members) => members
            .values()
            .for_each(|member| digest_refs(member, refs)),
        Value::Null | Value::Bool(_) | Value::Int(_) | Value::Str(_) => {}
    }
}

/// Every draft of `ran` as the journal reads it, each at its own `schema_version` and with its
/// causation, then the batch as `append` judges it: the batch rules 10 and 292 included.
fn committed(ran: &Ran) -> Result<(), String> {
    let drafts = ran
        .drafts
        .iter()
        .map(|draft| {
            let mut refs = BTreeSet::new();
            digest_refs(&draft.payload, &mut refs);
            let artifacts = refs.into_iter().map(|r| format!("\"{r}\"")).collect::<Vec<_>>().join(",");
            let causation = draft
                .causation_id
                .as_ref()
                .map_or_else(|| "null".to_owned(), |id| format!("\"{}\"", id.0));
            let body = format!(
                r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"{AGENT_STREAM}","event_type":"{}","schema_version":{},"event_time":"2026-09-21T14:00:00.000000000Z","clock_source":"local","causation_id":{causation},"correlation_id":null,"actor":{{"kind":"agent","id":"a","version":"0.1.0","build":"sha256:{}"}},"config_refs":{{"mandate_version":"{}"}},"payload":{},"artifact_refs":[{artifacts}],"pii_refs":[]}}"#,
                draft.event_id.0,
                draft.event_type,
                draft.schema_version(),
                "3".repeat(64),
                mandate_version(),
                String::from_utf8_lossy(&to_canonical(&draft.payload)),
            );
            Draft::parse(body.as_bytes()).map_err(|e| format!("{}: {e}", draft.event_type))
        })
        .collect::<Result<Vec<_>, _>>()?;
    check_batch(&drafts).map_err(|(i, e)| format!("draft {i}: {e}"))
}

fn request(ran: &Ran) -> Option<&EventDraft> {
    ran.drafts
        .iter()
        .find(|draft| draft.event_type == "ApprovalRequested")
}

/// The draft as the journal reads it at `schema_version`, under version 1's references.
fn parsed(draft: &EventDraft, schema_version: u64) -> Result<u64, String> {
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"{AGENT_STREAM}","event_type":"{}","schema_version":{schema_version},"event_time":"2026-09-21T14:00:00.000000000Z","clock_source":"local","causation_id":null,"correlation_id":null,"actor":{{"kind":"agent","id":"a","version":"0.1.0","build":"sha256:{}"}},"config_refs":{{"mandate_version":"sha256:{}"}},"payload":{},"artifact_refs":[],"pii_refs":[]}}"#,
        draft.event_id.0,
        draft.event_type,
        "3".repeat(64),
        "4".repeat(64),
        String::from_utf8_lossy(&to_canonical(&draft.payload)),
    );
    Draft::parse(body.as_bytes())
        .map(|draft| draft.schema_version())
        .map_err(|error| {
            format!(
                "{}: {error}",
                String::from_utf8_lossy(&to_canonical(&draft.payload))
            )
        })
}

fn proposed(ran: &Ran) -> bool {
    ran.draft_types().contains(&"IntentProposed") || !ran.handed.is_empty()
}

fn asked(ran: &Ran) -> bool {
    ran.draft_types().contains(&"ApprovalRequested")
}

#[test]
fn an_allowed_opening_names_every_deferral_in_byte_order_and_its_unread_liquidity() {
    let gate = FixedGate(deferring(
        &[
            Deferral::RelatedAccounts,
            Deferral::AgentMode,
            Deferral::Eligibility,
        ],
        None,
    ));
    let ran = evaluated(&gate, &FixedPlan::opening(Autonomy::Ask));
    let made = decision(&ran);
    assert_eq!(
        names(made),
        Some(vec![
            "agent_mode",
            "eligibility",
            "liquidity_floor",
            "participation",
            "related_accounts"
        ]),
        "the deferrals and the two unread liquidity checks, by bytes (rule 269)"
    );
    assert_eq!(member(made, "liquidity"), Some(&Value::Null));
    assert_eq!(
        member(made, "dry_run").and_then(Value::as_str),
        Some("allow")
    );
    assert_eq!(parsed(made, 3), Ok(3));
    assert!(asked(&ran) && !proposed(&ran), "{:?}", ran.draft_types());
}

#[test]
fn figures_read_leave_the_liquidity_checks_evaluated_and_are_recorded() {
    let gate = FixedGate(deferring(&[Deferral::QuoteAsk], Some(read())));
    let ran = evaluated(&gate, &FixedPlan::opening(Autonomy::Ask));
    let made = decision(&ran);
    assert_eq!(names(made), Some(vec!["quote_ask"]));
    let liquidity = member(made, "liquidity").unwrap();
    let field = |key: &str| liquidity.get(key).and_then(Value::as_str);
    assert_eq!(field("prior_close"), Some("150.5"));
    assert_eq!(field("median_dollar_volume_20d"), Some("2500000000"));
    assert_eq!(field("adv_20d"), Some("17000000"));
    assert_eq!(field("trailing_5m_volume"), Some("41000"));
    assert_eq!(field("read_at"), Some("2026-09-21T14:00:00.000000123Z"));
    assert_eq!(parsed(made, 3), Ok(3));
}

#[test]
fn an_auto_opening_with_a_deferral_is_asked_under_the_policy_overlay_never_proposed() {
    let gate = FixedGate(deferring(&[Deferral::Halt], Some(read())));
    let ran = evaluated(&gate, &FixedPlan::opening(Autonomy::Auto));
    let made = decision(&ran);
    assert_eq!(
        member(made, "autonomy").and_then(Value::as_str),
        Some("ask")
    );
    assert_eq!(
        member(made, "decided_by").and_then(Value::as_str),
        Some("policy_overlay")
    );
    assert!(asked(&ran) && !proposed(&ran), "{:?}", ran.draft_types());
    assert_eq!(parsed(made, 3), Ok(3));

    let ran = evaluated(&AllowGate, &FixedPlan::opening(Autonomy::Auto));
    assert!(
        proposed(&ran) && !asked(&ran),
        "the same opening with nothing deferred is proposed: {:?}",
        ran.draft_types()
    );
}

#[test]
fn an_asked_or_denied_classification_keeps_its_own_label() {
    let gate = FixedGate(deferring(&[Deferral::Halt], None));
    let plan = FixedPlan::opening(Autonomy::Ask);
    let ran = evaluated(&gate, &plan);
    assert_eq!(
        member(decision(&ran), "decided_by").and_then(Value::as_str),
        Some(plan.decided_by)
    );
    let ran = evaluated(&gate, &FixedPlan::opening(Autonomy::Deny));
    assert_eq!(
        member(decision(&ran), "autonomy").and_then(Value::as_str),
        Some("deny")
    );
    assert!(!asked(&ran) && !proposed(&ran), "{:?}", ran.draft_types());
}

#[test]
fn an_exit_defers_nothing_and_is_written_at_version_1() {
    for verdict in [
        deferring(&[Deferral::Halt], None),
        deferring(&[], Some(read())),
    ] {
        let ran = evaluated(&FixedGate(verdict), &exiting(Autonomy::Auto));
        let made = decision(&ran);
        assert_eq!(member(made, "deferred_to_binding"), None);
        assert_eq!(member(made, "liquidity"), None);
        assert_eq!(parsed(made, 1), Ok(1));
        assert!(proposed(&ran), "{:?}", ran.draft_types());
    }
    let ran = evaluated(
        &FixedGate(DryRunVerdict::DenyOnFigures {
            reason_code: "participation_cap".to_owned(),
            liquidity: read(),
        }),
        &exiting(Autonomy::Auto),
    );
    assert_eq!(member(decision(&ran), "liquidity"), None);
    assert_eq!(parsed(decision(&ran), 1), Ok(1));
}

#[test]
fn an_opening_with_nothing_deferred_on_its_figures_is_an_allow_at_version_1() {
    let ran = evaluated(
        &FixedGate(deferring(&[], Some(read()))),
        &FixedPlan::opening(Autonomy::Auto),
    );
    let made = decision(&ran);
    assert_eq!(member(made, "deferred_to_binding"), None);
    assert_eq!(parsed(made, 1), Ok(1));
    assert!(proposed(&ran), "{:?}", ran.draft_types());
}

#[test]
fn an_opening_denied_on_its_figures_records_them_and_defers_nothing() {
    let ran = evaluated(
        &FixedGate(DryRunVerdict::DenyOnFigures {
            reason_code: "liquidity_floor".to_owned(),
            liquidity: read(),
        }),
        &FixedPlan::opening(Autonomy::Auto),
    );
    let made = decision(&ran);
    assert_eq!(names(made), Some(Vec::new()));
    assert_eq!(
        member(made, "reason_code").and_then(Value::as_str),
        Some("liquidity_floor")
    );
    assert!(member(made, "liquidity").is_some_and(|l| l != &Value::Null));
    assert_eq!(parsed(made, 3), Ok(3));
    assert!(!asked(&ran) && !proposed(&ran), "{:?}", ran.draft_types());
    assert!(ran.notifications.contains(&"decision_skipped"));
}

/// DEC-2810: a deferring opening the builder produced no plan for has no request version, so it
/// is denied `plan_required` by the runtime itself, journaled at version 1 and notified as any
/// skipped decision, and nothing is asked, proposed, or left pending to repeat unseen; the same
/// opening with nothing deferred is asked at version 1.
#[test]
fn a_deferring_opening_without_a_plan_is_denied_plan_required_and_notified() {
    let mut plan = FixedPlan::opening(Autonomy::Auto);
    if let Some(proposal) = plan.proposal.as_mut() {
        proposal.execution = None;
    }
    let ran = evaluated(&FixedGate(deferring(&[Deferral::Halt], None)), &plan);
    let made = decision(&ran);
    assert_eq!(
        member(made, "dry_run").and_then(Value::as_str),
        Some("deny")
    );
    assert_eq!(
        member(made, "reason_code").and_then(Value::as_str),
        Some("plan_required")
    );
    assert_eq!(
        made.schema_version(),
        1,
        "a denial before any figures defers nothing"
    );
    assert!(!asked(&ran) && !proposed(&ran), "{:?}", ran.draft_types());
    assert!(
        ran.notifications.contains(&"decision_skipped"),
        "{:?}",
        ran.notifications
    );
    assert_eq!(committed(&ran), Ok(()));
    let ran = evaluated(&FixedGate(deferring(&[], Some(read()))), &plan);
    assert!(
        proposed(&ran),
        "nothing deferred, the plan is not needed: {:?}",
        ran.draft_types()
    );
    let ask = FixedPlan {
        autonomy: Autonomy::Ask,
        ..plan
    };
    let ran = evaluated(&AllowGate, &ask);
    assert_eq!(request(&ran).map(EventDraft::schema_version), Some(1));
    assert_eq!(committed(&ran), Ok(()));
}

/// A dry run that allows until told to defer: the gate's answer at the ask, then at re-validation.
struct TurningGate(std::cell::Cell<bool>);

impl GateDryRun for TurningGate {
    fn check(&self, _proposal: &Proposal) -> DryRunVerdict {
        if self.0.get() {
            deferring(&[Deferral::Halt], None)
        } else {
            DryRunVerdict::Allow
        }
    }
}

/// DEC-2810 item 3: a version 1 request, which binds no plan, granted while the dry run now defers
/// a check, is skipped `plan_required` and hands nothing; a version 2 request granted under the
/// same answer acts, its plan bound.
#[test]
fn a_grant_without_a_plan_that_now_defers_is_skipped_plan_required() {
    for (planned, acts) in [(false, false), (true, true)] {
        let (ids, view) = (TestIds, universe(&["AAPL"]));
        let gate = TurningGate(std::cell::Cell::new(false));
        let mut plan = FixedPlan::opening(Autonomy::Ask);
        if !planned && let Some(proposal) = plan.proposal.as_mut() {
            proposal.execution = None;
        }
        let ports: Ports<'_> = ports(&ids, &gate, &plan, &view);
        let (mut shell, asked) = asking_shell(&ports, Some("155"));
        assert_eq!(asked.request.schema_version(), if planned { 2 } else { 1 });
        gate.0.set(true);
        let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 20).event();
        let ran = tail(&mut shell, &grant, &ports);
        let revalidated = ran
            .drafts
            .iter()
            .find(|draft| draft.event_type == "ApprovalRevalidated")
            .unwrap_or_else(|| panic!("the grant is re-validated: {:?}", ran.draft_types()));
        let field = |key: &str| member(revalidated, key).and_then(Value::as_str);
        if acts {
            assert_eq!(field("result"), Some("act"), "planned");
            assert_eq!(ran.handed.len(), 1);
        } else {
            assert_eq!(field("result"), Some("skip"), "unplanned");
            assert_eq!(field("dry_run_reason"), Some("plan_required"));
            assert!(ran.handed.is_empty(), "{:?}", ran.draft_types());
        }
    }
}

/// A granted ask whose dry run still defers at re-validation acts: the executor's binding gate
/// evaluates every deferred check (DEC-2378 item 11 condition 3).
#[test]
fn a_granted_ask_still_deferring_at_revalidation_is_handed_to_the_binding_gate() {
    let (ids, view) = (TestIds, universe(&["AAPL"]));
    let gate = FixedGate(deferring(&[Deferral::Eligibility], None));
    let plan = FixedPlan::opening(Autonomy::Auto);
    let ports: Ports<'_> = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some("155"));
    let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 20).event();
    let ran = tail(&mut shell, &grant, &ports);
    assert_eq!(ran.handed.len(), 1, "{:?}", ran.draft_types());
    let revalidated = ran
        .drafts
        .iter()
        .find(|draft| draft.event_type == "ApprovalRevalidated")
        .unwrap_or_else(|| panic!("the grant is re-validated: {:?}", ran.draft_types()));
    for (key, want) in [
        ("result", "act"),
        ("decided_by_bound", "policy_overlay"),
        ("decided_by_now", "policy_overlay"),
    ] {
        assert_eq!(
            member(revalidated, key).and_then(Value::as_str),
            Some(want),
            "re-validation applies the overlay the request bound (check 10): {key}"
        );
    }
}

/// The names version 3 must carry for `verdict` on a proposal adding risk or not, computed from
/// the verdict alone: `None` when the decision is written at version 1.
fn expected(verdict: &DryRunVerdict, adds_risk: bool) -> Option<Vec<String>> {
    if !adds_risk {
        return None;
    }
    match verdict {
        DryRunVerdict::AllowDeferring {
            deferred,
            liquidity,
        } => {
            let mut all: BTreeSet<String> = deferred.iter().map(|d| d.name().to_owned()).collect();
            if liquidity.is_none() {
                all.extend(LIQUIDITY_PARTS.map(str::to_owned));
            }
            if all.is_empty() {
                return None;
            }
            let mut sorted: Vec<String> = all.into_iter().collect();
            sorted.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            Some(sorted)
        }
        DryRunVerdict::DenyOnFigures { .. } => Some(Vec::new()),
        DryRunVerdict::Allow | DryRunVerdict::Deny { .. } => None,
    }
}

fn verdicts() -> impl Strategy<Value = DryRunVerdict> {
    let parts = proptest::sample::subsequence(Deferral::ALL.to_vec(), 0..=Deferral::ALL.len());
    prop_oneof![
        Just(DryRunVerdict::Allow),
        Just(DryRunVerdict::Deny {
            reason_code: "max_order_usd".to_owned()
        }),
        Just(DryRunVerdict::DenyOnFigures {
            reason_code: "liquidity_floor".to_owned(),
            liquidity: read(),
        }),
        (parts, any::<bool>()).prop_map(|(parts, figures)| DryRunVerdict::AllowDeferring {
            deferred: parts.into_iter().collect(),
            liquidity: figures.then(read),
        }),
    ]
}

fn autonomies() -> impl Strategy<Value = Autonomy> {
    prop_oneof![
        Just(Autonomy::Auto),
        Just(Autonomy::Ask),
        Just(Autonomy::Deny)
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Every decision parses at the version the verdict implies, names exactly the deferrals the
    /// oracle computes, and an opening with any deferral is never proposed unasked.
    #[test]
    fn every_decision_parses_at_its_version_and_no_deferred_opening_is_proposed_unasked(
        verdict in verdicts(),
        autonomy in autonomies(),
        opening in any::<bool>(),
        planned in any::<bool>(),
    ) {
        let mut plan = if opening { FixedPlan::opening(autonomy) } else { exiting(Autonomy::Auto) };
        if !planned && let Some(proposal) = plan.proposal.as_mut() {
            proposal.execution = None;
        }
        let ran = evaluated(&FixedGate(verdict.clone()), &plan);
        let made = decision(&ran);
        let deferring_allow = matches!(verdict, DryRunVerdict::AllowDeferring { .. })
            && expected(&verdict, opening).is_some_and(|names| !names.is_empty());
        let refused = deferring_allow && !planned;
        prop_assert_eq!(
            member(made, "reason_code").and_then(Value::as_str) == Some("plan_required"),
            refused,
            "DEC-2810: exactly a deferring allow without a plan is refused plan_required"
        );
        let want = if refused { None } else { expected(&verdict, opening) };
        let version = if want.is_some() { 3 } else { 1 };
        prop_assert_eq!(parsed(made, version), Ok(version));
        prop_assert_eq!(made.schema_version(), version, "the draft names the version it parses at");
        let got = names(made).map(|names| names.into_iter().map(str::to_owned).collect::<Vec<_>>());
        prop_assert_eq!(&got, &want);
        let deferring = want.as_ref().is_some_and(|names| !names.is_empty());
        if deferring {
            prop_assert!(!proposed(&ran), "{:?}", ran.draft_types());
            prop_assert_eq!(asked(&ran), autonomy != Autonomy::Deny);
        }
        if let Some(asked) = request(&ran) {
            let named = asked
                .payload
                .get("content")
                .and_then(|content| content.get("deferred_to_binding"))
                .and_then(Value::as_array)
                .map(|names| names.iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>());
            prop_assert_eq!(&named, &want.clone().filter(|names| !names.is_empty()), "the request names the decision's deferrals");
            prop_assert_eq!(asked.schema_version(), if deferring { 3 } else if planned { 2 } else { 1 });
        }
        prop_assert_eq!(committed(&ran), Ok(()), "the journal accepts the batch the runtime writes");
        let overlay = deferring && matches!(verdict, DryRunVerdict::AllowDeferring { .. }) && autonomy == Autonomy::Auto;
        prop_assert_eq!(
            member(made, "decided_by").and_then(Value::as_str) == Some("policy_overlay"),
            overlay
        );
    }
}
