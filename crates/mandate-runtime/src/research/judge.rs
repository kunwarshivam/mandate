//! Judging a research call's output (agent harness spec §6.2 steps 4 and 5, §6.5; E19-4): the
//! strict parse, the harness checks that need platform data, the platform's fill of every fact the
//! model may not state (HI-18), and `mandate_research`'s seventeen ordered checks. The verdicts go
//! in the call's own `ModelInvocationRecorded` (journal spec §9.19 rule 151), which heads the
//! judging batch, so the record precedes every thesis record its output leads to (INF-10, HI-22).

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::InstrumentId;
use mandate_canon::{Digest, Value, to_canonical};
use mandate_gateway::{CallRecord, CandidateKind, Verdict, VerdictResult};
use mandate_num::Conviction;
use mandate_research::{
    AdmissionFacts, AssetClass, AssetId, ContentHash, Corroboration, Direction, FoldInput, GroupId,
    InstrumentFacts, Invalidation, LineageId, ModelId, ModelVersion, OutputEnvelope,
    ProposedThesis, RefusalReason, ResearchEvent, SchemaDec, SourceId as ResearchSource, Thesis,
    ThesisEntry, ThesisId, WorkingUniverse, fold_theses,
};
use mandate_research_run::agentic::RunKind;
use mandate_research_run::{CandidateThesis, InstrumentFact, ItemKind, Symbol, parse_output};
use mandate_time::UtcNanos;

use super::{Called, ItemFacts, ResearchConfig, ResearchContext, Run};
use crate::error::RuntimeError;
use crate::payload;
use crate::step::Batch;
use crate::types::{EventId, ModelDirection, ModelOutput, ModelOutputIgnored, RiskClock};

/// The ordinals thesis ids are derived at: far above any draft's ordinal in a batch and far below
/// the run and call ids' (`super::polled`), so no derived id meets another.
const THESIS_ORDINALS: u32 = 1 << 30;

/// Phrases that state a price target or a likely profit (DEC-126). The research agent may be
/// directional (§8.1), so "buy" alone is not here. The gateway's output limits are the first layer;
/// this is the second (§6.5), and it errs toward refusing. Whitespace is collapsed before matching;
/// abbreviations and look-alike characters still pass, a residual DEC-996 item 7 discloses, which
/// HI-10 bounds: a thesis that passes still cannot size, approve, or place anything.
const FORBIDDEN: [&str; 16] = [
    "price target",
    "target price",
    "price objective",
    "guarantee",
    "risk-free",
    "risk free",
    "can't lose",
    "cannot lose",
    "sure thing",
    "will return",
    "will double",
    "will triple",
    "% upside",
    "% return",
    "expected profit",
    "likely profit",
];

fn forbidden(text: &str) -> bool {
    let lower = text
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    FORBIDDEN.iter().any(|phrase| lower.contains(phrase))
}

/// One retrieved item as judging reads it: its observation and its facts.
struct Cited<'a> {
    observation: &'a EventId,
    facts: &'a ItemFacts,
}

/// One thesis candidate that passed the harness checks, with everything the platform filled.
struct Passed<'a> {
    candidate: &'a CandidateThesis,
    proposal: ProposedThesis,
    evidence: Vec<EventId>,
}

/// A thesis record the judging batch journals after the call record, with the research agent's
/// §8.2 output when admission admits it or checks 1 to 3 refuse it.
pub(super) struct ThesisDraft {
    pub(super) event_type: &'static str,
    pub(super) body: Value,
    pub(super) output: Option<Value>,
}

/// What judging decided: one verdict per candidate in response order, theses first (rule 151), the
/// accepted invalidations' `ThesisInvalidated` bodies, and the thesis records to journal. All are
/// empty when nothing can be judged, and `refused` says the output was there and the strict read
/// refused it, as one outside its run kind's schema is.
#[derive(Default)]
pub(super) struct Judged {
    pub(super) verdicts: Vec<Verdict>,
    pub(super) invalidations: Vec<Value>,
    pub(super) theses: Vec<ThesisDraft>,
    pub(super) refused: bool,
}

