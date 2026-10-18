#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The backtest runner ([DEC-1144](../../../docs/project/decisions/DEC-1144.md); backlog E4-4,
//! E4-5): runs a mandate version's backtest (`mandate_version_backtest::run_version`) over JSON
//! inputs, and stores in the artifact store everything a `BacktestRunRecorded` names: the report,
//! the four input objects whose digests reproduce it, the runner's build, and the §9.18 payload
//! itself. It never touches the journal.
//!
//! **Why a crate of its own.** The run links the executor's protection pricing, an agent-process
//! crate, and `mandate-cli` is a workspace service; the two groups never reach each other (DEC-986,
//! `xtask/layers.toml`), and DEC-1000 says a workspace service "runs it through the journal, never
//! by linking it". So the runner, in neither group, writes artifacts only, and `mandate backtest
//! record`, the workspace service, re-reads and re-checks them before it journals the record.
//!
//! The run is pure but for the store: no clock, no network, no randomness; every instant comes
//! from the inputs, and the same inputs give the same bytes.

pub mod inputs;

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use mandate_canon::{Digest, parse, to_canonical};
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactStore, get_artifact};
use mandate_num::Usd;
use mandate_spec::document::ProvenanceMap;
use mandate_spec::{Mandate, ValidatedMandate, ValidationContext};
use mandate_version_backtest::{RunRecord, VersionInput, VersionObjects, backtest_run_recorded};

pub use inputs::{FILE_NAMES, InputFiles, Inputs, decode};

/// The runner's name and version, whose bytes are stored and whose digest is the payload's
/// `code_build` (journal spec §6.3).
pub const BUILD: &str = concat!("mandate-backtest-runner/", env!("CARGO_PKG_VERSION"));

/// Why the runner stored no payload. Each is a refusal of the inputs, before any record exists;
/// a run that `run_version` refuses is not one: its payload says `refused`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunnerError {
    #[error("the backtest id is not 1 to 64 letters, digits, `_` or `-`")]
    BacktestIdInvalid,
    #[error("the mandate version is not a `sha256:` reference")]
    MandateVersionInvalid,
    #[error("the store holds no mandate under the version")]
    MandateMissing,
    #[error("the bytes stored under the version are not its canonical mandate")]
    MandateInvalid,
    #[error("the mandate does not validate in the backtest's context")]
    MandateRejected,
    #[error("input {0} is missing or malformed")]
    Input(String),
    #[error("the input objects could not be encoded: {0}")]
    Encode(&'static str),
    #[error("the artifact store: {0}")]
    Store(&'static str),
    #[error("no artifact store: neither `--store` nor MANDATE_ARTIFACTS names one")]
    StoreUnset,
    /// A step its story has not built yet (DEC-77): the stub's own report.
    #[error("not implemented until {story}")]
    Unimplemented { story: &'static str },
}

impl RunnerError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::BacktestIdInvalid => "backtest_id_invalid",
            Self::MandateVersionInvalid => "mandate_version_invalid",
            Self::MandateMissing => "mandate_missing",
            Self::MandateInvalid => "mandate_invalid",
            Self::MandateRejected => "mandate_rejected",
            Self::Input(_) => "input_invalid",
            Self::Encode(code) | Self::Store(code) => code,
            Self::StoreUnset => "artifacts_unset",
            Self::Unimplemented { .. } => "unimplemented",
        }
    }
}

/// What one run stored: the payload a `BacktestRunRecorded` carries, and the report, or the code
/// `run_version` refused with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub backtest_id: String,
    pub payload: ArtifactRef,
    pub outcome: Result<ArtifactRef, &'static str>,
}

impl fmt::Display for Ran {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "backtest_id={}", self.backtest_id)?;
        writeln!(f, "payload={}", self.payload)?;
        match &self.outcome {
            Ok(report) => writeln!(f, "report={report}"),
            Err(code) => writeln!(f, "refused={code}"),
        }
    }
}

/// The variable that names the cell's shared artifact store (#1578), which `--store` overrides.
pub const ARTIFACTS_VAR: &str = "MANDATE_ARTIFACTS";

/// The store root: the flag, else a non-empty `env` (the value of [`ARTIFACTS_VAR`]).
///
/// # Errors
/// [`RunnerError::StoreUnset`] when neither names one.
pub fn store_root(flag: Option<PathBuf>, env: Option<OsString>) -> Result<PathBuf, RunnerError> {
    match (flag, env) {
        (Some(flag), _) => Ok(flag),
        (None, Some(env)) if !env.is_empty() => Ok(PathBuf::from(env)),
        (None, _) => Err(RunnerError::StoreUnset),
    }
}

/// Reads and decodes the five input files [`FILE_NAMES`] names in `dir`.
///
/// # Errors
/// [`RunnerError::Input`] naming the first file that cannot be read, then as [`decode`].
pub fn read_dir(dir: &Path) -> Result<Inputs, RunnerError> {
    let read =
        |name: &str| fs::read(dir.join(name)).map_err(|_| RunnerError::Input(name.to_owned()));
    let [bars, outputs, theses, instrument, settings] = FILE_NAMES;
    decode(&InputFiles {
        bars: &read(bars)?,
        outputs: &read(outputs)?,
        theses: &read(theses)?,
        instrument: &read(instrument)?,
        settings: &read(settings)?,
    })
}

