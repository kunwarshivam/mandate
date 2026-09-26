//! Family N of [the mandate reference cases](../../../docs/specs/reference-cases/mandate.yaml),
//! **loaded from `fixtures/refcases/mandate.json`** rather than typed out here.
//!
//! Typing a case's figures into a test makes the test agree with whatever the author read, not with
//! the case; the founder owns the fixture, so the fixture is the input. Each case becomes one named
//! test (`MC_N01`), its base document supplies the envelope, its `input` block supplies the facts,
//! and its `expect` block is compared key by key.
//!
//! **Twenty-five of the twenty-eight.** MC-N01, MC-N14 and MC-N26 state `first_order_autonomy`,
//! which is stream H's `classify` over the facts this crate reports (DEC-132 item 3); they stay with
//! the hand tests in `admission.rs` until the harness PR can call both crates, and
//! [`the_three_cases_this_file_defers_are_named`] fails if that list ever changes silently.
//!
//! When stream F's `mandate` suite lands in `mandate-refcases`, these move there as `kind:
//! admission`, `lineage`, `thesis_expiry` and `stagger` interpretations and this file goes; it lives
//! here meanwhile because that suite does not exist yet and creating it would collide with stream F's
//! tests PR (the brief's Dependencies). Stream G's `mandate-risk` carries the same file for the same
//! reason.
//!
//! **Every `expect` key is read.** [`compare`] walks the case's own `expect` object and fails on a key
//! it does not know, so a case cannot pass because nobody compared the field that matters (DEC-85).

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{asset, at, dec, digest, lineage_id, source, thesis_id, usd, workspace};
use mandate_research::{
    AdmissionChange, AdmissionFacts, AdmissionInput, AllowlistVersion, AssetClass, AssetId,
    AutonomyDecision, ContentHash, Corroboration, Direction, FoldInput, InstrumentFacts,
    InstrumentRestriction, Invalidation, Lineage, LineageState, MandateEnvelope, ModelId,
    ModelVersion, OutputEnvelope, PolicyOverlay, ProposedThesis, ResearchEnvelope, ResearchEvent,
    SourceAllowlist, StaggerWindow, Thesis, UniverseChange, UniverseEntry, ValidatedMandate,
    WorkingUniverse, admit, expire_theses, fold_theses, stagger_offset,
};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/refcases/mandate.json"))
        .unwrap_or_else(|e| panic!("the reference-case fixture parses: {e}"))
}

fn case(id: &str) -> Value {
    let f = fixture();
    f.get("cases")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("the fixture lists cases"))
        .iter()
        .find(|c| c.get("id").and_then(Value::as_str) == Some(id))
        .unwrap_or_else(|| panic!("{id} is in the fixture"))
        .clone()
}

fn base_mandate(c: &Value) -> Value {
    let f = fixture();
    let base = str_at(c, "base");
    f.pointer(&format!("/bases/{base}/mandate"))
        .unwrap_or_else(|| panic!("base {base} is in the fixture"))
        .clone()
}

fn str_at(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{key} is a string in {v}"))
        .to_owned()
}

fn u32_at(v: &Value, key: &str) -> u32 {
    let n = v
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("{key} is an integer in {v}"));
    u32::try_from(n).unwrap_or_else(|_| panic!("{key} fits in u32"))
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|xs| {
            xs.iter()
                .map(|x| {
                    x.as_str()
                        .unwrap_or_else(|| panic!("{x} is a string"))
                        .to_owned()
                })
                .collect()
        })
        .unwrap_or_default()
}

fn asset_class(name: &str) -> AssetClass {
    match name {
        "us_equity" => AssetClass::UsEquity,
        "crypto" => AssetClass::Crypto,
        other => panic!("{other} is not an asset class this harness interprets"),
    }
}

fn decision(name: &str) -> AutonomyDecision {
    match name {
        "auto" => AutonomyDecision::Auto,
        "ask" => AutonomyDecision::Ask,
        "deny" => AutonomyDecision::Deny,
        other => panic!("{other} is not an autonomy decision"),
    }
}

