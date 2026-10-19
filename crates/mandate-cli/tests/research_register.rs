//! DEC-2247 item 1: `model register <llm id> <version> --entry <research.json>` registers the
//! research agent's model from a recorded research entry. The content and hash are recomputed from
//! the entry, never supplied. Only an `llm.` model of that entry's id and version registers, always
//! with `admits_instruments: true` and no parameters. Registering changes no envelope field, and
//! confirming still runs V-007 and V-036. The oracle is the entry typed out here.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{CONTROL, FixedIds, Journal, at, owner, stream};
use mandate_canon::{Digest, parse, to_canonical};
use mandate_cli::config::{register_model, register_research_model};
use mandate_cli::control::{ControlError, ControlJournal, Owner, Submitted};
use mandate_cli::version::{confirm, create};
use mandate_gateway::registry::{Identity, Snapshot};
use mandate_journal::{ArtifactStore, Draft, Environment};
use mandate_research_run::{OutputBounds, ResearchEntry, RetrievalPlan};
use mandate_spec::context::{AgentId, ContextArgs, JournaledFact, Membership};
use mandate_spec::{Mandate, ValidationContext, Violation, validate::validate};

type Store = BTreeMap<Digest, Vec<u8>>;

const NOW: i64 = 1_790_000_000;
const RECORDING: &str = include_str!("../../../deploy/cell/demo/research/research.json");
const MANDATE: &str = include_str!("../../../deploy/cell/demo/research-mandate.json");
const MODEL: (&str, &str) = ("llm.research_agent", "0.1.0");
/// The demo entry's content hash, written out: the one `research-mandate.json` pins, so a change to
/// the pinned template, plan, bounds or schema, or to how the entry is read, shows here first.
const DEMO_HASH: &str = "sha256:40f446f7858575d2c66a223f747348b3a91ef83e844e13815e527f4d181e5283";

/// The demo recording's entry, typed out.
fn entry() -> ResearchEntry {
    ResearchEntry {
        model_id: MODEL.0.to_owned(),
        model_version: MODEL.1.to_owned(),
        identity: Identity {
            provider: "recorded".to_owned(),
            model_name: "recorded/demo-research".to_owned(),
            snapshot: Snapshot::Dated("recorded-demo-2026-10-19".to_owned()),
            quantization: None,
        },
        deadline_ms: 120_000,
        max_output_tokens: 4_096,
        seed: Some(7),
        plan: RetrievalPlan::v0_1(),
        bounds: OutputBounds::v0_1(),
        agentic: None,
    }
}

fn register(
    journal: &mut Journal,
    store: &mut dyn ArtifactStore,
    owner: &Owner,
    model: (&str, &str),
    recording: &[u8],
) -> Result<Submitted, ControlError> {
    register_research_model(journal, store, owner, model, recording, at(NOW))
}

fn refused(result: Result<impl std::fmt::Debug, ControlError>) -> &'static str {
    match result {
        Err(ControlError::Refused { reason }) => reason,
        other => panic!("not refused: {other:?}"),
    }
}

/// The recording with `edit` applied to its parsed members.
fn recording_with(edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut recording: serde_json::Value = serde_json::from_str(RECORDING).unwrap();
    edit(&mut recording);
    recording.to_string().into_bytes()
}

