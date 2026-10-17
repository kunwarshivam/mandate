//! Judging a committed research call (agent harness spec §6.2 step 5, §6.5; E19-4): the strict
//! parse, the harness checks that need platform data, the platform's fill of every fact the model
//! may not state (HI-18), `mandate_research`'s seventeen ordered checks, and the one batch that
//! journals every candidate's verdict (HI-22).

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::InstrumentId;
use mandate_canon::Value;
use mandate_num::Conviction;
use mandate_research::{
    AdmissionChange, AdmissionDecision, AdmissionFacts, AssetClass, AssetId, ContentHash,
    Corroboration, Direction, FoldInput, GroupId, InstrumentFacts, Invalidation, LineageId,
    ModelId, ModelVersion, OutputEnvelope, ProposedThesis, RefusalReason, ResearchEvent, SchemaDec,
    SourceId as ResearchSource, Thesis, ThesisEntry, ThesisId, fold_theses,
};
use mandate_research_run::{CandidateThesis, InstrumentFact, ItemKind, Symbol, parse_output};
use mandate_time::UtcNanos;

use super::{ItemFacts, Phase, ResearchConfig, ResearchContext, Run};
use crate::error::RuntimeError;
use crate::payload;
use crate::step::Batch;
use crate::types::{EventId, ModelDirection, ModelOutput, ModelOutputIgnored, RiskClock};

/// The ordinals thesis ids are derived at: far above any draft's ordinal in a batch and far below
/// the run and call ids' (`super::polled`), so no derived id meets another.
const THESIS_ORDINALS: u32 = 1 << 30;

/// A harness refusal (§6.5's table), journaled among the verdicts and never as a §8.5 reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HarnessRefusal {
    UnknownInstrument,
    UnknownEvidence,
    DuplicateInstrument,
    ForbiddenLanguage,
    RevisionsNotEnabled,
    UnknownThesis,
}

impl HarnessRefusal {
    fn code(self) -> &'static str {
        match self {
            Self::UnknownInstrument => "unknown_instrument",
            Self::UnknownEvidence => "unknown_evidence",
            Self::DuplicateInstrument => "duplicate_instrument",
            Self::ForbiddenLanguage => "forbidden_language",
            Self::RevisionsNotEnabled => "revisions_not_enabled",
            Self::UnknownThesis => "unknown_thesis",
        }
    }
}

/// Phrases that state a price target or a likely profit (DEC-126). The research agent may be
/// directional (§8.1), so "buy" alone is not here. The gateway's output limits are the first layer;
/// this is the second (§6.5), and it errs toward refusing.
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
    let lower = text.to_lowercase();
    FORBIDDEN.iter().any(|phrase| lower.contains(phrase))
}

/// One retrieved item as judging reads it: its observation and its facts.
struct Cited<'a> {
    observation: &'a EventId,
    facts: &'a ItemFacts,
}

/// One thesis candidate that passed the harness checks, with everything the platform filled.
struct Passed<'a> {
    index: usize,
    candidate: &'a CandidateThesis,
    proposal: ProposedThesis,
    evidence: Vec<EventId>,
}

/// One verdict line of the verdict record.
struct Verdict {
    candidate: &'static str,
    index: usize,
    subject: String,
    verdict: String,
    thesis_id: Option<String>,
}

