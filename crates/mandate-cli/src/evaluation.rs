//! `mandate evaluation run` ([DEC-2244](../../../docs/project/decisions/DEC-2244.md); backlog
//! E17-8, E15-3): the forward-paper evaluation of an internal paper workspace's thesis records,
//! read from the workspace's own journal and journaled as `EvaluationRunRecorded` (journal spec
//! §9.18): whether the workspace is an internal paper one (item 4), the thesis records of its
//! agents' streams (item 2), and the run that evaluates, stores and appends (items 1 and 5).

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_evaluation::{
    Baselines, EvaluationError, EvaluationRecord, EvaluationRequest, InputFiles, ThesisRecord,
    decode, evaluate, evaluation_run_recorded, input_objects,
};
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactStore, StoredEvent, get_artifact};
use mandate_time::UtcNanos;
use secrecy::SecretString;

use crate::backtest::{is_id, rule_set};
use crate::control::{
    Confirmation, ControlError, ControlJournal, Decision, EVALUATION_TYPE, Now, Owner, Repeat,
    Shape, Submitted, agent_stream, commit_evaluation, control_stream, decide,
};
use crate::version::{refused, rows};

/// The agent-stream records the evaluation reads (journal spec §9.4).
const THESIS_TYPES: [&str; 2] = ["ThesisProposed", "ThesisRevised"];

/// The operator's setting that marks the workspace an internal paper one (DEC-2244 item 4, read as
/// DEC-1143 item 1 reads the API's).
pub const INTERNAL_PAPER_VAR: &str = "MANDATE_INTERNAL_PAPER_WORKSPACE";

/// The setting's value: on only for exactly `true`; off when absent, empty, or exactly `false`.
///
/// # Errors
/// [`ControlError::Refused`] with `internal_paper_setting` for any other value.
pub fn internal_paper_setting(value: Option<OsString>) -> Result<bool, ControlError> {
    match value.as_ref().map(|v| v.to_str()) {
        None => Ok(false),
        Some(Some("" | "false")) => Ok(false),
        Some(Some("true")) => Ok(true),
        Some(_) => Err(refused("internal_paper_setting")),
    }
}

/// Refuses unless `setting` is on and every row of the workspace's control stream, which must
/// hold one or more, is `paper` (DEC-2244 item 4). It reads no agent stream.
///
/// # Errors
/// [`ControlError::Refused`] with `not_internal_paper`; [`ControlError::Journal`] when the stream
/// cannot be read.
pub fn internal_paper(
    journal: &dyn ControlJournal,
    owner: &Owner,
    setting: bool,
) -> Result<(), ControlError> {
    let stored = journal.rows(&control_stream(owner)?)?;
    let paper = !stored.is_empty() && stored.iter().all(|row| row.environment == "paper");
    if setting && paper {
        Ok(())
    } else {
        Err(refused("not_internal_paper"))
    }
}

/// Every `ThesisProposed` and `ThesisRevised` of the workspace's agents (DEC-2244 item 2): the
/// agents are the `agent_id`s of the control stream's `AgentDeployed` records, each read once, in
/// id order, and each stream's records in stream order. It reads agent streams whatever the
/// workspace is, so a caller runs [`internal_paper`] first, as [`run`] does.
///
/// # Errors
/// [`ControlError::Refused`] with `record_unbound` for a record with no
/// `config_refs.mandate_version`, or `no_theses` when no agent stream holds one;
/// [`ControlError::Journal`] when a stream cannot be read or a stored event is not canonical.
pub fn thesis_records(
    journal: &dyn ControlJournal,
    owner: &Owner,
) -> Result<Vec<ThesisRecord>, ControlError> {
    let agents: BTreeSet<String> = rows(journal, owner)?
        .iter()
        .filter(|row| row.event_type == "AgentDeployed")
        .filter_map(|row| row.member("agent_id"))
        .map(str::to_owned)
        .collect();
    let mut records = Vec::new();
    for agent in &agents {
        for stored in journal.rows(&agent_stream(owner, agent)?)? {
            if THESIS_TYPES.contains(&stored.event_type.as_str()) {
                records.push(thesis_record(stored)?);
            }
        }
    }
    if records.is_empty() {
        return Err(refused("no_theses"));
    }
    Ok(records)
}

/// One stored thesis record, with the version its envelope binds.
fn thesis_record(stored: StoredEvent) -> Result<ThesisRecord, ControlError> {
    let envelope =
        parse(&stored.body).map_err(|e| ControlError::Journal(format!("a stored event: {e:?}")))?;
    let mandate_version = envelope
        .get("config_refs")
        .and_then(|refs| refs.get("mandate_version"))
        .and_then(Value::as_str)
        .and_then(ArtifactRef::parse)
        .ok_or_else(|| refused("record_unbound"))?
        .digest();
    Ok(ThesisRecord {
        stream_id: stored.stream_id,
        seq: stored.seq,
        hash: stored.hash,
        mandate_version,
        payload: envelope.get("payload").cloned().unwrap_or(Value::Null),
    })
}