#[test]
fn the_research_model_is_registered_from_its_entry_with_a_recomputed_hash() {
    let (mut journal, mut store) = (Journal::default(), Store::new());
    register(
        &mut journal,
        &mut store,
        &owner(),
        MODEL,
        RECORDING.as_bytes(),
    )
    .unwrap();
    let canonical = to_canonical(&entry().content().canonical());
    let hash = Digest::of(&canonical);
    assert_eq!(hash, entry().content().content_hash());
    assert_eq!(format!("sha256:{}", hash.to_hex()), DEMO_HASH);
    assert_eq!(store.get(&hash), Some(&canonical), "the content, stored");
    let rows = journal.rows(&stream(CONTROL)).unwrap();
    let [row] = rows.as_slice() else {
        panic!("one event and no envelope change on the control stream: {rows:?}")
    };
    let draft = Draft::parse(&row.body).unwrap();
    assert_eq!(
        (draft.event_type(), draft.schema_version()),
        ("ConfigSnapshotRegistered", 1)
    );
    let expected = format!(
        r#"{{"admits_instruments":true,"content_hash":"sha256:{}","kind":"model_version","model_id":"llm.research_agent","model_version":"0.1.0","params":[]}}"#,
        hash.to_hex()
    );
    let payload = parse(&row.body).unwrap().get("payload").cloned();
    assert_eq!(payload, Some(parse(expected.as_bytes()).unwrap()));
    let mandate: serde_json::Value = serde_json::from_str(MANDATE).unwrap();
    let pinned = mandate["behavior"]["signal_models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["id"] == MODEL.0)
        .map(|model| model["content_hash"].clone());
    assert_eq!(
        pinned,
        Some(serde_json::json!(format!("sha256:{}", hash.to_hex()))),
        "the demo's research mandate names the hash its registration records"
    );
}

#[test]
fn a_registration_that_is_not_the_research_entry_s_is_refused_before_anything_is_written() {
    let missing = |member: &'static str| {
        recording_with(move |r| {
            r["entry"].as_object_mut().unwrap().remove(member);
        })
    };
    let cases: Vec<((&str, &str), Vec<u8>, &str)> = vec![
        (
            ("quant.ma_crossover", "1.0.0"),
            RECORDING.into(),
            "model_not_research",
        ),
        (
            ("research_agent", "0.1.0"),
            RECORDING.into(),
            "model_not_research",
        ),
        (("llm.other", "0.1.0"), RECORDING.into(), "entry_mismatch"),
        (
            ("llm.research_agent", "0.2.0"),
            RECORDING.into(),
            "entry_mismatch",
        ),
        (MODEL, b"not json".to_vec(), "entry_unreadable"),
        (
            MODEL,
            recording_with(|r| drop(r.as_object_mut().unwrap().remove("entry"))),
            "entry_unreadable",
        ),
        (MODEL, missing("snapshot"), "entry_unreadable"),
        (MODEL, missing("seed"), "entry_unreadable"),
        (MODEL, missing("model_version"), "entry_unreadable"),
    ];
    for (model, recording, code) in cases {
        let (mut journal, mut store) = (Journal::default(), Store::new());
        let result = register(&mut journal, &mut store, &owner(), model, &recording);
        assert_eq!(refused(result), code, "{model:?}");
        assert!(store.is_empty() && journal.attempts.is_empty(), "{model:?}");
    }
    let (mut journal, mut store) = (Journal::default(), Store::new());
    let elsewhere = Owner {
        environment: Environment::Backtest,
        ..owner()
    };
    let result = register(
        &mut journal,
        &mut store,
        &elsewhere,
        MODEL,
        RECORDING.as_bytes(),
    );
    assert_eq!(refused(result), "paper_only");
    assert!(store.is_empty() && journal.attempts.is_empty());
}

/// `sha256:` and the digest of the canonical form of `text`.
fn reference(text: &str) -> String {
    format!(
        "sha256:{}",
        Digest::of(&to_canonical(&parse(text.as_bytes()).unwrap())).to_hex()
    )
}