/// Judges `run`, whose `ok` call record has committed, under the version in effect (HI-17), and
/// journals one batch: each judged thesis's record, the output of each admitted one and of each
/// checks 1 to 3 refuse, and the verdict record of every candidate.
///
/// # Errors
/// [`RuntimeError`] when a value cannot be written, or `mandate_research` cannot decide.
pub(super) fn judge(
    run: &Run,
    config: &ResearchConfig,
    context: &ResearchContext<'_>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    let Phase::Recorded {
        invocation,
        reply,
        observations,
        items,
        ..
    } = &run.phase
    else {
        return Ok(());
    };
    let output = match parse_output(&reply.output, &config.bounds) {
        Ok(output) => output,
        Err(rejection) => {
            let body = verdicts(
                run,
                invocation,
                "output_rejected",
                Some(rejection.code()),
                &[],
            )?;
            batch.journal("CandidateVerdictsRecorded", Some(invocation.clone()), body)?;
            return Ok(());
        }
    };
    if reply.reported != config.pin {
        let lines: Vec<Verdict> = output
            .theses
            .iter()
            .enumerate()
            .map(|(index, thesis)| Verdict {
                candidate: "thesis",
                index,
                subject: thesis.instrument.clone(),
                verdict: "not_pinned".to_owned(),
                thesis_id: None,
            })
            .chain(
                output
                    .invalidations
                    .iter()
                    .enumerate()
                    .map(|(index, invalidation)| Verdict {
                        candidate: "invalidation",
                        index,
                        subject: invalidation.thesis_id.clone(),
                        verdict: "not_pinned".to_owned(),
                        thesis_id: None,
                    }),
            )
            .collect();
        let body = verdicts(run, invocation, "not_pinned", None, &lines)?;
        batch.journal("CandidateVerdictsRecorded", Some(invocation.clone()), body)?;
        return Ok(());
    }
    let cited: BTreeMap<&str, Cited<'_>> = observations
        .iter()
        .zip(items)
        .map(|(observation, facts)| (facts.item_id.as_str(), Cited { observation, facts }))
        .collect();
    let snapshot: BTreeMap<&Symbol, &InstrumentFact> = items
        .iter()
        .filter_map(|item| item.instrument.as_ref())
        .map(|fact| (&fact.symbol, fact))
        .collect();
    let mut lines = Vec::new();
    let mut passed = Vec::new();
    let mut named = BTreeSet::new();
    for (index, candidate) in output.theses.iter().enumerate() {
        let fact = Symbol::new(&candidate.instrument)
            .ok()
            .and_then(|symbol| snapshot.get(&symbol).copied());
        let refusal = refusal(candidate, fact, &cited, &mut named, config);
        match (refusal, fact) {
            (None, Some(fact)) => {
                let ordinal = THESIS_ORDINALS.saturating_add(u32::try_from(index).unwrap_or(0));
                let thesis_id = batch.id_at(ordinal);
                passed.push(fill(
                    index, candidate, fact, &cited, &thesis_id, run, config, context,
                )?);
            }
            (refused, _) => lines.push(Verdict {
                candidate: "thesis",
                index,
                subject: candidate.instrument.clone(),
                verdict: refused
                    .unwrap_or(HarnessRefusal::UnknownInstrument)
                    .code()
                    .to_owned(),
                thesis_id: None,
            }),
        }
    }
    let facts = platform_facts(context.facts, &snapshot)?;
    let proposals: Vec<ProposedThesis> = passed.iter().map(|p| p.proposal.clone()).collect();
    let fold = fold_theses(&FoldInput {
        mandate: context.mandate,
        overlay: context.overlay,
        universe: context.universe,
        proposals: &proposals,
        facts: &facts,
        lineages: context.lineages,
    })
    .map_err(|error| RuntimeError::NotInterpreted {
        what: format!("an admission mandate-research could not decide: {error}"),
        story: "E19-4",
    })?;
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
        let record = batch.journal(
            event_type,
            Some(invocation.clone()),
            thesis_record(entry, accepted, &accepted.proposal.thesis.output, reply)?,
        )?;
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
        if let Some(ignored) = ignored {
            let output = model_output(entry, accepted, run, reply.response_ref, ignored)?;
            batch.journal(
                "ModelOutputRecorded",
                Some(record),
                payload::model_output(&output)?,
            )?;
        }
        lines.push(Verdict {
            candidate: "thesis",
            index: accepted.index,
            subject: accepted.candidate.instrument.clone(),
            verdict: match step.decision {
                AdmissionDecision::Admitted {
                    change: AdmissionChange::Renewed,
                } => "renewed".to_owned(),
                AdmissionDecision::Admitted {
                    change: AdmissionChange::Admitted,
                } => "admitted".to_owned(),
                AdmissionDecision::Refused { reason } => reason.code().to_owned(),
            },
            thesis_id: Some(entry.thesis_id.as_str().to_owned()),
        });
    }
    lines.sort_by_key(|line| line.index);
    let active: BTreeSet<&str> = context
        .active_theses
        .iter()
        .map(|thesis| thesis.thesis_id.as_str())
        .collect();
    for (index, invalidation) in output.invalidations.iter().enumerate() {
        let verdict = if !active.contains(invalidation.thesis_id.as_str()) {
            HarnessRefusal::UnknownThesis.code()
        } else if invalidation
            .evidence
            .iter()
            .any(|id| !cited.contains_key(id.as_str()))
        {
            HarnessRefusal::UnknownEvidence.code()
        } else {
            "accepted"
        };
        lines.push(Verdict {
            candidate: "invalidation",
            index,
            subject: invalidation.thesis_id.clone(),
            verdict: verdict.to_owned(),
            thesis_id: None,
        });
    }
    let body = verdicts(run, invocation, "judged", None, &lines)?;
    batch.journal("CandidateVerdictsRecorded", Some(invocation.clone()), body)?;
    Ok(())
}