/// Judges the `ok` record of `run`'s call, a `kind` run's, under the version in effect (HI-17), its
/// output read as that kind's schema pins it (§6.9). An output the
/// harness's strict read refuses, or one whose candidates the platform cannot fill or admission
/// cannot decide, is judged as nothing: no verdict and no thesis record, so nothing reaches the
/// order path (HI-6, DEC-1120 item 4).
///
/// # Errors
/// [`RuntimeError`] when a value cannot be written.
pub(super) fn judge(
    run: &Run,
    called: &Called<'_>,
    record: &CallRecord,
    config: &ResearchConfig,
    context: &ResearchContext<'_>,
    batch: &Batch<'_>,
    kind: RunKind,
) -> Result<Judged, RuntimeError> {
    let (Some(output), Some(prompt), Some(response)) = (
        record.output.as_ref(),
        record.prompt_ref,
        record.response_ref,
    ) else {
        return Ok(Judged::default());
    };
    let Ok(output) = parse_output(&to_canonical(output), &config.bounds, kind) else {
        return Ok(Judged {
            refused: true,
            ..Judged::default()
        });
    };
    let refs = Refs { prompt, response };
    let first_invalidation = output.theses.len();
    let mut ended = BTreeSet::new();
    let mut invalidation_results = Vec::new();
    let mut invalidations = Vec::new();
    for (index, candidate) in output.invalidations.iter().enumerate() {
        let thesis = context
            .active_theses
            .iter()
            .find(|active| active.thesis_id == candidate.thesis_id);
        let Some(thesis) = thesis else {
            invalidation_results.push(VerdictResult::UnknownThesis);
            continue;
        };
        ended.insert(thesis.thesis_id.as_str());
        invalidation_results.push(VerdictResult::Judged);
        invalidations.push(invalidation_record(
            thesis,
            first_invalidation.saturating_add(index),
            &candidate.evidence,
        )?);
    }
    let left: WorkingUniverse = match context.universe {
        WorkingUniverse::Known {
            instruments,
            pinned,
        } => WorkingUniverse::Known {
            instruments: instruments
                .iter()
                .filter(|instrument| {
                    !context.active_theses.iter().any(|active| {
                        ended.contains(active.thesis_id.as_str())
                            && active.instrument == **instrument
                    })
                })
                .cloned()
                .collect(),
            pinned: *pinned,
        },
        WorkingUniverse::Unavailable => WorkingUniverse::Unavailable,
    };
    let cited: BTreeMap<&str, Cited<'_>> = called
        .observations
        .iter()
        .zip(called.items)
        .map(|(observation, facts)| (facts.item_id.as_str(), Cited { observation, facts }))
        .collect();
    let snapshot: BTreeMap<&Symbol, &InstrumentFact> = called
        .items
        .iter()
        .filter_map(|item| item.instrument.as_ref())
        .map(|fact| (&fact.symbol, fact))
        .collect();
    let mut results = Vec::new();
    let mut passed = Vec::new();
    let mut named = BTreeSet::new();
    for (index, candidate) in output.theses.iter().enumerate() {
        let fact = Symbol::new(&candidate.instrument)
            .ok()
            .and_then(|symbol| snapshot.get(&symbol).copied());
        match (refusal(candidate, fact, &cited, &mut named, config), fact) {
            (None, Some(fact)) => {
                let ordinal = THESIS_ORDINALS.saturating_add(u32::try_from(index).unwrap_or(0));
                let thesis_id = batch.id_at(ordinal);
                let Ok(filled) = fill(
                    candidate, fact, &cited, &thesis_id, run, config, context, &ended,
                ) else {
                    return Ok(Judged::default());
                };
                passed.push(filled);
                results.push(VerdictResult::Judged);
            }
            (refused, _) => results.push(refused.unwrap_or(VerdictResult::UnknownInstrument)),
        }
    }
    let Ok(facts) = platform_facts(context.facts, &snapshot) else {
        return Ok(Judged::default());
    };
    let proposals: Vec<ProposedThesis> = passed.iter().map(|p| p.proposal.clone()).collect();
    let fold = fold_theses(&FoldInput {
        mandate: context.mandate,
        overlay: context.overlay,
        universe: &left,
        proposals: &proposals,
        facts: &facts,
        lineages: context.lineages,
    });
    let Ok(fold) = fold else {
        return Ok(Judged::default());
    };
    let mut theses = Vec::new();
    for (accepted, step) in passed.iter().zip(&fold.steps) {
        let entry = step
            .journal
            .iter()
            .find_map(|event| match event {
                ResearchEvent::ThesisProposed(entry) | ResearchEvent::ThesisRevised(entry) => {
                    Some(entry)
                }
                ResearchEvent::UniverseChanged(_) => None,
            })
            .ok_or_else(|| payload::non_canonical("thesis"))?;
        let event_type = if entry.revision > 0 {
            "ThesisRevised"
        } else {
            "ThesisProposed"
        };
        let ignored = match step.decision.reason() {
            None => Some(None),
            Some(RefusalReason::DirectionNotAllowed) => {
                Some(Some(ModelOutputIgnored::DirectionNotAllowed))
            }
            Some(RefusalReason::HorizonMismatch) => Some(Some(ModelOutputIgnored::HorizonMismatch)),
            Some(RefusalReason::RevisionWithoutPredecessor) => {
                Some(Some(ModelOutputIgnored::RevisionWithoutPredecessor))
            }
            Some(_) => None,
        };
        let output = match ignored {
            Some(ignored) => Some(payload::model_output(&model_output(
                entry,
                accepted,
                run,
                refs.response,
                ignored,
            )?)?),
            None => None,
        };
        theses.push(ThesisDraft {
            event_type,
            body: thesis_record(entry, accepted, &accepted.proposal.thesis.output, &refs)?,
            output,
        });
    }
    let verdicts = results
        .into_iter()
        .map(|result| Verdict {
            kind: CandidateKind::Thesis,
            result,
        })
        .chain(invalidation_results.into_iter().map(|result| Verdict {
            kind: CandidateKind::Invalidation,
            result,
        }))
        .collect();
    Ok(Judged {
        verdicts,
        invalidations,
        theses,
        refused: false,
    })
}