/// Creates and confirms `document` on a control stream registering the demo's two models: what
/// `confirm` answers, and the rules the document violates over that stream's registry. The rules
/// come from a validation context this helper builds itself, folding the stream with
/// `JournaledFact::from_record` as the spec folds it. That is not the context `confirm` builds:
/// `confirm`'s also knows whether the connection was recorded. So the violation sets here are
/// compared only against the confirmed mandate's own fold, and `confirm`'s answer is asserted
/// alongside them.
fn confirmed(document: &str) -> (Result<Submitted, ControlError>, BTreeSet<Violation>) {
    let (mut journal, mut store, mut ids) = (Journal::default(), Store::new(), FixedIds::default());
    let owner = owner();
    register_model(
        &mut journal,
        &mut store,
        &owner,
        "quant.ma_crossover",
        "1.0.0",
        at(NOW),
    )
    .unwrap();
    register(
        &mut journal,
        &mut store,
        &owner,
        MODEL,
        RECORDING.as_bytes(),
    )
    .unwrap();
    let documents = |d: &Digest| store.get(d).map(|bytes| parse(bytes).unwrap());
    let mut facts = Vec::new();
    for row in journal.rows(&stream(CONTROL)).unwrap() {
        let payload = parse(&row.body).unwrap().get("payload").unwrap().clone();
        facts.extend(
            JournaledFact::from_record(&row.event_type, &payload, &documents, None).unwrap(),
        );
    }
    let mandate = Mandate::parse(&parse(document.as_bytes()).unwrap()).unwrap();
    let args = ContextArgs {
        agent: AgentId::new("demo-research"),
        connection_id: mandate.connection_id.clone(),
        validation_date: at(NOW).at.date(),
        membership: Some(Membership {
            workspace_users: 1,
            approver_users: 1,
        }),
        independent_approval_required: false,
        instrument_groups: BTreeMap::new(),
        eligibility_failures: BTreeSet::new(),
    };
    let context = ValidationContext::from_journal(&mandate, args, &facts).unwrap();
    let violations = validate(&mandate, &context).unwrap().violations;
    let created = create(
        &mut journal,
        &mut store,
        &owner,
        document.as_bytes(),
        at(NOW),
    );
    if let Err(refusal) = created {
        return (Err(refusal), violations);
    }
    let version = reference(document);
    let gesture = format!(r#"{{"gesture":"mandate_confirm","mandate_version":"{version}"}}"#);
    let code =
        Digest::of(&to_canonical(&parse(gesture.as_bytes()).unwrap())).to_hex()[..8].to_owned();
    let answer = confirm(
        &mut journal,
        &mut store,
        &mut ids,
        &owner,
        &version,
        &code,
        at(NOW),
    );
    (answer, violations)
}

/// The demo's research mandate with `edit` applied to its signal models, kept sorted by id as
/// V-009 asks, so each case is refused for the rule it names.
fn models_with(edit: impl FnOnce(&mut Vec<serde_json::Value>)) -> String {
    let mut mandate: serde_json::Value = serde_json::from_str(MANDATE).unwrap();
    let models = mandate["behavior"]["signal_models"].as_array_mut().unwrap();
    edit(models);
    models.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    mandate.to_string()
}

/// The demo's research mandate confirms over its two registrations, the research model's admitted
/// by its recomputed hash. Each case below is refused, and adds exactly the rules it names to what
/// the confirmed mandate's bare fold already reports: V-007 for a hash other than the registered
/// one, V-007 and V-036 for a second admitting `llm.` model (it is also unregistered), and V-036
/// for an `llm.` model admitting nothing under a research envelope or a quant model claiming to
/// admit. A regression in one rule cannot hide behind another.
#[test]
fn the_demo_research_mandate_confirms_and_v_007_and_v_036_still_refuse() {
    let (answer, base) = confirmed(MANDATE);
    answer.unwrap();
    assert!(
        !base.contains(&Violation::V007) && !base.contains(&Violation::V036),
        "{base:?}"
    );
    let research = |models: &mut Vec<serde_json::Value>| {
        models
            .iter_mut()
            .find(|model| model["id"] == MODEL.0)
            .unwrap()
            .clone()
    };
    let cases = [
        (
            "a hash other than the registration's",
            &[Violation::V007][..],
            models_with(|models| {
                let model = models.iter_mut().find(|m| m["id"] == MODEL.0).unwrap();
                model["content_hash"] = serde_json::json!(format!("sha256:{}", "4".repeat(64)));
            }),
        ),
        (
            "a second admitting llm model",
            &[Violation::V007, Violation::V036][..],
            models_with(|models| {
                let mut second = research(models);
                second["id"] = serde_json::json!("llm.second_agent");
                models.push(second);
            }),
        ),
        (
            "an llm model admitting nothing under a research envelope",
            &[Violation::V036][..],
            models_with(|models| {
                let model = models.iter_mut().find(|m| m["id"] == MODEL.0).unwrap();
                model["admits_instruments"] = serde_json::json!(false);
            }),
        ),
        (
            "a quant model claiming to admit",
            &[Violation::V036][..],
            models_with(|models| {
                let model = models
                    .iter_mut()
                    .find(|m| m["id"] == "quant.ma_crossover")
                    .unwrap();
                model["admits_instruments"] = serde_json::json!(true);
            }),
        ),
    ];
    for (case, named, document) in cases {
        let (answer, violations) = confirmed(&document);
        assert_eq!(refused(answer), "version_invalid", "{case}");
        let added: BTreeSet<Violation> = violations.difference(&base).copied().collect();
        assert_eq!(added, named.iter().copied().collect(), "{case}");
    }
}