/// §6.5's harness checks in the table's order; the first that fails decides.
fn refusal(
    candidate: &CandidateThesis,
    fact: Option<&InstrumentFact>,
    cited: &BTreeMap<&str, Cited<'_>>,
    named: &mut BTreeSet<Symbol>,
    config: &ResearchConfig,
) -> Option<HarnessRefusal> {
    let Some(fact) = fact else {
        return Some(HarnessRefusal::UnknownInstrument);
    };
    if candidate
        .evidence
        .iter()
        .any(|id| !cited.contains_key(id.as_str()))
    {
        return Some(HarnessRefusal::UnknownEvidence);
    }
    if !named.insert(fact.symbol.clone()) {
        return Some(HarnessRefusal::DuplicateInstrument);
    }
    if forbidden(&candidate.thesis) || forbidden(&candidate.invalidation) {
        return Some(HarnessRefusal::ForbiddenLanguage);
    }
    if candidate.predecessor_thesis_id.is_some() && !config.revisions_enabled {
        return Some(HarnessRefusal::RevisionsNotEnabled);
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
    index: usize,
    candidate: &'a CandidateThesis,
    fact: &InstrumentFact,
    cited: &BTreeMap<&str, Cited<'_>>,
    thesis_id: &EventId,
    run: &Run,
    config: &ResearchConfig,
    context: &ResearchContext<'_>,
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
    let active = context
        .active_theses
        .iter()
        .find(|thesis| thesis.instrument == instrument);
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
        index,
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
    reply: &mandate_research_run::GatewayReply,
) -> Result<Value, RuntimeError> {
    let reference = |digest: &mandate_canon::Digest| payload::text(&format!("sha256:{digest}"));
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
        ("prompt_ref", reference(&reply.prompt_ref)),
        ("response_ref", reference(&reply.response_ref)),
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
    response_ref: mandate_canon::Digest,
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

/// The provisional verdict record (DEC-996 item 4): the run, its call record, the outcome, and one
/// line per candidate in the output's order, theses then invalidations (HI-22).
fn verdicts(
    run: &Run,
    invocation: &EventId,
    outcome: &str,
    rejection: Option<&str>,
    lines: &[Verdict],
) -> Result<Value, RuntimeError> {
    let lines = lines
        .iter()
        .map(|line| {
            payload::object(vec![
                ("candidate", payload::text(line.candidate)),
                (
                    "index",
                    payload::count(u64::try_from(line.index).unwrap_or(u64::MAX), "index")?,
                ),
                ("subject", payload::text(&line.subject)),
                ("verdict", payload::text(&line.verdict)),
                (
                    "thesis_id",
                    line.thesis_id.as_deref().map_or(Value::Null, payload::text),
                ),
            ])
        })
        .collect::<Result<Vec<_>, _>>()?;
    payload::object(vec![
        ("run_id", payload::text(&run.run_id)),
        ("invocation", payload::text(&invocation.0)),
        ("outcome", payload::text(outcome)),
        ("rejection", rejection.map_or(Value::Null, payload::text)),
        ("verdicts", Value::Array(lines)),
    ])
}
