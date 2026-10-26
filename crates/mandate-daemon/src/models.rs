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

/// The model host as the process reads it: every pinned quant model's evaluation at the tick
/// `at`, an empty answer for a refusal or a source that cannot be read.
pub trait Models {
    fn evaluate(&self, at: RiskClock) -> Vec<Evaluated>;
}

/// Why the models' inputs could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelsError {
    /// The agent stream holds a `ModelOutputRecorded` whose payload does not read.
    Unreadable,
}

/// The evaluations of `evaluated` that `agent`, the agent stream's records, holds no
/// `ModelOutputRecorded` for with the same model id, version, instrument and `as_of`, in the order
/// given.
///
/// # Errors
/// [`ModelsError::Unreadable`] for a record whose payload does not read.
pub fn fresh(
    agent: &[StoredEvent],
    evaluated: Vec<Evaluated>,
) -> Result<Vec<Evaluated>, ModelsError> {
    let journaled = agent
        .iter()
        .filter(|row| row.event_type == "ModelOutputRecorded")
        .map(journaled_key)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(evaluated
        .into_iter()
        .filter(|e| {
            let key = key_of(&e.output);
            !journaled.contains(&key)
        })
        .collect())
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
