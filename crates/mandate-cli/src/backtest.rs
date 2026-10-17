//! `mandate backtest record` ([DEC-1144](../../../docs/project/decisions/DEC-1144.md); backlog
//! E4-5): journals the `BacktestRunRecorded` of a run `mandate-backtest-runner` stored, as the
//! `backtest_runner` system (journal spec §9.18, DEC-1141).
//!
//! The CLI is a workspace service and may not link the run (DEC-986, DEC-1000), so the runner
//! stores the payload and everything it names, and this command trusts none of it: it re-reads the
//! payload by its digest, re-hashes every artifact the payload names, checks that the report is the
//! one the payload names and is of the version and the inputs the payload names, and that the
//! version is the workspace's latest confirmed one; the `rule_set` it binds is the stream's latest
//! registered rule set. Any mismatch refuses before anything is appended.

use std::collections::BTreeSet;
use std::io::Write;

use clap::{Args, Subcommand};
use mandate_canon::{Value, parse};
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource, Environment, get_artifact};

use crate::control::{
    BACKTEST_TYPE, Confirmation, ControlError, ControlJournal, Decision, Now, Owner, Repeat, Shape,
    Submitted, commit_backtest, control_stream, decide, same_environment, text,
};
use crate::postgres::JournalArgs;
use crate::register::open;
use crate::version::{CONFIRMED, Row, paper_or_live, refused, rows, stored};

#[derive(Debug, Subcommand)]
pub enum BacktestCommand {
    /// Journal the `BacktestRunRecorded` of a run `mandate-backtest-runner run` stored, as the
    /// `backtest_runner` system: `--payload` is the `payload=` reference it printed, in the same
    /// `--store`. Every artifact the payload names is re-read and re-checked, and the version must
    /// be the workspace's latest confirmed one; prints the backtest id and the report reference,
    /// or the run's refusal code.
    Record(RecordArgs),
}

/// There is no `--user`: the record's actor is the `backtest_runner` system (DEC-1141).
#[derive(Debug, Args)]
pub struct RecordArgs {
    /// The workspace whose control stream records the backtest.
    #[arg(long, value_name = "ID")]
    pub workspace: String,
    /// `paper`, the default, or `live`: the workspace's environment (DEC-952).
    #[arg(long, value_name = "ENV", default_value = "paper")]
    pub environment: String,
    /// The payload's reference, `sha256:` and 64 hex digits, as the runner printed it.
    #[arg(long, value_name = "REF")]
    pub payload: String,
    #[command(flatten)]
    pub target: JournalArgs,
}

/// What `record` committed or found: the backtest, its report or refusal, and the event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub backtest_id: String,
    pub outcome: Result<String, String>,
    pub submitted: Submitted,
}

/// The payload's members, exactly (journal spec §9.18).
const MEMBERS: [&str; 12] = [
    "backtest_id",
    "code_build",
    "configuration",
    "data_snapshot",
    "mandate_version",
    "marker",
    "model_outputs",
    "refusal",
    "report",
    "report_version",
    "result",
    "theses",
];

/// The members that name a stored artifact; `report` is one only when the run completed.
const REFERENCES: [&str; 7] = [
    "code_build",
    "configuration",
    "data_snapshot",
    "mandate_version",
    "model_outputs",
    "report",
    "theses",
];

/// The report's `inputs` members and the payload member each must equal (DEC-1001 item 2).
const INPUTS: [(&str, &str); 4] = [
    ("bars", "data_snapshot"),
    ("model_outputs", "model_outputs"),
    ("theses", "theses"),
    ("config", "configuration"),
];

const REPORT_VERSION: u64 = 2;

fn member<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}

/// The workspace API's id: what `GET /backtests/{id}` can name.
fn is_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A stable reason code (ADR-0001 ES-09), as the read serves one.
fn is_code(code: &str) -> bool {
    let mut bytes = code.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase())
        && code.len() <= 64
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The checked bytes under `reference`, or `missing`/`corrupt` for a store that lacks them or holds
/// others.
fn checked(
    store: &dyn ArtifactSource,
    reference: &ArtifactRef,
    (missing, corrupt): (&'static str, &'static str),
) -> Result<Vec<u8>, ControlError> {
    get_artifact(store, reference).map_err(|e| match e {
        ArtifactError::Missing => refused(missing),
        ArtifactError::Corrupt => refused(corrupt),
        ArtifactError::Unavailable => {
            ControlError::Journal(format!("the artifact store: {}", e.code()))
        }
    })
}