/// An accepted invalidation's `ThesisInvalidated` body (journal spec §9.22): the thesis, its
/// lineage, and its instrument as the run's inputs held them, the verdict's index in the call
/// record's `verdicts`, and the evidence as the output gave it. The model is its cause; the record's
/// `causation_id` is the call record, which heads the batch.
fn invalidation_record(
    thesis: &super::ActiveThesis,
    candidate: usize,
    evidence: &[String],
) -> Result<Value, RuntimeError> {
    payload::object(vec![
        ("thesis_id", payload::text(&thesis.thesis_id)),
        ("lineage_id", payload::text(&thesis.lineage_id)),
        ("instrument_id", payload::text(thesis.instrument.as_str())),
        ("cause", payload::text("model")),
        (
            "candidate",
            payload::count(u64::try_from(candidate).unwrap_or(u64::MAX), "candidate")?,
        ),
        (
            "evidence",
            Value::Array(evidence.iter().map(|id| payload::text(id)).collect()),
        ),
        ("source", Value::Null),
    ])
}

/// The artifacts a judged call's thesis records name.
struct Refs {
    prompt: Digest,
    response: Digest,
}

/// §6.5's harness checks in the table's order; the first that fails decides.
fn refusal(
    candidate: &CandidateThesis,
    fact: Option<&InstrumentFact>,
    cited: &BTreeMap<&str, Cited<'_>>,
    named: &mut BTreeSet<Symbol>,
    config: &ResearchConfig,
) -> Option<VerdictResult> {
    let Some(fact) = fact else {
        return Some(VerdictResult::UnknownInstrument);
    };
    if candidate
        .evidence
        .iter()
        .any(|id| !cited.contains_key(id.as_str()))
    {
        return Some(VerdictResult::UnknownEvidence);
    }
    if !named.insert(fact.symbol.clone()) {
        return Some(VerdictResult::DuplicateInstrument);
    }
    if forbidden(&candidate.thesis) || forbidden(&candidate.invalidation) {
        return Some(VerdictResult::ForbiddenLanguage);
    }
    if candidate.predecessor_thesis_id.is_some() && !config.revisions_enabled {
        return Some(VerdictResult::RevisionsNotEnabled);
    }
    None
}

fn research_class(class: mandate_domain::AssetClass) -> AssetClass {
    match class {
        mandate_domain::AssetClass::UsEquity => AssetClass::UsEquity,
        mandate_domain::AssetClass::Crypto => AssetClass::Crypto,
    }
}

fn not_canonical(field: &str) -> impl Fn(mandate_research::ResearchError) -> RuntimeError + '_ {
    move |_| payload::non_canonical(field)
}