/// Whether `id` is an id the journal (`id`) and the workspace API's path segment both admit.
fn is_backtest_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn stored(error: ArtifactError) -> RunnerError {
    RunnerError::Store(error.code())
}

/// The mandate stored under `version`, validated in the backtest's context: the account is the
/// version's own allocation and nothing else, one user, no registry, no earlier version, and the
/// profile's stop-limit classes (DEC-1144 item 4). A version the workspace confirmed passed its
/// own rules then; this only builds the type the run takes, and refuses one that does not pass.
fn mandate(
    store: &dyn ArtifactStore,
    version: &str,
    inputs: &Inputs,
) -> Result<ValidatedMandate, RunnerError> {
    let reference = ArtifactRef::parse(version).ok_or(RunnerError::MandateVersionInvalid)?;
    let bytes = get_artifact(store, &reference).map_err(|e| match e {
        ArtifactError::Missing => RunnerError::MandateMissing,
        other => stored(other),
    })?;
    let value = parse(&bytes).map_err(|_| RunnerError::MandateInvalid)?;
    let document = Mandate::parse(&value).map_err(|_| RunnerError::MandateInvalid)?;
    let canonical = document
        .canonical_bytes()
        .map_err(|_| RunnerError::MandateInvalid)?;
    if Digest::of(&canonical) != reference.digest() {
        return Err(RunnerError::MandateInvalid);
    }
    let allocation = Usd::parse(document.capital.allocation_usd.as_str())
        .map_err(|_| RunnerError::MandateInvalid)?;
    let context = ValidationContext {
        account_equity_usd: allocation,
        other_allocations_usd: Usd::ZERO,
        validation_date: inputs.coverage_start.date(),
        registry: None,
        provenance: ProvenanceMap::default(),
        workspace_users: 1,
        approver_users: 1,
        independent_approval_required: false,
        disclosures_accepted: Default::default(),
        instrument_groups: Default::default(),
        claimed_by_other_agents: Default::default(),
        connection_environment: None,
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: Default::default(),
        previous_version: None,
        current_mandate_version: None,
        stop_limit_asset_classes: Some(inputs.stop_limit_classes.clone()),
    };
    ValidatedMandate::new(document, &context, &[]).map_err(|_| RunnerError::MandateRejected)
}

/// Runs the backtest `backtest_id` of the mandate stored under `mandate_version` over `inputs`,
/// and stores, in this order, the runner's build, the four input objects, the report when the run
/// completed, and last the `BacktestRunRecorded` payload, so the payload names nothing missing.
///
/// # Errors
/// [`RunnerError`], having stored no payload. A refused run is not an error: it is
/// `outcome: Err(code)`, and its payload records `refused`.
pub fn run(
    store: &mut dyn ArtifactStore,
    backtest_id: &str,
    mandate_version: &str,
    inputs: &Inputs,
) -> Result<Ran, RunnerError> {
    if !is_backtest_id(backtest_id) {
        return Err(RunnerError::BacktestIdInvalid);
    }
    let validated = mandate(store, mandate_version, inputs)?;
    let input = VersionInput {
        mandate: &validated,
        settings: &inputs.settings,
        instrument: &inputs.instrument,
        bars: &inputs.bars,
        coverage_start: inputs.coverage_start,
        first_bar_volumes: &inputs.medians,
        outputs: &inputs.outputs,
        theses: &inputs.theses,
        corporate_actions: &inputs.corporate_actions,
    };
    let objects = VersionObjects::of(&input).map_err(|e| RunnerError::Encode(e.code()))?;
    let code_build = store.put_artifact(BUILD.as_bytes()).map_err(stored)?;
    for object in [
        &objects.bars,
        &objects.model_outputs,
        &objects.theses,
        &objects.config,
    ] {
        store.put_artifact(&to_canonical(object)).map_err(stored)?;
    }
    let run = mandate_version_backtest::run_version(&input);
    let (record, outcome) = match &run {
        Ok(completed) => {
            let bytes = completed
                .report
                .canonical_bytes()
                .map_err(|e| RunnerError::Encode(e.code()))?;
            let report = store.put_artifact(&bytes).map_err(stored)?;
            (Ok(&completed.report), Ok(report))
        }
        Err(error) => (Err(error), Err(error.code())),
    };
    let version = ArtifactRef::parse(mandate_version).ok_or(RunnerError::MandateVersionInvalid)?;
    let payload = backtest_run_recorded(&RunRecord {
        backtest_id,
        mandate_version: version.digest(),
        inputs: objects.digests(),
        code_build: code_build.digest(),
        outcome: record,
    })
    .map_err(|e| RunnerError::Encode(e.code()))?;
    let payload = store
        .put_artifact(&to_canonical(&payload))
        .map_err(stored)?;
    Ok(Ran {
        backtest_id: backtest_id.to_owned(),
        payload,
        outcome,
    })
}