/// What `run` committed or found: the evaluation, its report, whether it passed, and the event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluated {
    pub evaluation_id: String,
    pub report: String,
    pub passed: bool,
    pub submitted: Submitted,
}

/// One evaluation as `run` is asked for it: the four input files' bytes, the id the API will name
/// it by, and the instant it reads up to.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub files: InputFiles<'a>,
    pub evaluation_id: &'a str,
    pub cut_off: UtcNanos,
}

/// Runs the forward-paper evaluation of the workspace's thesis records and journals it
/// (DEC-2244 items 1, 2, 4 and 5): the internal paper gate, then the records, the inputs and the
/// evaluation; then it stores the build text, the three input objects, the report and the payload,
/// re-reads each by its digest, and appends `EvaluationRunRecorded` as the `system` `evaluator`.
/// A re-run of the same evaluation finds the record it committed (DEC-290).
///
/// # Errors
/// [`ControlError::Refused`], having appended nothing: `internal_paper` refusals,
/// `evaluation_id_invalid`, `input_invalid`, [`thesis_records`]' refusals, the evaluator's own
/// code when it refuses the evaluation (having stored nothing), `rule_set_missing` when the stream
/// has registered no rule set to bind, `artifact_corrupt` for a stored
/// object that does not re-hash, and `evaluation_id_taken` for an id another record holds.
/// [`ControlError::Journal`] as `commit` reports.
pub fn run(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    owner: &Owner,
    setting: bool,
    request: &Request<'_>,
    now: Now,
) -> Result<Evaluated, ControlError> {
    internal_paper(journal, owner, setting)?;
    if !is_id(request.evaluation_id) {
        return Err(refused("evaluation_id_invalid"));
    }
    let inputs = decode(&request.files).map_err(|_| refused("input_invalid"))?;
    let records = thesis_records(journal, owner)?;
    let source: &dyn ArtifactStore = store;
    let documents = |digest: &Digest| {
        let bytes = get_artifact(source, &ArtifactRef::from_digest(*digest)).ok()?;
        parse(&bytes).ok()
    };
    let evaluation = EvaluationRequest {
        decision_id: &inputs.decision_id,
        decision: &inputs.decision,
        cut_off: request.cut_off,
        records: &records,
        documents: &documents,
        closes: &inputs.closes,
        baselines: Baselines {
            basket: &inputs.basket,
            index: &inputs.index,
        },
        cost: &inputs.cost,
    };
    let evaluator = |e: EvaluationError| refused(e.code());
    let report = evaluate(&evaluation).map_err(evaluator)?;
    let objects = input_objects(&evaluation).map_err(evaluator)?;
    let build = format!("mandate-cli/{}", env!("CARGO_PKG_VERSION"));
    let payload = evaluation_run_recorded(&EvaluationRecord {
        evaluation_id: request.evaluation_id,
        report: &report,
        records: &records,
        code_build: Digest::of(build.as_bytes()),
    })
    .map_err(evaluator)?;
    let report_ref = ArtifactRef::from_digest(report.digest().map_err(evaluator)?).to_string();
    let evaluated = |submitted| Evaluated {
        evaluation_id: request.evaluation_id.to_owned(),
        report: report_ref.clone(),
        passed: report.passed(),
        submitted,
    };
    let control = rows(journal, owner)?;
    let rule_set = rule_set(&control)?;
    let earlier = control.into_iter().find(|r| {
        r.event_type == EVALUATION_TYPE && r.member("evaluation_id") == Some(request.evaluation_id)
    });
    if let Some(earlier) = earlier {
        return if *earlier.payload() == payload {
            Ok(evaluated(earlier.submitted()))
        } else {
            Err(refused("evaluation_id_taken"))
        };
    }
    let report_bytes = report.canonical_bytes().map_err(evaluator)?;
    let mut artifact_refs = BTreeSet::new();
    for bytes in [
        build.as_bytes(),
        &to_canonical(&objects.theses),
        &to_canonical(&objects.closes),
        &to_canonical(&objects.cost_model),
        &report_bytes,
    ] {
        artifact_refs.insert(put_checked(store, bytes)?);
    }
    put_checked(store, &to_canonical(&payload))?;
    let key = payload
        .as_object()
        .map(|members| {
            MEMBERS
                .iter()
                .map(|name| (*name, members.get(*name).cloned().unwrap_or(Value::Null)))
                .collect()
        })
        .unwrap_or_default();
    let decided = match decide(
        journal,
        owner,
        EVALUATION_TYPE,
        key,
        Confirmation::Fixed(false),
        Repeat::FindsEarlier,
    )? {
        Decision::Committed(earlier) => return Ok(evaluated(earlier)),
        Decision::Fresh(decided) => decided,
    };
    let shape = Shape {
        schema_version: 1,
        artifact_refs: artifact_refs.into_iter().collect(),
        config_refs: vec![("rule_set", Value::Str(rule_set))],
    };
    commit_evaluation(journal, owner, decided, shape, now).map(evaluated)
}