/// The envelope fields §8.5 reads, taken from the case's base document.
///
/// The patch list is applied only where a family-N case uses one; none does (every `patch` is `[]`),
/// so a non-empty patch fails loudly rather than being ignored (DEC-85).
fn envelope_of(c: &Value) -> MandateEnvelope {
    let patch = c.get("patch").and_then(Value::as_array);
    assert!(
        patch.is_none_or(|p| p.is_empty()),
        "{} carries a patch this harness has not been taught",
        str_at(c, "id")
    );
    let m = base_mandate(c);
    let universe = m
        .get("universe")
        .unwrap_or_else(|| panic!("a base has a universe"));
    let behavior = m
        .get("behavior")
        .unwrap_or_else(|| panic!("a base has a behavior"));
    let models = behavior
        .get("signal_models")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("a base lists signal models"));
    let research = behavior.get("research").and_then(|r| {
        (!r.is_null()).then(|| ResearchEnvelope {
            interval_s: u32_at(r, "interval_s"),
            cost_cap_usd_per_day: usd(&str_at(r, "cost_cap_usd_per_day")),
            max_revisions_per_lineage: u32_at(r, "max_revisions_per_lineage"),
        })
    });
    let disclosure = universe
        .get("leveraged_etp_disclosure_version")
        .and_then(Value::as_str)
        .map(|v| digest(v.strip_prefix("sha256:").unwrap_or(v)));

    MandateEnvelope {
        universe_pinned: universe
            .get("pinned")
            .and_then(Value::as_bool)
            .unwrap_or_else(|| panic!("a base states universe.pinned")),
        max_instruments: u32_at(universe, "max_instruments"),
        asset_classes: strings(universe.get("asset_classes"))
            .iter()
            .map(|c| asset_class(c))
            .collect(),
        leveraged_etps_enabled: universe
            .get("leveraged_etps_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        leveraged_etp_disclosure_version: disclosure,
        admits_instruments: models.iter().any(|m| {
            m.get("admits_instruments")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        }),
        research,
        admission: decision(&str_at(
            m.get("autonomy")
                .unwrap_or_else(|| panic!("a base has an autonomy block")),
            "admission",
        )),
    }
}