/// The platform's half of §6.5's field split (HI-18): identity, timing, asset class, sources, and
/// corroboration from platform data; direction, conviction, confidence, horizon, and text from the
/// model.
///
/// Corroboration (§13 item 3, DEC-996 item 6) is `independent_source` exactly when the cited items
/// about this instrument come from at least two distinct allowlisted text sources; otherwise none,
/// so §8.5 check 15 refuses. Market-data corroboration waits for E17-7's rule.
#[allow(
    clippy::too_many_arguments,
    reason = "one thesis's fill reads the candidate, the snapshot fact, the cited items, its id, the run, the pin, and the context"
)]
fn fill<'a>(
    candidate: &'a CandidateThesis,
    fact: &InstrumentFact,
    cited: &BTreeMap<&str, Cited<'_>>,
    thesis_id: &EventId,
    run: &Run,
    config: &ResearchConfig,
    context: &ResearchContext<'_>,
    ended: &BTreeSet<&str>,
) -> Result<Passed<'a>, RuntimeError> {
    let instrument =
        AssetId::new(fact.asset_id.as_str()).map_err(|_| payload::non_canonical("asset_id"))?;
    let as_of = UtcNanos::from_parts(run.cut_off.secs(), 0)?;
    let expires_at = UtcNanos::from_parts(
        run.cut_off
            .secs()
            .checked_add(i64::from(candidate.horizon_s))
            .ok_or_else(|| payload::non_canonical("expires_at"))?,
        0,
    )?;
    let active = context.active_theses.iter().find(|thesis| {
        thesis.instrument == instrument && !ended.contains(thesis.thesis_id.as_str())
    });
    let (lineage, revision, predecessor) = match active {
        Some(thesis) => (
            thesis.lineage_id.clone(),
            thesis.revision,
            (thesis.revision > 0).then(|| thesis.thesis_id.clone()),
        ),
        None => (thesis_id.0.clone(), 0, None),
    };
    let mut evidence = Vec::new();
    let mut sources = Vec::new();
    let mut independent = BTreeSet::new();
    for id in &candidate.evidence {
        let Some(item) = cited.get(id.as_str()) else {
            continue;
        };
        evidence.push(item.observation.clone());
        if matches!(item.facts.kind, ItemKind::News | ItemKind::Filing) {
            sources.push(
                ResearchSource::new(item.facts.source.as_str()).map_err(not_canonical("source"))?,
            );
            if item.facts.symbols.contains(&fact.symbol)
                && context
                    .facts
                    .allowlist
                    .sources
                    .iter()
                    .any(|allowed| allowed.as_str() == item.facts.source.as_str())
            {
                independent.insert(item.facts.source.as_str());
            }
        }
    }
    let thesis = Thesis {
        thesis_id: ThesisId::new(&thesis_id.0).map_err(not_canonical("thesis_id"))?,
        lineage_id: LineageId::new(&lineage).map_err(not_canonical("lineage_id"))?,
        revision,
        predecessor_thesis_id: predecessor
            .as_deref()
            .map(ThesisId::new)
            .transpose()
            .map_err(not_canonical("predecessor_thesis_id"))?,
        instrument_id: instrument,
        output: OutputEnvelope {
            model_id: ModelId::new(&config.pin.model_id)
                .map_err(|_| payload::non_canonical("model_id"))?,
            model_version: ModelVersion::new(&config.pin.model_version),
            content_hash: ContentHash::new(config.pin.content_hash),
            as_of,
            expires_at,
        },
        direction: if candidate.direction == "long" {
            Direction::Long
        } else {
            Direction::Other
        },
        horizon_s: candidate.horizon_s,
        conviction: SchemaDec::from_checked_text(&candidate.conviction.to_string()),
        confidence: SchemaDec::from_checked_text(&candidate.confidence.to_string()),
        evidence: None,
        evidence_sources: sources,
        invalidation: Invalidation::new(&candidate.invalidation)
            .map_err(not_canonical("invalidation"))?,
    };
    Ok(Passed {
        candidate,
        proposal: ProposedThesis {
            thesis,
            instrument: InstrumentFacts {
                asset_class: research_class(fact.asset_class),
                leveraged_or_inverse_etp: fact.leveraged_or_inverse_etp,
            },
            corroboration: (independent.len() >= 2).then_some(Corroboration::IndependentSource),
        },
        evidence,
    })
}

/// The admission facts with the snapshot's own verdicts added: an instrument the snapshot marks
/// ineligible fails the floor, and its group binds, whatever the caller's facts said. Adding only
/// ever refuses more (DEC-176).
fn platform_facts(
    facts: &AdmissionFacts,
    snapshot: &BTreeMap<&Symbol, &InstrumentFact>,
) -> Result<AdmissionFacts, RuntimeError> {
    let mut merged = facts.clone();
    for fact in snapshot.values() {
        let asset =
            AssetId::new(fact.asset_id.as_str()).map_err(|_| payload::non_canonical("asset_id"))?;
        if !fact.eligible {
            merged.eligibility_failures.insert(asset.clone());
        }
        if let Some(group) = &fact.group {
            let group = GroupId::new(group).map_err(|_| payload::non_canonical("group"))?;
            merged.instrument_groups.entry(asset).or_insert(group);
        }
    }
    Ok(merged)
}

