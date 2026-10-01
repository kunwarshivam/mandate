//! Family N of the `mandate` suite: the research agent's `admission`, `lineage`, `thesis_expiry`, and
//! `stagger` cases (spec §8.4 to §8.6), run through `mandate-research` (DEC-132, the brief's
//! "case-loading design", DEC-154).
//!
//! **Every key is read (DEC-85).** Each object a case carries — `input`, the thesis, its
//! corroboration, `admission_action`, a lineage's state, an expiry entry, a step's expectation, and
//! the expected `first_order_autonomy` — is swept against the members this module reads, and a
//! member it does not know fails the case naming it. A journal row is compared whole, as a JSON
//! object built from the crate's event, so a row that grew a member fails as a difference.
//!
//! **What the harness fills and why.** The envelope comes from the case's patched base, projected
//! onto `mandate_research::MandateEnvelope` field by field, because the research crate reads its own
//! stand-in types (DEC-132 item 24). The
//! thesis's `asset_class` and `leveraged_etp` fill `InstrumentFacts`, and `corroboration.kind` fills
//! the platform's corroboration, because the fixture keeps instrument reference data and what the
//! platform found in the thesis dictionary (DEC-132 items 6 and 7). The output envelope's model
//! identity is the base's admitting signal model, the only model that proposes theses. No family-N
//! case states a policy, so the overlay is `PolicyOverlay::permissive()`, as `ref.py`'s `admit`
//! applies none.
//!
//! **`first_order_autonomy`.** Three cases state the first order's autonomy decision, which is
//! `mandate_builder::classify` over the facts admission reports (DEC-132 item 3, DEC-179). The
//! policy is the case's patched base through the strict parse, unvalidated, as family A's is
//! (DEC-162 item 1), and admission's reported `admission_ceiling` must equal that policy's
//! `admission`, since no overlay applies. The action is `admission_action` as stated, with `purpose`
//! `open` and `new_instrument` and `thesis_confidence` from admission's facts, as `ref.py`'s `admit`
//! composes it; the classification is compared whole, so an approval member stated or omitted
//! wrongly fails. Where the expectation is `null`, the admission must report no first-order facts,
//! which is why `admission_action`, the input only `classify` reads, has nothing to decide there;
//! its members are still swept.
//!
//! **One invented value.** The source allowlist's version is `AllowlistVersion(1)`: no case states
//! one and no check reads it; it reaches only the thesis entry's `allowlist_version`, which the
//! fixture's journal rows do not state.

use std::collections::{BTreeMap, BTreeSet};

use mandate_builder::{ActionContext, BuilderError, Classification, RequestedBy, classify};
use mandate_canon::{DecStr, Digest};
use mandate_domain::{MarketSession, Purpose};
use mandate_num::{Signed, Unit, Usd};
use mandate_research::{
    AdmissionChange, AdmissionDecision, AdmissionFacts, AdmissionInput, AllowlistVersion,
    AssetClass, AssetId, AutonomyDecision, ContentHash, Corroboration, Direction, FirstOrderFacts,
    FoldInput, GroupId, InstrumentFacts, InstrumentRestriction, Invalidation, Lineage, LineageId,
    LineageState, MandateEnvelope, ModelId, ModelVersion, OutputEnvelope, PolicyOverlay,
    ProposedThesis, ResearchEnvelope, ResearchEvent, SchemaDec, SourceAllowlist, SourceId,
    StaggerWindow, Thesis, ThesisId, UniverseChange, UniverseEntry, ValidatedMandate,
    WorkingUniverse, WorkspaceId, admit, expire_theses, fold_theses, stagger_offset,
};
use mandate_spec::document::OnTimeout;
use mandate_time::UtcNanos;
use serde_json::json;

use super::{must_parse, not_implemented, num, patched, patched_json, u32_of, unknown_members};
use crate::{Json, at, ensure, expect_eq, list_at, str_at};

/// The facts every `admission` and `lineage` input states besides its thesis or theses.
const FACT_KEYS: &str = "working_universe eligibility_failures allowlisted_sources \
    instrument_groups claimed_by_other_agents halted_instruments data_universe \
    research_spend_usd_today lineages disclosures_accepted admission_action";
const THESIS_KEYS: &str = "thesis_id lineage_id revision predecessor_thesis_id instrument_id \
    asset_class direction horizon_s conviction confidence as_of expires_at evidence_sources \
    corroboration leveraged_etp invalidation";
/// `classify`'s action context (§6.2), read only by the first order's autonomy decision.
const ACTION_KEYS: &str = "order_usd combined_score instrument asset_class session \
    first_trade_in_instrument drawdown daily_pnl_fraction position_usd_after gross_usd_after \
    bought_today_usd position_pnl_fraction unusual_input";
const STEP_KEYS: &str = "thesis_id admitted reason score_carried_forward lineage_revisions \
    lineage_retired universe_size_after journal";
const ENTRY_KEYS: &str = "instrument thesis_id lineage_id revision expires_at invalidated";

type ModelIdentity = (ModelId, ModelVersion, ContentHash);