/// One thesis and the platform's facts about it, from the case's `input`.
///
/// **The asset class and the leveraged-ETP flag come from the case's thesis fields** because the
/// fixture keeps instrument reference data there (the reference's `admit` takes one dictionary); the
/// crate takes both from [`InstrumentFacts`], and DEC-132 item 6 records why. The corroboration kind
/// is read the same way: the fixture states what the platform found.
fn proposal_of(t: &Value) -> ProposedThesis {
    let corroboration = t
        .get("corroboration")
        .and_then(|c| c.get("kind"))
        .and_then(Value::as_str)
        .map(|k| match k {
            "independent_source" => Corroboration::IndependentSource,
            "market_data" => Corroboration::MarketData,
            other => panic!("{other} is not a corroboration kind"),
        });
    let direction = match str_at(t, "direction").as_str() {
        "long" => Direction::Long,
        _ => Direction::Other,
    };
    let predecessor = t
        .get("predecessor_thesis_id")
        .and_then(Value::as_str)
        .map(thesis_id);

    ProposedThesis {
        thesis: Thesis {
            thesis_id: thesis_id(&str_at(t, "thesis_id")),
            lineage_id: lineage_id(&str_at(t, "lineage_id")),
            revision: u32_at(t, "revision"),
            predecessor_thesis_id: predecessor,
            instrument_id: asset(&str_at(t, "instrument_id")),
            output: OutputEnvelope {
                model_id: ModelId::new("llm.research_agent").expect("a fixed model id"),
                model_version: ModelVersion::new("0.1.0"),
                content_hash: ContentHash::new(digest(
                    "4444444444444444444444444444444444444444444444444444444444444444",
                )),
                as_of: at(&str_at(t, "as_of")),
                expires_at: at(&str_at(t, "expires_at")),
            },
            direction,
            horizon_s: u32_at(t, "horizon_s"),
            conviction: dec(&str_at(t, "conviction")),
            confidence: dec(&str_at(t, "confidence")),
            evidence: None,
            evidence_sources: strings(t.get("evidence_sources"))
                .iter()
                .map(|s| source(s))
                .collect(),
            invalidation: Invalidation::new(&str_at(t, "invalidation"))
                .expect("a fixture invalidation is not empty"),
        },
        instrument: InstrumentFacts {
            asset_class: asset_class(&str_at(t, "asset_class")),
            leveraged_or_inverse_etp: t
                .get("leveraged_etp")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
        corroboration,
    }
}

/// The account, operator and profile facts checks 7 to 15 read, from the case's `input`.
fn facts_of(input: &Value) -> AdmissionFacts {
    AdmissionFacts {
        allowlist: SourceAllowlist {
            version: AllowlistVersion(1),
            sources: strings(input.get("allowlisted_sources"))
                .iter()
                .map(|s| source(s))
                .collect(),
        },
        eligibility_failures: strings(input.get("eligibility_failures"))
            .iter()
            .map(|a| asset(a))
            .collect(),
        instrument_groups: input
            .get("instrument_groups")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .map(|(k, v)| {
                        (
                            asset(k),
                            mandate_research::GroupId::new(
                                v.as_str()
                                    .unwrap_or_else(|| panic!("a group id is a string")),
                            )
                            .expect("a fixture group id is not empty"),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
        claimed_by_other_agents: strings(input.get("claimed_by_other_agents"))
            .iter()
            .map(|a| asset(a))
            .collect(),
        halted_instruments: strings(input.get("halted_instruments"))
            .iter()
            .map(|a| asset(a))
            .collect(),
        disclosures_accepted: strings(input.get("disclosures_accepted"))
            .iter()
            .map(|d| digest(d.strip_prefix("sha256:").unwrap_or(d)))
            .collect(),
        data_universe: input.get("data_universe").and_then(|u| {
            (!u.is_null()).then(|| {
                strings(Some(u))
                    .iter()
                    .map(|a| asset(a))
                    .collect::<BTreeSet<AssetId>>()
            })
        }),
        research_spend_usd_today: usd(&str_at(input, "research_spend_usd_today")),
    }
}

/// The `lineages` map a case supplies as already folded state.
fn lineages_of(input: &Value) -> LineageState {
    let mut lineages = BTreeMap::new();
    let mut holders = BTreeMap::new();
    if let Some(m) = input.get("lineages").and_then(Value::as_object) {
        for (id, state) in m {
            lineages.insert(
                lineage_id(id),
                Lineage {
                    revisions: u32_at(state, "revisions"),
                    admitted: u32_at(state, "admitted"),
                    retired: state
                        .get("retired")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                },
            );
        }
    }
    if let Some(m) = input.get("lineage_instruments").and_then(Value::as_object) {
        for (id, instrument) in m {
            holders.insert(
                lineage_id(id),
                asset(
                    instrument
                        .as_str()
                        .unwrap_or_else(|| panic!("an instrument id is a string")),
                ),
            );
        }
    }
    LineageState::from_parts(lineages, holders)
}

fn universe_of(input: &Value) -> WorkingUniverse {
    WorkingUniverse::Known {
        instruments: strings(input.get("working_universe"))
            .iter()
            .map(|a| asset(a))
            .collect(),
        pinned: false,
    }
}

/// A journal entry as the fixture writes it, so a `Vec` of these compares whole.
fn journal_shape(events: &[ResearchEvent]) -> Vec<BTreeMap<String, String>> {
    events
        .iter()
        .map(|e| {
            let mut row: BTreeMap<String, String> = BTreeMap::new();
            match e {
                ResearchEvent::ThesisProposed(t) | ResearchEvent::ThesisRevised(t) => {
                    row.insert(
                        "type".into(),
                        if t.revision > 0 {
                            "ThesisRevised"
                        } else {
                            "ThesisProposed"
                        }
                        .into(),
                    );
                    row.insert("thesis_id".into(), t.thesis_id.as_str().into());
                    row.insert("lineage_id".into(), t.lineage_id.as_str().into());
                    row.insert("revision".into(), t.revision.to_string());
                    row.insert("instrument".into(), t.instrument.as_str().into());
                    row.insert("admitted".into(), t.admitted.to_string());
                    row.insert(
                        "reason".into(),
                        t.reason.map_or("null".into(), |r| r.code().to_owned()),
                    );
                    row.insert(
                        "corroboration".into(),
                        t.corroboration
                            .map_or("null".into(), |c| c.code().to_owned()),
                    );
                }
                ResearchEvent::UniverseChanged(c) => {
                    row.insert("type".into(), "UniverseChanged".into());
                    row.insert("instrument".into(), c.instrument.as_str().into());
                    row.insert(
                        "change".into(),
                        match c.change {
                            UniverseChange::Admitted => "admitted".into(),
                            UniverseChange::Removed => "removed".into(),
                        },
                    );
                    row.insert("reason".into(), c.reason.code().into());
                    row.insert("thesis_id".into(), c.thesis_id.as_str().into());
                    row.insert(
                        "universe_size_after".into(),
                        c.universe_size_after.to_string(),
                    );
                }
            }
            row
        })
        .collect()
}

/// The same shape read out of the fixture's own `journal` array.
fn expected_journal(expect: &Value, key: &str) -> Vec<BTreeMap<String, String>> {
    expect
        .get(key)
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|r| {
                    let mut row: BTreeMap<String, String> = BTreeMap::new();
                    let o = r
                        .as_object()
                        .unwrap_or_else(|| panic!("a journal row is an object"));
                    for (k, v) in o {
                        if k == "predecessor_thesis_id" {
                            continue;
                        }
                        row.insert(
                            k.clone(),
                            match v {
                                Value::String(s) => s.clone(),
                                Value::Null => "null".into(),
                                other => other.to_string(),
                            },
                        );
                    }
                    row.remove("direction");
                    row.remove("horizon_s");
                    row.remove("conviction");
                    row.remove("confidence");
                    row
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Every `expect` key this harness knows how to compare, per kind. A case stating a key that is not
/// listed fails loudly (DEC-85), so a new fixture key cannot pass unnoticed.
fn assert_every_key_known(id: &str, kind: &str, expect: &Value) {
    let known: &[&str] = match kind {
        "admission" => &[
            "admitted",
            "reason",
            "ignored",
            "change",
            "working_universe",
            "journal",
            "universe_size_after",
            "first_order_autonomy",
        ],
        "lineage" => &[
            "steps",
            "lineages",
            "lineage_instruments",
            "working_universe",
        ],
        "thesis_expiry" => &[
            "working_universe",
            "removed",
            "instrument_restrictions",
            "journal",
        ],
        "stagger" => &["offsets", "window_s"],
        other => panic!("{other} is not a family-N kind"),
    };
    let o = expect
        .as_object()
        .unwrap_or_else(|| panic!("{id}'s expect block is an object"));
    for key in o.keys() {
        assert!(
            known.contains(&key.as_str()),
            "{id} states expect.{key}, which this harness does not read (DEC-85)"
        );
    }
}

fn run_admission(id: &str) {
    let c = case(id);
    let input = c
        .get("input")
        .unwrap_or_else(|| panic!("{id} has an input"));
    let expect = c
        .get("expect")
        .unwrap_or_else(|| panic!("{id} has an expect"));
    assert_every_key_known(id, "admission", expect);

    let mandate = ValidatedMandate::from_validated_envelope(envelope_of(&c));
    let overlay = PolicyOverlay::permissive();
    let universe = universe_of(input);
    let proposal = proposal_of(
        input
            .get("thesis")
            .unwrap_or_else(|| panic!("{id} has a thesis")),
    );
    let facts = facts_of(input);
    let lineages = lineages_of(input);

    let a = admit(&AdmissionInput {
        mandate: &mandate,
        overlay: &overlay,
        universe: &universe,
        proposal: &proposal,
        facts: &facts,
        lineages: &lineages,
    })
    .unwrap_or_else(|e| panic!("{id}: the crate decides, not {}", e.code()));

    assert_eq!(
        a.decision.admitted(),
        expect
            .get("admitted")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "{id}: admitted"
    );
    assert_eq!(
        a.decision
            .reason()
            .map_or("null".to_owned(), |r| r.code().to_owned()),
        expect
            .get("reason")
            .and_then(|r| r.as_str().map(str::to_owned))
            .unwrap_or_else(|| "null".to_owned()),
        "{id}: the journaled refusal reason"
    );
    assert_eq!(
        a.decision.ignored(),
        expect
            .get("ignored")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "{id}: §8.2 ignored"
    );
    let change = match a.decision {
        mandate_research::AdmissionDecision::Admitted { change } => match change {
            AdmissionChange::Admitted => "admitted".to_owned(),
            AdmissionChange::Renewed => "renewed".to_owned(),
        },
        mandate_research::AdmissionDecision::Refused { .. } => "null".to_owned(),
    };
    assert_eq!(
        change,
        expect
            .get("change")
            .and_then(|c| c.as_str().map(str::to_owned))
            .unwrap_or_else(|| "null".to_owned()),
        "{id}: change"
    );
    let WorkingUniverse::Known { instruments, .. } = &a.universe else {
        panic!("{id}: the crate returns a known universe");
    };
    assert_eq!(
        instruments
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect::<Vec<String>>(),
        strings(expect.get("working_universe")),
        "{id}: the working universe after"
    );
    assert_eq!(
        instruments.len(),
        usize::try_from(
            expect
                .get("universe_size_after")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        )
        .unwrap_or(0),
        "{id}: universe_size_after"
    );
    assert_eq!(
        journal_shape(&a.journal),
        expected_journal(expect, "journal"),
        "{id}: the journal"
    );
}

fn run_lineage(id: &str) {
    let c = case(id);
    let input = c
        .get("input")
        .unwrap_or_else(|| panic!("{id} has an input"));
    let expect = c
        .get("expect")
        .unwrap_or_else(|| panic!("{id} has an expect"));
    assert_every_key_known(id, "lineage", expect);

    let mandate = ValidatedMandate::from_validated_envelope(envelope_of(&c));
    let overlay = PolicyOverlay::permissive();
    let universe = universe_of(input);
    let proposals: Vec<ProposedThesis> = input
        .get("theses")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{id} lists theses"))
        .iter()
        .map(proposal_of)
        .collect();
    let facts = facts_of(input);
    let lineages = lineages_of(input);

    let fold = fold_theses(&FoldInput {
        mandate: &mandate,
        overlay: &overlay,
        universe: &universe,
        proposals: &proposals,
        facts: &facts,
        lineages: &lineages,
    })
    .unwrap_or_else(|e| panic!("{id}: the fold decides, not {}", e.code()));

    let steps = expect
        .get("steps")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{id} lists steps"));
    assert_eq!(fold.steps.len(), steps.len(), "{id}: one step per thesis");
    for (step, want) in fold.steps.iter().zip(steps.iter()) {
        let at_id = str_at(want, "thesis_id");
        assert_eq!(step.thesis_id.as_str(), at_id, "{id}: step order");
        assert_eq!(
            step.decision.admitted(),
            want.get("admitted")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            "{id}/{at_id}: admitted"
        );
        assert_eq!(
            step.decision
                .reason()
                .map_or("null".to_owned(), |r| r.code().to_owned()),
            want.get("reason")
                .and_then(|r| r.as_str().map(str::to_owned))
                .unwrap_or_else(|| "null".to_owned()),
            "{id}/{at_id}: reason"
        );
        assert_eq!(
            step.score_carried_forward(),
            want.get("score_carried_forward")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            "{id}/{at_id}: MI-18 score_carried_forward"
        );
        assert_eq!(
            step.lineage_revisions,
            u32_at(want, "lineage_revisions"),
            "{id}/{at_id}: lineage_revisions"
        );
        assert_eq!(
            step.lineage_retired,
            want.get("lineage_retired")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            "{id}/{at_id}: lineage_retired"
        );
        assert_eq!(
            journal_shape(&step.journal),
            expected_journal(want, "journal"),
            "{id}/{at_id}: the journal"
        );
    }

    for (lineage, want) in expect
        .get("lineages")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("{id} states the folded lineages"))
    {
        let got = fold
            .lineages
            .lineage(&lineage_id(lineage))
            .unwrap_or_else(|| panic!("{id}: the fold walked lineage {lineage}"));
        assert_eq!(
            (got.revisions, got.admitted, got.retired),
            (
                u32_at(want, "revisions"),
                u32_at(want, "admitted"),
                want.get("retired")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            ),
            "{id}: lineage {lineage}"
        );
    }
    let holders: BTreeMap<String, String> = fold
        .lineages
        .holders()
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), v.as_str().to_owned()))
        .collect();
    let want_holders: BTreeMap<String, String> = expect
        .get("lineage_instruments")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.as_str()
                            .unwrap_or_else(|| panic!("an instrument id"))
                            .to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(holders, want_holders, "{id}: which lineage holds what");

    let WorkingUniverse::Known { instruments, .. } = &fold.universe else {
        panic!("{id}: the fold returns a known universe");
    };
    assert_eq!(
        instruments
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect::<Vec<String>>(),
        strings(expect.get("working_universe")),
        "{id}: the working universe after the fold"
    );
}

fn run_thesis_expiry(id: &str) {
    let c = case(id);
    let input = c
        .get("input")
        .unwrap_or_else(|| panic!("{id} has an input"));
    let expect = c
        .get("expect")
        .unwrap_or_else(|| panic!("{id} has an expect"));
    assert_every_key_known(id, "thesis_expiry", expect);

    let entries: Vec<UniverseEntry> = input
        .get("entries")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{id} lists entries"))
        .iter()
        .map(|e| UniverseEntry {
            instrument: asset(&str_at(e, "instrument")),
            thesis_id: thesis_id(&str_at(e, "thesis_id")),
            lineage_id: lineage_id(&str_at(e, "lineage_id")),
            revision: u32_at(e, "revision"),
            expires_at: at(&str_at(e, "expires_at")),
            invalidated: e
                .get("invalidated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect();

    let e = expire_theses(at(&str_at(input, "now")), &entries, &lineages_of(input))
        .unwrap_or_else(|err| panic!("{id}: the removal is decided, not {}", err.code()));

    let WorkingUniverse::Known { instruments, .. } = &e.universe else {
        panic!("{id}: expiry returns a known universe");
    };
    assert_eq!(
        instruments
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect::<Vec<String>>(),
        strings(expect.get("working_universe")),
        "{id}: the working universe after"
    );
    assert_eq!(
        e.removed
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect::<Vec<String>>(),
        strings(expect.get("removed")),
        "{id}: removed"
    );
    let restrictions: BTreeMap<String, String> = e
        .instrument_restrictions
        .iter()
        .map(|(instrument, restriction)| {
            (
                instrument.as_str().to_owned(),
                match restriction {
                    InstrumentRestriction::RemovedInstrument => "removed_instrument".to_owned(),
                    InstrumentRestriction::StaleMark => "stale_mark".to_owned(),
                },
            )
        })
        .collect();
    let want: BTreeMap<String, String> = expect
        .get("instrument_restrictions")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.as_str()
                            .unwrap_or_else(|| panic!("a restriction"))
                            .to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(restrictions, want, "{id}: instrument restrictions");
    assert_eq!(
        journal_shape(&e.journal),
        expected_journal(expect, "journal"),
        "{id}: the journal"
    );
}

fn run_stagger(id: &str) {
    let c = case(id);
    let input = c
        .get("input")
        .unwrap_or_else(|| panic!("{id} has an input"));
    let expect = c
        .get("expect")
        .unwrap_or_else(|| panic!("{id} has an expect"));
    assert_every_key_known(id, "stagger", expect);

    let window = StaggerWindow(u32_at(input, "window_s"));
    let offsets: Vec<u32> = input
        .get("pairs")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{id} lists pairs"))
        .iter()
        .map(|pair| {
            let p = pair
                .as_array()
                .unwrap_or_else(|| panic!("a pair is an array"));
            let ws = p
                .first()
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("a workspace id"));
            let th = p
                .get(1)
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("a thesis id"));
            stagger_offset(&workspace(ws), &thesis_id(th), window)
                .unwrap_or_else(|e| panic!("{id}: the offset is exact, not {}", e.code()))
        })
        .collect();

    assert_eq!(
        offsets,
        expect
            .get("offsets")
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{id} states offsets"))
            .iter()
            .map(|o| u32::try_from(o.as_u64().unwrap_or(0)).unwrap_or(0))
            .collect::<Vec<u32>>(),
        "{id}: the deterministic offsets"
    );
    assert_eq!(
        u32_at(input, "window_s"),
        u32_at(expect, "window_s"),
        "{id}: the window the offsets are inside"
    );
}

macro_rules! refcase {
    ($name:ident, $id:literal, $run:ident, $story:literal) => {
        #[test]
        #[ignore = $story]
        fn $name() {
            $run($id);
        }
    };
}

refcase!(mc_n02, "MC-N02", run_admission, "pending E17-3");
refcase!(mc_n03, "MC-N03", run_admission, "pending E17-3");
refcase!(mc_n04, "MC-N04", run_admission, "pending E17-3");
refcase!(mc_n05, "MC-N05", run_admission, "pending E17-3");
refcase!(mc_n06, "MC-N06", run_admission, "pending E17-7");
refcase!(mc_n07, "MC-N07", run_admission, "pending E17-7");
refcase!(mc_n08, "MC-N08", run_admission, "pending E17-3");
refcase!(mc_n09, "MC-N09", run_admission, "pending E17-3");
refcase!(mc_n10, "MC-N10", run_admission, "pending E17-3");
refcase!(mc_n11, "MC-N11", run_admission, "pending E17-3");
refcase!(mc_n12, "MC-N12", run_admission, "pending E17-3");
refcase!(mc_n13, "MC-N13", run_admission, "pending E17-3");
refcase!(mc_n15, "MC-N15", run_admission, "pending E17-3");
refcase!(mc_n16, "MC-N16", run_admission, "pending E17-3");
refcase!(mc_n25, "MC-N25", run_admission, "pending E17-3");
refcase!(mc_n17, "MC-N17", run_lineage, "pending E17-9");
refcase!(mc_n18, "MC-N18", run_lineage, "pending E17-9");
refcase!(mc_n19, "MC-N19", run_lineage, "pending E17-9");
refcase!(mc_n24, "MC-N24", run_lineage, "pending E17-9");
refcase!(mc_n27, "MC-N27", run_lineage, "pending E17-9");
refcase!(mc_n28, "MC-N28", run_lineage, "pending E17-9");
refcase!(mc_n20, "MC-N20", run_thesis_expiry, "pending E17-3");
refcase!(mc_n21, "MC-N21", run_thesis_expiry, "pending E17-3");
refcase!(mc_n22, "MC-N22", run_thesis_expiry, "pending E17-9");
refcase!(mc_n23, "MC-N23", run_stagger, "pending E17-3");

/// The three cases this file defers, and why. If the fixture ever stops stating
/// `first_order_autonomy` for one of them — or starts stating it for another — this fails rather than
/// leaving a case silently uncovered on both sides.
#[test]
fn the_three_cases_this_file_defers_are_named() {
    let f = fixture();
    let deferred: Vec<String> = f
        .get("cases")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("the fixture lists cases"))
        .iter()
        .filter(|c| {
            c.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id.starts_with("MC-N"))
                && c.pointer("/expect/first_order_autonomy")
                    .is_some_and(|v| !v.is_null())
        })
        .map(|c| {
            c.get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
        .collect();

    assert_eq!(
        deferred,
        vec!["MC-N01", "MC-N14", "MC-N26"],
        "exactly these three need stream H's classify, and admission.rs covers them by hand \
         until the harness PR can call both crates (DEC-132 item 3)"
    );
}

/// Every family-N case is covered on one side or the other: run here, or deferred above.
#[test]
fn every_family_n_case_is_covered() {
    let f = fixture();
    let all: Vec<String> = f
        .get("cases")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("the fixture lists cases"))
        .iter()
        .filter_map(|c| c.get("id").and_then(Value::as_str))
        .filter(|id| id.starts_with("MC-N"))
        .map(str::to_owned)
        .collect();
    let here: BTreeSet<&str> = [
        "MC-N02", "MC-N03", "MC-N04", "MC-N05", "MC-N06", "MC-N07", "MC-N08", "MC-N09", "MC-N10",
        "MC-N11", "MC-N12", "MC-N13", "MC-N15", "MC-N16", "MC-N17", "MC-N18", "MC-N19", "MC-N20",
        "MC-N21", "MC-N22", "MC-N23", "MC-N24", "MC-N25", "MC-N27", "MC-N28",
    ]
    .into_iter()
    .collect();
    let deferred: BTreeSet<&str> = ["MC-N01", "MC-N14", "MC-N26"].into_iter().collect();

    assert_eq!(all.len(), 28, "family N holds 28 cases");
    assert_eq!(here.len(), 25, "25 run from the fixture here");
    for id in &all {
        assert!(
            here.contains(id.as_str()) || deferred.contains(id.as_str()),
            "{id} is neither run here nor named as deferred"
        );
    }
}