/// The payload under `payload_ref`, when it is §9.18's twelve members with rule 143 holding, a
/// backtest marker and report version 2, an id the read can name, and a code the read can serve.
fn payload(store: &dyn ArtifactSource, payload_ref: &str) -> Result<Value, ControlError> {
    let reference =
        ArtifactRef::parse(payload_ref).ok_or_else(|| refused("payload_ref_invalid"))?;
    let bytes = checked(store, &reference, ("payload_missing", "payload_corrupt"))?;
    let invalid = || refused("payload_invalid");
    let value = parse(&bytes).map_err(|_| invalid())?;
    let names: Vec<&str> = value
        .as_object()
        .ok_or_else(invalid)?
        .keys()
        .map(|k| k.as_str())
        .collect();
    let completed = match member(&value, "result") {
        Some("completed") => true,
        Some("refused") => false,
        _ => return Err(invalid()),
    };
    let refusal = value.get("refusal");
    let consistent = if completed {
        refusal == Some(&Value::Null) && member(&value, "report").is_some()
    } else {
        value.get("report") == Some(&Value::Null) && member(&value, "refusal").is_some_and(is_code)
    };
    let well_formed = names == MEMBERS
        && consistent
        && member(&value, "marker") == Some("backtest")
        && value.get("report_version").and_then(Value::as_int) == Some(REPORT_VERSION)
        && member(&value, "backtest_id").is_some_and(is_id);
    if !well_formed {
        return Err(invalid());
    }
    Ok(value)
}

/// Every artifact `payload` names, re-read and re-hashed; for a completed run, the report, which
/// must be a backtest report of the payload's version and inputs. Returns the references, sorted.
fn artifacts(store: &dyn ArtifactSource, payload: &Value) -> Result<Vec<String>, ControlError> {
    let mut references = BTreeSet::new();
    for name in REFERENCES {
        let Some(text) = member(payload, name) else {
            continue;
        };
        let reference = ArtifactRef::parse(text).ok_or_else(|| refused("payload_invalid"))?;
        let bytes = checked(store, &reference, ("artifact_missing", "artifact_corrupt"))?;
        if name == "report" {
            check_report(&bytes, payload)?;
        }
        references.insert(text.to_owned());
    }
    Ok(references.into_iter().collect())
}

fn check_report(bytes: &[u8], payload: &Value) -> Result<(), ControlError> {
    let mismatch = || refused("report_mismatch");
    let report = parse(bytes).map_err(|_| mismatch())?;
    let inputs = report.get("inputs").ok_or_else(mismatch)?;
    let matches = member(&report, "kind") == Some("backtest_report")
        && report.get("report_version").and_then(Value::as_int) == Some(REPORT_VERSION)
        && member(&report, "mandate_version") == member(payload, "mandate_version")
        && INPUTS
            .iter()
            .all(|(input, named)| member(inputs, input) == member(payload, named));
    if matches { Ok(()) } else { Err(mismatch()) }
}

/// The latest registered rule set's content hash.
fn rule_set(rows: &[Row]) -> Result<String, ControlError> {
    rows.iter()
        .rev()
        .find(|r| {
            r.event_type == "ConfigSnapshotRegistered" && r.member("kind") == Some("rule_set")
        })
        .and_then(|r| r.member("content_hash"))
        .filter(|hash| ArtifactRef::parse(hash).is_some())
        .map(str::to_owned)
        .ok_or_else(|| refused("rule_set_missing"))
}

