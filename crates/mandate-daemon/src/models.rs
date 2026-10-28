//! The deployed runtime's pinned quant models (DEC-2383, DEC-503): on a risk-clock tick the process
//! asks its [`Models`] port for the host's evaluations of the deployed mandate's pinned models, and
//! hands the core, for each one not yet journaled, `Input::Observation` (the closes it read, stored
//! as an artifact) and then `Input::ModelOutput`, before the tick itself. The port is the model
//! host's (`mandate-modelhost::evaluate`), composed in a crate above this one (DEC-503 item 2); this
//! crate never runs a model, and a refusal is an empty answer, which can only shrink a buy (MI-10).
//!
//! [`fresh`] keeps the evaluations whose model, version, instrument and `as_of` the agent stream
//! holds no `ModelOutputRecorded` for, so a tick every second and a restart record each daily
//! output once. [`inputs`] is the pair the core takes for one evaluation. Replay reads the records
//! and never asks the port (INF-10); exits, protective orders, kill switches and reconciliation
//! never ask it (INF-13).

use mandate_canon::{Digest, Value, parse};
use mandate_journal::StoredEvent;
use mandate_runtime::{Input, ModelOutput, Observation, RiskClock};
use mandate_time::UtcNanos;

/// One pinned model's evaluation at a tick: the host's output, the canonical bytes of the closes
/// it read, and the source those closes came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluated {
    pub output: ModelOutput,
    pub closes: Vec<u8>,
    pub source: String,
}

/// The streams the process read for a tick's evaluation: the workspace's control rows, `None` when
/// they do not read, and the deployment's agent stream's thesis records ([`THESES`]), in stream
/// order, the only records of it a port reads. A research deployment's working universe is read
/// from them (DEC-2827); a pinned one's models need neither.
#[derive(Debug, Clone, Copy)]
pub struct Streams<'a> {
    pub control: Option<&'a [StoredEvent]>,
    pub agent: &'a [StoredEvent],
}

/// The agent stream's thesis records (mandate spec §8.5, §8.6): what [`Streams::agent`] holds, and
/// all the process keeps of that stream's records (DEC-2852 item 4).
pub const THESES: [&str; 3] = ["ThesisProposed", "ThesisRevised", "ThesisInvalidated"];

/// Whether `row` is one of the agent stream's thesis records.
#[must_use]
pub fn thesis(row: &StoredEvent) -> bool {
    THESES.contains(&row.event_type.as_str())
}

/// The outputs the agent stream journaled, folded a record at a time as the process reads them
/// (DEC-2852 item 4): what [`fresh`] reads of the whole stream, so a pass reads only what was
/// appended. A `ModelOutputRecorded` that does not read makes it unreadable from then on, as it
/// makes every later read of the whole stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outputs {
    journaled: Vec<OutputKey>,
    unreadable: bool,
}

impl Outputs {
    /// The outputs `agent`, the agent stream's records in order, journaled.
    #[must_use]
    pub fn of(agent: &[StoredEvent]) -> Self {
        let mut outputs = Self::default();
        for row in agent {
            outputs.fold(row);
        }
        outputs
    }

    /// Folds `row`, the agent stream's record after the last folded.
    pub fn fold(&mut self, row: &StoredEvent) {
        if row.event_type != "ModelOutputRecorded" {
            return;
        }
        match journaled_key(row) {
            Ok(key) => self.journaled.push(key),
            Err(ModelsError::Unreadable) => self.unreadable = true,
        }
    }

    /// The evaluations of `evaluated` the folded records hold no `ModelOutputRecorded` for, in the
    /// order given, each such key kept once.
    ///
    /// # Errors
    /// [`ModelsError::Unreadable`] once a folded record's payload did not read.
    pub fn fresh(&self, evaluated: Vec<Evaluated>) -> Result<Vec<Evaluated>, ModelsError> {
        if self.unreadable {
            return Err(ModelsError::Unreadable);
        }
        let mut kept = self.journaled.clone();
        Ok(evaluated
            .into_iter()
            .filter(|e| {
                let key = key_of(&e.output);
                let new = !kept.contains(&key);
                kept.push(key);
                new
            })
            .collect())
    }
}

/// The model host as the process reads it: every pinned quant model's evaluation at the tick
/// `at`, over the `streams` the process read, an empty answer for a refusal or a source that cannot
/// be read.
pub trait Models {
    fn evaluate(&self, at: RiskClock, streams: Streams<'_>) -> Vec<Evaluated>;
}

/// Why the models' inputs could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelsError {
    /// The agent stream holds a `ModelOutputRecorded` whose payload does not read.
    Unreadable,
}

/// The evaluations of `evaluated` that `agent`, the agent stream's records, holds no
/// `ModelOutputRecorded` for with the same model id, version, instrument and `as_of`, in the order
/// given, each such key kept once.
///
/// # Errors
/// [`ModelsError::Unreadable`] for a record whose payload does not read.
pub fn fresh(
    agent: &[StoredEvent],
    evaluated: Vec<Evaluated>,
) -> Result<Vec<Evaluated>, ModelsError> {
    Outputs::of(agent).fresh(evaluated)
}

/// What makes two outputs the same record: model, version, instrument, and `as_of` as the
/// payload writes it.
type OutputKey = (String, String, String, Option<String>);

fn key_of(output: &ModelOutput) -> OutputKey {
    (
        output.model_id.clone(),
        output.model_version.clone(),
        output.instrument_id.as_str().to_owned(),
        stamp(output.as_of),
    )
}

/// `at` as journal spec §4.7 writes a whole second, the form `ModelOutputRecorded`'s `as_of` takes.
fn stamp(at: RiskClock) -> Option<String> {
    UtcNanos::from_parts(at.secs(), 0)
        .ok()
        .map(|t| t.to_string())
}

fn journaled_key(row: &StoredEvent) -> Result<OutputKey, ModelsError> {
    let body = parse(&row.body).map_err(|_| ModelsError::Unreadable)?;
    let payload = body.get("payload").ok_or(ModelsError::Unreadable)?;
    let text = |name: &str| {
        payload
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(ModelsError::Unreadable)
    };
    Ok((
        text("model_id")?,
        text("model_version")?,
        text("instrument_id")?,
        Some(text("as_of")?),
    ))
}

/// The core's inputs for `evaluated`, whose closes are stored under `data_ref`: its
/// `Input::Observation` (source, instrument, the output's `as_of`, `data_ref`), then its
/// `Input::ModelOutput`.
///
/// # Errors
/// None: the result is a `Result` so a later check of the evaluation can refuse it.
pub fn inputs(evaluated: &Evaluated, data_ref: Digest) -> Result<[Input; 2], ModelsError> {
    let output = &evaluated.output;
    Ok([
        Input::Observation(Observation {
            source: evaluated.source.clone(),
            instrument_id: Some(output.instrument_id.clone()),
            as_of: output.as_of,
            data_ref,
        }),
        Input::ModelOutput(output.clone()),
    ])
}