/// `kind: admission` — one thesis through §8.5's seventeen checks.
pub(super) fn admission_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let (input, expect) = (at(case, "input")?, at(case, "expect")?);
    swept(input, &format!("{FACT_KEYS} thesis"), "input")?;
    swept(
        at(input, "admission_action")?,
        ACTION_KEYS,
        "admission_action",
    )?;
    let (mandate, model) = envelope(&patched_json(fixture, case)?)?;
    let thesis = at(input, "thesis")?;
    let a = admit(&AdmissionInput {
        mandate: &mandate,
        overlay: &PolicyOverlay::permissive(),
        universe: &universe(input)?,
        proposal: &proposal(thesis, &model)?,
        facts: &facts(input)?,
        lineages: &lineages(input)?,
    })
    .map_err(|e| format!("admit: {}", e.code()))?;
    verdict(expect, a.decision)?;
    same("ignored", a.decision.ignored().into(), expect)?;
    let change = match a.decision {
        AdmissionDecision::Admitted {
            change: AdmissionChange::Admitted,
        } => Some("admitted"),
        AdmissionDecision::Admitted { .. } => Some("renewed"),
        AdmissionDecision::Refused { .. } => None,
    };
    same("change", change.into(), expect)?;
    let held = same_universe(&a.universe, expect)?;
    same("universe_size_after", held.into(), expect)?;
    journal(&a.journal, list_at(expect, "journal")?, &[thesis])?;
    let wanted = at(expect, "first_order_autonomy")?;
    if wanted.is_null() {
        return ensure(a.first_order.is_none(), || {
            "first_order_autonomy: expected none, but admission reported first-order facts".into()
        });
    }
    swept(
        wanted,
        "decision by approvers_required on_timeout",
        "first_order_autonomy",
    )?;
    let first_order = a.first_order.as_ref().ok_or_else(|| {
        "first_order_autonomy: expected a decision, but admission reported no first order"
            .to_owned()
    })?;
    let mandate = must_parse(&patched(fixture, case)?)?;
    expect_eq(
        "first_order admission_ceiling against the parsed mandate's `autonomy.admission`",
        research_decision_name(first_order.admission_ceiling),
        decision_name(mandate.autonomy.admission),
    )?;
    let action = first_order_action(at(input, "admission_action")?, first_order)?;
    let decided = classify(&mandate.autonomy, &action).map_err(builder)?;
    expect_eq(
        "first_order_autonomy",
        classification_json(&decided),
        wanted.clone(),
    )
}

/// The first order's §6.3 facts: the case's `admission_action` as stated, with `purpose` `open`
/// and `new_instrument` and `thesis_confidence` from the facts admission reported, as `ref.py`'s
/// `admit` composes them. `unusual_input` must be `false`: V-018 reserves the field until the
/// input-drift detector ships, `ActionContext` has no place for it, and a `true` would be an input
/// no v1 decision can read.
fn first_order_action(
    stated: &Json,
    first_order: &FirstOrderFacts,
) -> Result<ActionContext, String> {
    ensure(!bool_of(stated, "unusual_input")?, || {
        "`admission_action.unusual_input` is true, which no v1 decision reads (V-018)".to_owned()
    })?;
    let usd = |key: &str| num(Usd::parse(str_at(stated, key)?), key);
    let unit = |key: &str| num(Unit::parse(str_at(stated, key)?), key);
    let signed = |key: &str| num(Signed::parse(str_at(stated, key)?), key);
    Ok(ActionContext {
        purpose: Purpose::Open,
        order_usd: usd("order_usd")?,
        combined_score: unit("combined_score")?,
        instrument: mandate_domain::AssetId::parse(str_at(stated, "instrument")?)
            .map_err(|e| format!("`instrument`: {}", e.code()))?,
        asset_class: mandate_domain::AssetClass::parse(str_at(stated, "asset_class")?)
            .map_err(|e| format!("`asset_class`: {}", e.code()))?,
        session: MarketSession::parse_condition_form(str_at(stated, "session")?)
            .map_err(|e| format!("`session`: {}", e.code()))?,
        first_trade_in_instrument: bool_of(stated, "first_trade_in_instrument")?,
        new_instrument: first_order.new_instrument,
        thesis_confidence: num(
            Unit::parse(first_order.thesis_confidence.as_str()),
            "thesis_confidence",
        )?,
        drawdown: unit("drawdown")?,
        daily_pnl_fraction: signed("daily_pnl_fraction")?,
        position_usd_after: usd("position_usd_after")?,
        gross_usd_after: usd("gross_usd_after")?,
        bought_today_usd: usd("bought_today_usd")?,
        position_pnl_fraction: signed("position_pnl_fraction")?,
        requested_by: RequestedBy::Agent,
    })
}

/// A classification as the fixture writes one: the approval members appear exactly when §6.4
/// gives the decision an approval, so comparing the whole object checks them both ways.
fn classification_json(decided: &Classification) -> Json {
    let mut out = json!({ "decision": decision_name(decided.decision), "by": decided.by.label() });
    if let (Some(approval), Some(members)) = (decided.approval, out.as_object_mut()) {
        members.insert(
            "approvers_required".to_owned(),
            u64::from(approval.approvers_required.get()).into(),
        );
        let on_timeout = match approval.on_timeout {
            OnTimeout::Skip => "skip",
        };
        members.insert("on_timeout".to_owned(), on_timeout.into());
    }
    out
}

/// The §6.2 spelling, written here rather than taken from a crate: the fixture's word is the
/// expectation, so the harness must know it independently.
fn decision_name(decision: mandate_domain::AutonomyDecision) -> &'static str {
    match decision {
        mandate_domain::AutonomyDecision::Auto => "auto",
        mandate_domain::AutonomyDecision::Ask => "ask",
        mandate_domain::AutonomyDecision::Deny => "deny",
    }
}

/// [`decision_name`] for the research crate's own stand-in type (DEC-132 item 24).
fn research_decision_name(decision: AutonomyDecision) -> &'static str {
    match decision {
        AutonomyDecision::Auto => "auto",
        AutonomyDecision::Ask => "ask",
        AutonomyDecision::Deny => "deny",
    }
}