/// `ThesisProposed` or `ThesisRevised` with journal spec §9.4's members.
fn thesis_record(
    entry: &ThesisEntry,
    accepted: &Passed<'_>,
    output: &OutputEnvelope,
    refs: &Refs,
) -> Result<Value, RuntimeError> {
    let reference = |digest: &Digest| payload::text(&format!("sha256:{digest}"));
    payload::object(vec![
        ("model_id", payload::text(output.model_id.as_str())),
        (
            "model_version",
            payload::text(output.model_version.as_str()),
        ),
        ("content_hash", reference(output.content_hash.digest())),
        ("thesis_id", payload::text(entry.thesis_id.as_str())),
        ("lineage_id", payload::text(entry.lineage_id.as_str())),
        (
            "revision",
            payload::count(u64::from(entry.revision), "revision")?,
        ),
        (
            "predecessor_thesis_id",
            entry
                .predecessor_thesis_id
                .as_ref()
                .map_or(Value::Null, |id| payload::text(id.as_str())),
        ),
        ("autopsy_ref", Value::Null),
        ("instrument_id", payload::text(entry.instrument.as_str())),
        (
            "asset_class",
            payload::text(match entry.asset_class {
                AssetClass::UsEquity => "us_equity",
                AssetClass::Crypto => "crypto",
            }),
        ),
        ("direction", payload::text(&accepted.candidate.direction)),
        ("as_of", payload::text(&output.as_of.to_string())),
        ("expires_at", payload::text(&output.expires_at.to_string())),
        (
            "horizon_s",
            payload::count(u64::from(entry.horizon_s), "horizon_s")?,
        ),
        ("conviction", payload::text(entry.conviction.as_str())),
        ("confidence", payload::text(entry.confidence.as_str())),
        ("evidence_ref", Value::Null),
        (
            "evidence_sources",
            Value::Array(
                entry
                    .evidence_sources
                    .iter()
                    .map(|source| payload::text(source.as_str()))
                    .collect(),
            ),
        ),
        (
            "corroboration",
            entry
                .corroboration
                .map_or(Value::Null, |kind| payload::text(kind.code())),
        ),
        ("invalidation", payload::text(entry.invalidation.as_str())),
        (
            "allowlist_version",
            payload::count(u64::from(entry.allowlist_version.0), "allowlist_version")?,
        ),
        ("prompt_ref", reference(&refs.prompt)),
        ("response_ref", reference(&refs.response)),
        ("admitted", Value::Bool(entry.admitted)),
        (
            "reason",
            entry
                .reason
                .map_or(Value::Null, |reason| payload::text(reason.code())),
        ),
    ])
}

/// The research agent's §8.2 output for a judged thesis: the order builder's input for an
/// admitted one, `ignored` for one checks 1 to 3 refuse.
fn model_output(
    entry: &ThesisEntry,
    accepted: &Passed<'_>,
    run: &Run,
    response_ref: Digest,
    ignored: Option<ModelOutputIgnored>,
) -> Result<ModelOutput, RuntimeError> {
    let output = &accepted.proposal.thesis.output;
    let candidate = accepted.candidate;
    Ok(ModelOutput {
        model_id: output.model_id.as_str().to_owned(),
        model_version: output.model_version.as_str().to_owned(),
        content_hash: *output.content_hash.digest(),
        instrument_id: InstrumentId::new(entry.instrument.as_str())
            .map_err(|_| payload::non_canonical("instrument_id"))?,
        as_of: run.cut_off,
        expires_at: RiskClock::from_secs(output.expires_at.secs()),
        direction: if candidate.direction == "long" {
            ModelDirection::Long
        } else {
            ModelDirection::Other(candidate.direction.clone())
        },
        conviction: Conviction::parse(&candidate.conviction.to_string())?,
        confidence: candidate.confidence,
        horizon_s: u64::from(candidate.horizon_s),
        thesis_ref: Some(response_ref),
        evidence: accepted.evidence.clone(),
        invalidation: Some(candidate.invalidation.clone()),
        thesis_id: Some(entry.thesis_id.as_str().to_owned()),
        lineage_id: Some(entry.lineage_id.as_str().to_owned()),
        ignored,
    })
}