/// The payload's members, exactly (journal spec §9.18).
const MEMBERS: [&str; 13] = [
    "closes",
    "code_build",
    "cost_model",
    "cut_off",
    "decision",
    "evaluation_id",
    "passed",
    "report",
    "report_version",
    "theses",
    "theses_from",
    "theses_head",
    "theses_to",
];

/// Stores `bytes`, then re-reads them by their reference and re-hashes them; returns the reference.
fn put_checked(store: &mut dyn ArtifactStore, bytes: &[u8]) -> Result<String, ControlError> {
    let stored = |e: ArtifactError| match e {
        ArtifactError::Missing | ArtifactError::Corrupt => refused("artifact_corrupt"),
        ArtifactError::Unavailable => {
            ControlError::Journal(format!("the artifact store: {}", e.code()))
        }
    };
    let reference = store.put_artifact(bytes).map_err(stored)?;
    let read = get_artifact(store, &reference).map_err(stored)?;
    if read != bytes {
        return Err(refused("artifact_corrupt"));
    }
    Ok(reference.to_string())
}

#[derive(Debug, Subcommand)]
pub enum EvaluationCommand {
    /// Run the forward-paper evaluation of an internal paper workspace's thesis records and journal
    /// it as the `evaluator` system (DEC-2244): `--inputs` holds `decision.json`, `closes.json`,
    /// `cost.json` and `baselines.json`; `MANDATE_INTERNAL_PAPER_WORKSPACE` must be `true`. Prints
    /// the evaluation id, the report reference, whether it passed, and the event.
    Run(RunArgs),
}

/// There is no `--user` and no `--environment`: the record's actor is the `evaluator` system, and
/// an evaluation is never live.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// The internal paper workspace whose thesis records are evaluated.
    #[arg(long, value_name = "ID")]
    pub workspace: String,
    /// The directory holding the four input files.
    #[arg(long, value_name = "DIR")]
    pub inputs: PathBuf,
    /// The id `GET /evaluations/{id}` will name it by.
    #[arg(long, value_name = "ID")]
    pub evaluation_id: String,
    /// The instant the evaluation reads up to, RFC 3339 UTC.
    #[arg(long, value_name = "INSTANT")]
    pub cut_off: String,
    /// The workspace journal's Postgres DSN, without a password (set PGPASSWORD or ~/.pgpass).
    #[arg(long, value_name = "DSN")]
    pub journal: SecretString,
    /// The artifact store's root; `MANDATE_ARTIFACTS` when not given. Neither refuses
    /// `artifacts_unset`.
    #[arg(long, value_name = "DIR", visible_alias = "artifacts")]
    pub store: Option<PathBuf>,
}

/// The paper workspace `workspace` names, with no user: the record's actor is the evaluator.
///
/// # Errors
/// [`ControlError::Refused`] with `owner_workspace_invalid` for an id that is not a stream id's.
pub fn evaluator_owner(workspace: &str) -> Result<Owner, ControlError> {
    let _ = workspace;
    Err(ControlError::Unimplemented { story: "E17-8" })
}

/// The four input files under `dir`, in [`mandate_evaluation::INPUT_FILE_NAMES`]' order.
///
/// # Errors
/// [`ControlError::Refused`] with `input_invalid` when one cannot be read.
pub fn read_inputs(dir: &Path) -> Result<[Vec<u8>; 4], ControlError> {
    let _ = dir;
    Err(ControlError::Unimplemented { story: "E17-8" })
}

/// The cut-off `text` names, RFC 3339 UTC.
///
/// # Errors
/// [`ControlError::Refused`] with `cut_off_invalid` for any other text.
pub fn cut_off(text: &str) -> Result<UtcNanos, ControlError> {
    let _ = text;
    Err(ControlError::Unimplemented { story: "E17-8" })
}

/// `evaluation_id=`, `report=`, `passed=`, then the event's id and `seq`.
///
/// # Errors
/// When `report` cannot be written.
pub fn print_evaluated(report: &mut impl Write, evaluated: &Evaluated) -> std::io::Result<()> {
    let _ = (report, evaluated);
    Err(std::io::Error::other(ControlError::Unimplemented {
        story: "E17-8",
    }))
}

/// Runs `evaluation run` on the workspace's Postgres journal and store, and prints the outcome.
///
/// # Errors
/// `owner_workspace_invalid`, `cut_off_invalid`, `artifacts_unset`, `internal_paper_setting` or
/// `input_invalid` before the journal is opened, then [`run`]'s; no message names the DSN.
pub fn run_command(args: &RunArgs, now: Now, report: &mut impl Write) -> anyhow::Result<Evaluated> {
    let _ = (args, now, report);
    Err(ControlError::Unimplemented { story: "E17-8" }.into())
}