/// A `mandate-builder` refusal, with `unimplemented` turned into the message DEC-77 requires.
fn builder(error: BuilderError) -> String {
    if error.code() == "unimplemented" {
        not_implemented("`mandate_builder::classify`")
    } else {
        format!("`mandate_builder::classify`: {error} ({})", error.code())
    }
}

/// `kind: lineage` — a sequence of theses folded through §8.6, step by step.
pub(super) fn lineage_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let (input, expect) = (at(case, "input")?, at(case, "expect")?);
    swept(input, &format!("{FACT_KEYS} theses"), "input")?;
    swept(
        at(input, "admission_action")?,
        ACTION_KEYS,
        "admission_action",
    )?;
    let (mandate, model) = envelope(&patched_json(fixture, case)?)?;
    let theses = list_at(input, "theses")?;
    let proposals = theses
        .iter()
        .map(|t| proposal(t, &model))
        .collect::<Result<Vec<_>, _>>()?;
    let start = universe(input)?;
    let fold = fold_theses(&FoldInput {
        mandate: &mandate,
        overlay: &PolicyOverlay::permissive(),
        universe: &start,
        proposals: &proposals,
        facts: &facts(input)?,
        lineages: &lineages(input)?,
    })
    .map_err(|e| format!("fold_theses: {}", e.code()))?;
    let steps = list_at(expect, "steps")?;
    expect_eq("steps", fold.steps.len(), steps.len())?;
    let mut size = members(&start)?.len();
    for ((step, want), thesis) in fold.steps.iter().zip(steps).zip(theses) {
        let id = str_at(want, "thesis_id")?;
        swept(want, STEP_KEYS, id)?;
        expect_eq("step thesis_id", step.thesis_id.as_str(), id)?;
        size = size_after(size, &step.journal)?;
        let pairs = [
            ("score_carried_forward", step.score_carried_forward().into()),
            ("lineage_revisions", step.lineage_revisions.into()),
            ("lineage_retired", step.lineage_retired.into()),
            ("universe_size_after", size.into()),
        ];
        pairs
            .into_iter()
            .try_for_each(|(key, got)| same(key, got, want))
            .and_then(|()| verdict(want, step.decision))
            .and_then(|()| journal(&step.journal, list_at(want, "journal")?, &[thesis]))
            .map_err(|e| format!("{id}: {e}"))?;
    }
    let folded = fold.lineages.lineages().iter();
    let folded: serde_json::Map<_, _> = folded
        .map(|(id, l)| (id.as_str().to_owned(), lineage_json(*l)))
        .collect();
    same("lineages", folded.into(), expect)?;
    let holders = fold.lineages.holders().iter();
    let holders: serde_json::Map<_, _> = holders
        .map(|(id, a)| (id.as_str().to_owned(), a.as_str().into()))
        .collect();
    same("lineage_instruments", holders.into(), expect)?;
    same_universe(&fold.universe, expect).map(|_| ())
}