/// Journals the `BacktestRunRecorded` whose payload is stored under `payload_ref`, after every
/// check this module's documentation lists; a re-run finds the record it committed (DEC-290).
///
/// # Errors
/// [`ControlError::Refused`], having appended nothing: `paper_only`, `workspace_environment`;
/// `payload_ref_invalid`, `payload_missing`, `payload_corrupt`, `payload_invalid`;
/// `artifact_missing`, `artifact_corrupt`, `report_mismatch`; `document_missing`,
/// `document_corrupt`, `version_not_confirmed`; `rule_set_missing`; `backtest_id_taken` for an id
/// another record holds. [`ControlError::Journal`] as `commit` reports.
pub fn record(
    journal: &mut dyn ControlJournal,
    store: &dyn ArtifactSource,
    owner: &Owner,
    payload_ref: &str,
    now: Now,
) -> Result<Recorded, ControlError> {
    paper_or_live(owner)?;
    same_environment(journal, &control_stream(owner)?, owner)?;
    let payload = payload(store, payload_ref)?;
    let artifact_refs = artifacts(store, &payload)?;
    let version = member(&payload, "mandate_version").unwrap_or_default();
    stored(store, version)?;
    let rows = rows(journal, owner)?;
    let confirmed = rows.iter().rev().find(|r| r.event_type == CONFIRMED);
    if confirmed.and_then(|r| r.member("mandate_version")) != Some(version) {
        return Err(refused("version_not_confirmed"));
    }
    let rule_set = rule_set(&rows)?;
    let backtest_id = member(&payload, "backtest_id")
        .unwrap_or_default()
        .to_owned();
    let outcome = match member(&payload, "report") {
        Some(report) => Ok(report.to_owned()),
        None => Err(member(&payload, "refusal").unwrap_or_default().to_owned()),
    };
    let recorded = |submitted| Recorded {
        backtest_id: backtest_id.clone(),
        outcome: outcome.clone(),
        submitted,
    };
    let earlier = rows.iter().find(|r| {
        r.event_type == BACKTEST_TYPE && r.member("backtest_id") == Some(backtest_id.as_str())
    });
    if let Some(earlier) = earlier {
        return if *earlier.payload() == payload {
            Ok(recorded(earlier.submitted()))
        } else {
            Err(refused("backtest_id_taken"))
        };
    }
    let key = MEMBERS
        .iter()
        .map(|name| (*name, payload.get(name).cloned().unwrap_or(Value::Null)))
        .collect();
    let fixed = Confirmation::Fixed(false);
    let decided = match decide(
        journal,
        owner,
        BACKTEST_TYPE,
        key,
        fixed,
        Repeat::FindsEarlier,
    )? {
        Decision::Committed(earlier) => return Ok(recorded(earlier)),
        Decision::Fresh(decided) => decided,
    };
    let shape = Shape {
        schema_version: 1,
        artifact_refs,
        config_refs: vec![
            ("mandate_version", text(version)),
            ("rule_set", text(&rule_set)),
        ],
    };
    commit_backtest(journal, owner, decided, shape, now).map(recorded)
}

/// Runs `backtest record` on the workspace's Postgres journal and store, and prints the backtest
/// id, the report or the refusal, and the event.
///
/// # Errors
/// `environment_invalid` or `owner_workspace_invalid` before the journal is opened, then
/// [`record`]'s; no message names the DSN.
pub fn run_record(
    args: &RecordArgs,
    now: Now,
    report: &mut impl Write,
) -> anyhow::Result<Recorded> {
    let environment = match args.environment.as_str() {
        "paper" => Environment::Paper,
        "live" => Environment::Live,
        _ => return Err(refused("environment_invalid").into()),
    };
    let owner = Owner {
        workspace: args.workspace.clone(),
        user: String::new(),
        environment,
    };
    control_stream(&owner).map_err(|_| refused("owner_workspace_invalid"))?;
    let (mut journal, store) = open(&args.target)?;
    let recorded = record(&mut journal, &store, &owner, &args.payload, now)?;
    print_recorded(report, &recorded)?;
    Ok(recorded)
}

/// `backtest_id=`, then `report=` or `refused=`, then the event's id and `seq`.
///
/// # Errors
/// When `report` cannot be written.
pub fn print_recorded(report: &mut impl Write, recorded: &Recorded) -> std::io::Result<()> {
    writeln!(report, "backtest_id={}", recorded.backtest_id)?;
    match &recorded.outcome {
        Ok(reference) => writeln!(report, "report={reference}")?,
        Err(code) => writeln!(report, "refused={code}")?,
    }
    let Submitted { event_id, seq } = &recorded.submitted;
    writeln!(report, "recorded as event {event_id} at seq {seq}")
}