/// `kind: thesis_expiry` — removals at the horizon, on invalidation, and on retirement (DEC-118).
pub(super) fn thesis_expiry_case(case: &Json) -> Result<(), String> {
    let (input, expect) = (at(case, "input")?, at(case, "expect")?);
    swept(input, "now entries lineages", "input")?;
    let entries = list_at(input, "entries")?
        .iter()
        .map(|e| {
            swept(e, ENTRY_KEYS, "entry")?;
            Ok(UniverseEntry {
                instrument: asset(str_at(e, "instrument")?)?,
                thesis_id: thesis_id(str_at(e, "thesis_id")?)?,
                lineage_id: lineage_id(str_at(e, "lineage_id")?)?,
                revision: u32_of(e, "revision")?,
                expires_at: instant(str_at(e, "expires_at")?)?,
                invalidated: e
                    .get("invalidated")
                    .map_or(Ok(false), |_| bool_of(e, "invalidated"))?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let now = instant(str_at(input, "now")?)?;
    let e = expire_theses(now, &entries, &lineages(input)?)
        .map_err(|e| format!("expire_theses: {}", e.code()))?;
    same_universe(&e.universe, expect)?;
    let removed: Vec<&str> = e.removed.iter().map(AssetId::as_str).collect();
    same("removed", removed.into(), expect)?;
    let restrictions: serde_json::Map<_, _> = e
        .instrument_restrictions
        .iter()
        .map(|(a, r)| {
            let code = match r {
                InstrumentRestriction::RemovedInstrument => "removed_instrument",
                InstrumentRestriction::StaleMark => "stale_mark",
            };
            (a.as_str().to_owned(), code.into())
        })
        .collect();
    same("instrument_restrictions", restrictions.into(), expect)?;
    journal(&e.journal, list_at(expect, "journal")?, &[])
}

/// `kind: stagger` — §8.4's deterministic offset for each (workspace, thesis) pair (DEC-100).
pub(super) fn stagger_case(case: &Json) -> Result<(), String> {
    let (input, expect) = (at(case, "input")?, at(case, "expect")?);
    swept(input, "window_s pairs", "input")?;
    let window = u32_of(input, "window_s")?;
    let offsets = list_at(input, "pairs")?
        .iter()
        .map(|pair| {
            let (Some(ws), Some(th), None) = (
                pair.get(0).and_then(Json::as_str),
                pair.get(1).and_then(Json::as_str),
                pair.get(2),
            ) else {
                return Err(format!("a pair is [workspace, thesis], got {pair}"));
            };
            let ws = WorkspaceId::new(ws).map_err(|e| e.code().to_owned())?;
            stagger_offset(&ws, &thesis_id(th)?, StaggerWindow(window))
                .map_err(|e| format!("stagger_offset: {}", e.code()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    same("offsets", offsets.into(), expect)?;
    same("window_s", window.into(), expect)
}

/// `admitted` and `reason`, which an admission and a fold step both state.
fn verdict(expect: &Json, decision: AdmissionDecision) -> Result<(), String> {
    same("admitted", decision.admitted().into(), expect)?;
    same("reason", decision.reason().map(|r| r.code()).into(), expect)
}

/// `got` against the expectation's `key`, compared as JSON.
fn same(key: &str, got: Json, expect: &Json) -> Result<(), String> {
    expect_eq(key, got, at(expect, key)?.clone())
}

/// The member-set sweep over space-separated names, with the object named in the failure.
fn swept(value: &Json, known: &str, what: &str) -> Result<(), String> {
    let known: Vec<&str> = known.split_whitespace().collect();
    unknown_members(value, &known)
        .map_err(|unknown| format!("`{what}` members not interpreted: {unknown}"))
}

/// The envelope fields §8.5 reads, and the admitting model's identity for the output envelope.
fn envelope(mandate: &Json) -> Result<(ValidatedMandate, ModelIdentity), String> {
    let universe = at(mandate, "universe")?;
    let models = list_at(mandate, "behavior.signal_models")?;
    let admitting: Vec<&Json> = models
        .iter()
        .filter(|m| m.get("admits_instruments").and_then(Json::as_bool) == Some(true))
        .collect();
    let identity = admitting.first().copied().or(models.first());
    let identity = identity.ok_or("a base lists no signal model")?;
    let research = match at(mandate, "behavior.research")? {
        Json::Null => None,
        r => Some(ResearchEnvelope {
            interval_s: u32_of(r, "interval_s")?,
            cost_cap_usd_per_day: usd(str_at(r, "cost_cap_usd_per_day")?)?,
            max_revisions_per_lineage: u32_of(r, "max_revisions_per_lineage")?,
        }),
    };
    let disclosure = match at(universe, "leveraged_etp_disclosure_version")? {
        Json::Null => None,
        v => Some(digest(
            v.as_str().ok_or("a disclosure version is a string")?,
        )?),
    };
    let envelope = MandateEnvelope {
        universe_pinned: bool_of(universe, "pinned")?,
        max_instruments: u32_of(universe, "max_instruments")?,
        asset_classes: strings(universe, "asset_classes")?
            .iter()
            .map(|c| asset_class(c))
            .collect::<Result<_, _>>()?,
        leveraged_etps_enabled: bool_of(universe, "leveraged_etps_enabled")?,
        leveraged_etp_disclosure_version: disclosure,
        admits_instruments: !admitting.is_empty(),
        research,
        admission: match str_at(mandate, "autonomy.admission")? {
            "auto" => AutonomyDecision::Auto,
            "ask" => AutonomyDecision::Ask,
            "deny" => AutonomyDecision::Deny,
            other => return Err(format!("`{other}` is not an autonomy decision")),
        },
    };
    let model = (
        ModelId::new(str_at(identity, "id")?).map_err(|e| e.to_string())?,
        ModelVersion::new(str_at(identity, "version")?),
        ContentHash::new(digest(str_at(identity, "content_hash")?)?),
    );
    Ok((ValidatedMandate::from_validated_envelope(envelope), model))
}

/// One thesis with the platform's facts about it (the module doc says where each comes from).
fn proposal(t: &Json, model: &ModelIdentity) -> Result<ProposedThesis, String> {
    swept(t, THESIS_KEYS, "thesis")?;
    let corroboration = match at(t, "corroboration")? {
        Json::Null => None,
        c => {
            swept(c, "kind", "corroboration")?;
            match c.get("kind").map(Json::as_str) {
                None | Some(Some("")) => None,
                Some(Some("independent_source")) => Some(Corroboration::IndependentSource),
                Some(Some("market_data")) => Some(Corroboration::MarketData),
                Some(other) => return Err(format!("{other:?} is not a corroboration kind")),
            }
        }
    };
    let predecessor = match at(t, "predecessor_thesis_id")? {
        Json::Null => None,
        p => Some(thesis_id(
            p.as_str().ok_or("a predecessor id is a string")?,
        )?),
    };
    let long = str_at(t, "direction")? == "long";
    let thesis = Thesis {
        thesis_id: thesis_id(str_at(t, "thesis_id")?)?,
        lineage_id: lineage_id(str_at(t, "lineage_id")?)?,
        revision: u32_of(t, "revision")?,
        predecessor_thesis_id: predecessor,
        instrument_id: asset(str_at(t, "instrument_id")?)?,
        output: OutputEnvelope {
            model_id: model.0.clone(),
            model_version: model.1.clone(),
            content_hash: model.2.clone(),
            as_of: instant(str_at(t, "as_of")?)?,
            expires_at: instant(str_at(t, "expires_at")?)?,
        },
        direction: if long {
            Direction::Long
        } else {
            Direction::Other
        },
        horizon_s: u32_of(t, "horizon_s")?,
        conviction: decimal(str_at(t, "conviction")?)?,
        confidence: decimal(str_at(t, "confidence")?)?,
        evidence: None,
        evidence_sources: sources(t, "evidence_sources")?.into_iter().collect(),
        invalidation: Invalidation::new(str_at(t, "invalidation")?)
            .map_err(|e| e.code().to_owned())?,
    };
    let instrument = InstrumentFacts {
        asset_class: asset_class(str_at(t, "asset_class")?)?,
        leveraged_or_inverse_etp: bool_of(t, "leveraged_etp")?,
    };
    Ok(ProposedThesis {
        thesis,
        instrument,
        corroboration,
    })
}

/// Checks 7 to 15's account, operator, and profile facts.
fn facts(input: &Json) -> Result<AdmissionFacts, String> {
    let groups = at(input, "instrument_groups")?.as_object();
    let groups = groups
        .ok_or("`instrument_groups` is an object")?
        .iter()
        .map(|(a, g)| {
            let g = g.as_str().ok_or("a group id is a string")?;
            Ok((asset(a)?, GroupId::new(g).map_err(|e| e.to_string())?))
        });
    let data_universe = match at(input, "data_universe")? {
        Json::Null => None,
        _ => Some(assets(input, "data_universe")?),
    };
    let disclosures = strings(input, "disclosures_accepted")?;
    let disclosures = disclosures.iter().map(|d| digest(d));
    Ok(AdmissionFacts {
        allowlist: SourceAllowlist {
            version: AllowlistVersion(1),
            sources: sources(input, "allowlisted_sources")?.into_iter().collect(),
        },
        eligibility_failures: assets(input, "eligibility_failures")?,
        instrument_groups: groups.collect::<Result<_, String>>()?,
        claimed_by_other_agents: assets(input, "claimed_by_other_agents")?,
        halted_instruments: assets(input, "halted_instruments")?,
        disclosures_accepted: disclosures.collect::<Result<_, _>>()?,
        data_universe,
        research_spend_usd_today: usd(str_at(input, "research_spend_usd_today")?)?,
    })
}

/// The folded lineage state an input supplies. `ref.py`'s fold also reads an input
/// `lineage_instruments`, but no case states one, so the sweep rejects it rather than this module
/// carrying a reading nothing exercises; every lineage therefore starts holding nothing.
fn lineages(input: &Json) -> Result<LineageState, String> {
    let mut folded = BTreeMap::new();
    let listed = at(input, "lineages")?.as_object();
    for (id, state) in listed.ok_or("`lineages` is an object")? {
        swept(state, "revisions admitted retired", id)?;
        let lineage = Lineage {
            revisions: u32_of(state, "revisions")?,
            admitted: u32_of(state, "admitted")?,
            retired: bool_of(state, "retired")?,
        };
        folded.insert(lineage_id(id)?, lineage);
    }
    Ok(LineageState::from_parts(folded, BTreeMap::new()))
}

fn universe(input: &Json) -> Result<WorkingUniverse, String> {
    let listed = strings(input, "working_universe")?;
    let instruments = assets(input, "working_universe")?;
    ensure(instruments.len() == listed.len(), || {
        "the input universe repeats an instrument (MI-15)".to_owned()
    })?;
    Ok(WorkingUniverse::Known {
        instruments,
        pinned: false,
    })
}

fn members(universe: &WorkingUniverse) -> Result<&BTreeSet<AssetId>, String> {
    match universe {
        WorkingUniverse::Known { instruments, .. } => Ok(instruments),
        WorkingUniverse::Unavailable => Err("the crate returned an unavailable universe".into()),
    }
}

/// The working universe compared as a set: the crate holds a `BTreeSet`, and for a refusal `ref.py`
/// hands back the input list in its authored order, so the array's order carries no meaning. A
/// repeated member in the expectation fails rather than being absorbed. Returns the size held.
fn same_universe(got: &WorkingUniverse, expect: &Json) -> Result<usize, String> {
    let want = strings(expect, "working_universe")?;
    let unique: BTreeSet<&str> = want.iter().map(String::as_str).collect();
    ensure(unique.len() == want.len(), || {
        "the expected universe repeats an instrument (MI-15)".to_owned()
    })?;
    let held = members(got)?;
    let mine: BTreeSet<&str> = held.iter().map(AssetId::as_str).collect();
    expect_eq("working_universe", mine, unique)?;
    Ok(held.len())
}

/// The universe size after one fold step, derived from its journaled changes rather than read back
/// from the crate, which reports no size per step.
fn size_after(size: usize, journal: &[ResearchEvent]) -> Result<usize, String> {
    journal.iter().try_fold(size, |size, event| match event {
        ResearchEvent::UniverseChanged(c) if c.change == UniverseChange::Admitted => size
            .checked_add(1)
            .ok_or("the universe size overflows".to_owned()),
        ResearchEvent::UniverseChanged(_) => size
            .checked_sub(1)
            .ok_or("a removal from an empty universe".to_owned()),
        _ => Ok(size),
    })
}

/// The journal, each event rendered as the fixture writes it and compared whole. `theses` are the
/// input theses the entries came from, for `direction`, which the crate keeps only as long or not.
fn journal(events: &[ResearchEvent], expect: &[Json], theses: &[&Json]) -> Result<(), String> {
    let rows = events.iter().map(|e| row(e, theses));
    let rows = rows.collect::<Result<Vec<_>, _>>()?;
    expect_eq("journal", Json::Array(rows), Json::Array(expect.to_vec()))
}

fn row(event: &ResearchEvent, theses: &[&Json]) -> Result<Json, String> {
    let (kind, t) = match event {
        ResearchEvent::ThesisProposed(t) => ("ThesisProposed", t),
        ResearchEvent::ThesisRevised(t) => ("ThesisRevised", t),
        ResearchEvent::UniverseChanged(c) => {
            let change = match c.change {
                UniverseChange::Admitted => "admitted",
                UniverseChange::Removed => "removed",
            };
            return Ok(json!({
                "type": "UniverseChanged", "instrument": c.instrument.as_str(), "change": change,
                "reason": c.reason.code(), "thesis_id": c.thesis_id.as_str(),
                "universe_size_after": c.universe_size_after,
            }));
        }
    };
    let stated = theses
        .iter()
        .find(|j| j.get("thesis_id").and_then(Json::as_str) == Some(t.thesis_id.as_str()))
        .and_then(|j| j.get("direction").and_then(Json::as_str));
    let direction = match t.direction {
        Direction::Long => Some("long"),
        Direction::Other => stated.filter(|d| *d != "long"),
    };
    let mut out = json!({
        "type": kind, "thesis_id": t.thesis_id.as_str(), "lineage_id": t.lineage_id.as_str(),
        "revision": t.revision, "instrument": t.instrument.as_str(),
        "direction": direction.ok_or("a non-long direction the thesis did not state")?,
        "horizon_s": t.horizon_s, "conviction": t.conviction.as_str(),
        "confidence": t.confidence.as_str(), "corroboration": t.corroboration.map(Corroboration::code),
        "admitted": t.admitted, "reason": t.reason.map(|r| r.code()),
    });
    if let (ResearchEvent::ThesisRevised(_), Some(members)) = (event, out.as_object_mut()) {
        let predecessor = t.predecessor_thesis_id.as_ref().map(ThesisId::as_str);
        members.insert("predecessor_thesis_id".to_owned(), predecessor.into());
    }
    Ok(out)
}

fn lineage_json(l: Lineage) -> Json {
    json!({ "revisions": l.revisions, "admitted": l.admitted, "retired": l.retired })
}

fn strings(value: &Json, key: &str) -> Result<Vec<String>, String> {
    let listed = list_at(value, key)?
        .iter()
        .map(|s| s.as_str().map(str::to_owned));
    listed
        .collect::<Option<_>>()
        .ok_or_else(|| format!("`{key}` holds a non-string"))
}

fn assets(value: &Json, key: &str) -> Result<BTreeSet<AssetId>, String> {
    strings(value, key)?.iter().map(|a| asset(a)).collect()
}

fn sources(value: &Json, key: &str) -> Result<Vec<SourceId>, String> {
    let listed = strings(value, key)?;
    listed
        .iter()
        .map(|s| SourceId::new(s).map_err(|e| e.code().to_owned()))
        .collect()
}

fn asset(id: &str) -> Result<AssetId, String> {
    AssetId::new(id).map_err(|e| e.to_string())
}

fn thesis_id(id: &str) -> Result<ThesisId, String> {
    ThesisId::new(id).map_err(|e| e.code().to_owned())
}

fn lineage_id(id: &str) -> Result<LineageId, String> {
    LineageId::new(id).map_err(|e| e.code().to_owned())
}

fn asset_class(name: &str) -> Result<AssetClass, String> {
    match name {
        "us_equity" => Ok(AssetClass::UsEquity),
        "crypto" => Ok(AssetClass::Crypto),
        other => Err(format!(
            "`{other}` is not an asset class family N interprets"
        )),
    }
}

/// A schema decimal, checked against the journal grammar before the crate carries its text.
fn decimal(text: &str) -> Result<SchemaDec, String> {
    DecStr::parse(text).map_err(|e| format!("`{text}`: {e}"))?;
    Ok(SchemaDec::from_checked_text(text))
}

fn digest(text: &str) -> Result<Digest, String> {
    let hex = text.strip_prefix("sha256:").and_then(Digest::from_hex);
    hex.ok_or_else(|| format!("`{text}` is not a sha256: digest"))
}

fn usd(text: &str) -> Result<Usd, String> {
    Usd::parse(text).map_err(|e| format!("`{text}`: {e}"))
}

fn instant(text: &str) -> Result<UtcNanos, String> {
    UtcNanos::parse(text).map_err(|e| format!("`{text}`: {e}"))
}

fn bool_of(value: &Json, key: &str) -> Result<bool, String> {
    at(value, key)?
        .as_bool()
        .ok_or_else(|| format!("`{key}` is not a boolean"))
}

/// The harness's own oracle: a family-N case passes only because every expectation was compared.
/// Each test doctors one member of the real fixture and requires the case to fail, so a comparison
/// dropped from this module turns a doctored case green and fails here.
#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use mandate_num::Ratio;
    use mandate_spec::condition::{ConditionField, Facts, FieldKind};
    use serde_json::json;

    use crate::{Json, mandate, read_fixture};

    /// §6.3's fields, less the reserved `unusual_input` (V-018).
    const FIELDS: [ConditionField; 15] = [
        ConditionField::Purpose,
        ConditionField::OrderUsd,
        ConditionField::CombinedScore,
        ConditionField::Instrument,
        ConditionField::AssetClass,
        ConditionField::Session,
        ConditionField::FirstTradeInInstrument,
        ConditionField::NewInstrument,
        ConditionField::ThesisConfidence,
        ConditionField::Drawdown,
        ConditionField::DailyPnlFraction,
        ConditionField::PositionUsdAfter,
        ConditionField::GrossUsdAfter,
        ConditionField::BoughtTodayUsd,
        ConditionField::PositionPnlFraction,
    ];

    /// The cases that state a first order's autonomy decision.
    const CLASSIFIED: [&str; 3] = ["MC-N01", "MC-N14", "MC-N26"];
    /// Input members that are data maps, whose keys are ids rather than field names.
    const DATA_MAPS: [&str; 2] = ["instrument_groups", "lineages"];
    const PLANTED: &str = "zz_planted";

    fn fixture() -> Result<Json, String> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
        read_fixture(&dir, "mandate.json").map(Arc::unwrap_or_clone)
    }

    /// The family-N cases, every one of which this module interprets.
    fn family_n(fixture: &Json) -> Result<Vec<String>, String> {
        let ids: Vec<String> = crate::list_at(fixture, "cases")?
            .iter()
            .filter_map(|c| c["id"].as_str())
            .filter(|id| id.starts_with("MC-N"))
            .map(str::to_owned)
            .collect();
        crate::ensure(ids.len() == 28, || {
            format!("family N holds 28 cases, found {}", ids.len())
        })?;
        Ok(ids)
    }

    fn run(fixture: Json, id: &str) -> Result<(), String> {
        let wanted = format!("mandate::{id}");
        let case = mandate::cases(&Arc::new(fixture))
            .into_iter()
            .find(|c| c.id == wanted)
            .ok_or_else(|| format!("no case {wanted}"))?;
        (case.run)()
    }

    /// The fixture with one case's member at `pointer` rewritten by `doctor`.
    fn doctored(
        fixture: &Json,
        id: &str,
        pointer: &str,
        doctor: impl FnOnce(&mut Json),
    ) -> Result<Json, String> {
        let mut copy = fixture.clone();
        let case = copy
            .get_mut("cases")
            .and_then(Json::as_array_mut)
            .and_then(|cases| cases.iter_mut().find(|c| c["id"] == id))
            .ok_or_else(|| format!("no case {id}"))?;
        doctor(
            case.pointer_mut(pointer)
                .ok_or_else(|| format!("{id} has no {pointer}"))?,
        );
        Ok(copy)
    }

    /// Every JSON pointer under `root`, with the value found there.
    fn pointers(root: &Json, at: &str, out: &mut Vec<(String, Json)>) {
        out.push((at.to_owned(), root.clone()));
        match root {
            Json::Object(members) => members
                .iter()
                .for_each(|(k, v)| pointers(v, &format!("{at}/{k}"), out)),
            Json::Array(items) => items
                .iter()
                .enumerate()
                .for_each(|(i, v)| pointers(v, &format!("{at}/{i}"), out)),
            _ => {}
        }
    }

    /// A value that differs from `value` and keeps its JSON type where it has one.
    fn changed(value: &Json) -> Json {
        match value {
            Json::Bool(b) => Json::Bool(!b),
            Json::Number(n) => Json::from(n.as_u64().unwrap_or_default().saturating_add(1)),
            Json::String(s) => Json::String(format!("{s}x")),
            Json::Null => Json::String(PLANTED.to_owned()),
            Json::Array(items) if items.is_empty() => {
                Json::Array(vec![Json::String(PLANTED.to_owned())])
            }
            Json::Array(items) => Json::Array(items.iter().skip(1).cloned().collect()),
            Json::Object(_) => Json::Null,
        }
    }

    /// A doctored case must fail.
    fn must_fail(result: Result<(), String>, id: &str, what: &str) -> Result<(), String> {
        match result {
            Ok(()) => Err(format!(
                "{id}: {what} was doctored and the case still passed"
            )),
            Err(_) => Ok(()),
        }
    }

    #[test]
    fn every_interpreted_case_passes() -> Result<(), String> {
        let fixture = fixture()?;
        for id in family_n(&fixture)? {
            run(fixture.clone(), &id).map_err(|e| format!("{id}: {e}"))?;
        }
        Ok(())
    }

    /// Each of the three first orders reaches `classify` with every fact in its own §6.3 field: the
    /// stated `admission_action` members, `purpose` `open`, and `new_instrument` and
    /// `thesis_confidence` from the facts admission reported, read back through the `Facts`
    /// projection a rule reads. The reported facts here are `false` and a confidence no stated
    /// member holds (the fixture's thesis confidence equals its `combined_score`), so a harness that
    /// fixed `new_instrument` or took the confidence from another field fails, even where no base's
    /// rules would notice.
    #[test]
    fn every_first_order_fact_reaches_its_own_field() -> Result<(), String> {
        const CONFIDENCE: &str = "0.31";
        let fixture = fixture()?;
        let mut compared = 0_usize;
        for id in CLASSIFIED {
            let case = crate::list_at(&fixture, "cases")?
                .iter()
                .find(|c| c.get("id").and_then(Json::as_str) == Some(id))
                .ok_or("the case")?;
            let stated = crate::at(case, "input.admission_action")?;
            crate::ensure(
                stated
                    .as_object()
                    .is_some_and(|m| m.values().all(|v| v != CONFIDENCE)),
                || format!("{id}: a stated member holds {CONFIDENCE}"),
            )?;
            let first_order = mandate_research::FirstOrderFacts {
                new_instrument: false,
                thesis_confidence: mandate_research::SchemaDec::from_checked_text(CONFIDENCE),
                admission_ceiling: mandate_research::AutonomyDecision::Ask,
            };
            let facts = super::first_order_action(stated, &first_order)?;
            let mut wanted = stated.clone();
            let members = wanted.as_object_mut().ok_or("an action object")?;
            members.remove("unusual_input");
            members.insert("purpose".to_owned(), json!("open"));
            members.insert("new_instrument".to_owned(), json!(false));
            members.insert("thesis_confidence".to_owned(), json!(CONFIDENCE));
            crate::expect_eq("the facts stated", members.len(), FIELDS.len())?;
            for field in FIELDS {
                let name = field.as_str();
                let want = members.get(name).ok_or_else(|| format!("{id}: {name}"))?;
                let same = match field.kind() {
                    FieldKind::Bool => facts.bool_field(field) == want.as_bool(),
                    FieldKind::Enum | FieldKind::Text => facts.enum_field(field) == want.as_str(),
                    FieldKind::Decimal => {
                        let text = want.as_str().ok_or_else(|| format!("{id}: {name}"))?;
                        facts.decimal_field(field)
                            == Some(Ratio::parse(text).map_err(|e| format!("{id}: {e}"))?)
                    }
                };
                crate::ensure(same, || {
                    format!("{id}: `{name}` did not reach its own field")
                })?;
                compared = compared.saturating_add(1);
            }
        }
        crate::expect_eq("facts compared", compared, 3 * FIELDS.len())
    }

    /// A first order whose `unusual_input` is true fails naming it, rather than being classified as
    /// if the reserved field were false.
    #[test]
    fn an_unusual_first_order_input_fails_naming_it() -> Result<(), String> {
        let fixture = fixture()?;
        for id in CLASSIFIED {
            let doctored = doctored(&fixture, id, "/input/admission_action/unusual_input", |v| {
                *v = Json::Bool(true);
            })?;
            match run(doctored, id) {
                Err(e) if e.contains("unusual_input") => {}
                other => return Err(format!("{id}: an unusual input gave {other:?}")),
            }
        }
        Ok(())
    }

    /// Every member of every expectation, at every depth, is compared: changing a leaf, shortening
    /// a list, emptying an object, dropping a member, or adding one fails the case.
    #[test]
    fn every_expected_member_is_compared() -> Result<(), String> {
        let fixture = fixture()?;
        let mut doctorings = 0_usize;
        let ids = family_n(&fixture)?;
        let cases = ids.len();
        for id in ids {
            let expect = run_expect(&fixture, &id)?;
            let mut all = Vec::new();
            pointers(&expect, "/expect", &mut all);
            for (pointer, value) in all.iter().skip(1) {
                let replaced = doctored(&fixture, &id, pointer, |v| *v = changed(value))?;
                must_fail(run(replaced, &id), &id, pointer)?;
                let (parent, key) = pointer.rsplit_once('/').ok_or("a pointer has a parent")?;
                let removed = doctored(&fixture, &id, parent, |p| {
                    if let Some(members) = p.as_object_mut() {
                        members.remove(key);
                    } else if let Some(items) = p.as_array_mut() {
                        key.parse::<usize>()
                            .ok()
                            .filter(|i| *i < items.len())
                            .map(|i| items.remove(i));
                    }
                })?;
                must_fail(run(removed, &id), &id, &format!("{pointer} (removed)"))?;
                if value.is_object() {
                    let grown = doctored(&fixture, &id, pointer, |v| {
                        if let Some(members) = v.as_object_mut() {
                            members.insert(PLANTED.to_owned(), Json::Null);
                        }
                    })?;
                    must_fail(run(grown, &id), &id, &format!("{pointer}/{PLANTED}"))?;
                }
                doctorings = doctorings.saturating_add(2);
            }
        }
        crate::ensure(doctorings > cases, || {
            format!("{doctorings} doctorings for {cases} cases")
        })
    }

    /// Every field object a case's `input` holds rejects a member the harness does not read, and the
    /// failure names it (DEC-85). Data maps are keyed by ids, so their members are data, not fields;
    /// the objects inside them are swept.
    #[test]
    fn every_input_object_rejects_an_unknown_member() -> Result<(), String> {
        let fixture = fixture()?;
        let mut swept = 0_usize;
        let ids = family_n(&fixture)?;
        let cases = ids.len();
        for id in ids {
            let case = crate::list_at(&fixture, "cases")?
                .iter()
                .find(|c| c["id"] == id.as_str())
                .ok_or("the case")?;
            let mut all = Vec::new();
            pointers(crate::at(case, "input")?, "/input", &mut all);
            for (pointer, _) in all.iter().filter(|(p, v)| {
                v.is_object() && !DATA_MAPS.iter().any(|m| p.ends_with(&format!("/{m}")))
            }) {
                let planted = doctored(&fixture, &id, pointer, |v| {
                    if let Some(members) = v.as_object_mut() {
                        members.insert(PLANTED.to_owned(), Json::Null);
                    }
                })?;
                match run(planted, &id) {
                    Err(e) if e.contains(PLANTED) => swept = swept.saturating_add(1),
                    other => return Err(format!("{id}{pointer}: an unread member gave {other:?}")),
                }
            }
        }
        crate::ensure(swept >= cases, || {
            format!("{swept} input objects swept for {cases} cases")
        })
    }

    fn run_expect(fixture: &Json, id: &str) -> Result<Json, String> {
        crate::list_at(fixture, "cases")?
            .iter()
            .find(|c| c["id"] == id)
            .and_then(|c| c.get("expect"))
            .cloned()
            .ok_or_else(|| format!("{id} has no expect"))
    }

    /// A repeated instrument fails on either side rather than collapsing into the crate's set
    /// (MI-15), so a duplicate can never make two lists compare equal.
    #[test]
    fn a_repeated_instrument_fails_on_either_side() -> Result<(), String> {
        let fixture = fixture()?;
        let pairs = [
            ("MC-N22", "/expect/working_universe"),
            ("MC-N02", "/input/working_universe"),
            ("MC-N02", "/expect/working_universe"),
        ];
        for (id, pointer) in pairs {
            let repeated = doctored(&fixture, id, pointer, |v| {
                if let Some(items) = v.as_array_mut() {
                    let first = items.first().cloned().unwrap_or_default();
                    items.push(first);
                }
            })?;
            match run(repeated, id) {
                Err(e) if e.contains("MI-15") => {}
                other => return Err(format!("{id}{pointer}: a repeat gave {other:?}")),
            }
        }
        Ok(())
    }

    /// A stagger pair is exactly a workspace and a thesis: a third member would be an input no
    /// offset reads (DEC-85).
    #[test]
    fn a_stagger_pair_is_exactly_two_ids() -> Result<(), String> {
        let fixture = fixture()?;
        let grown = doctored(&fixture, "MC-N23", "/input/pairs/0", |v| {
            if let Some(items) = v.as_array_mut() {
                items.push(Json::String(PLANTED.to_owned()));
            }
        })?;
        match run(grown, "MC-N23") {
            Err(e) if e.contains(PLANTED) => Ok(()),
            other => Err(format!("a three-member pair gave {other:?}")),
        }
    }
}
