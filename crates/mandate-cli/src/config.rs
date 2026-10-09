//! `mandate config register` and `mandate model register` (first paper trade brief, D1; DEC-526).
//! Each stores the configuration object it names in the artifact store, then commits exactly one
//! `ConfigSnapshotRegistered` naming it on the workspace control stream (journal spec §9.2,
//! DEC-155 item 5). Both are paper only.

use mandate_canon::{Digest, Object, Value, parse, to_canonical};
use mandate_journal::{ArtifactStore, Environment};
use mandate_modelhost::Refusal;
use mandate_time::UtcNanos;

use crate::control::{
    Confirmation, ControlError, ControlJournal, Decision, Now, Owner, Repeat, Shape, Submitted,
    commit_choice, decide, text,
};

/// The kinds `config register` registers: §9.2's version-1 kinds the paper run reads, and
/// version 2's `policy_set` and `model_registry` (DEC-484 item 4). `mandate_version` and
/// `model_version` are other commands'.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum ConfigKind {
    FeeConfig,
    TradingCalendar,
    InstrumentSnapshot,
    RuleSet,
    PolicySet,
    ModelRegistry,
}

impl ConfigKind {
    /// The kind's code in the payload and in a version-2 object's own `kind`.
    fn code(self) -> &'static str {
        match self {
            Self::FeeConfig => "fee_config",
            Self::TradingCalendar => "trading_calendar",
            Self::InstrumentSnapshot => "instrument_snapshot",
            Self::RuleSet => "rule_set",
            Self::PolicySet => "policy_set",
            Self::ModelRegistry => "model_registry",
        }
    }

    /// The `ConfigSnapshotRegistered` schema version that registers the kind (DEC-484 item 4).
    fn schema_version(self) -> u64 {
        match self {
            Self::PolicySet | Self::ModelRegistry => 2,
            Self::FeeConfig | Self::TradingCalendar | Self::InstrumentSnapshot | Self::RuleSet => 1,
        }
    }
}

const EVENT: &str = "ConfigSnapshotRegistered";

/// One member of DEC-523's snapshot: its name, and the test its string value must pass.
type Member = (&'static str, fn(&str) -> bool);

/// DEC-523's instrument snapshot: these eight string members and no others, each admitted by its
/// test.
const SNAPSHOT: [Member; 8] = [
    ("asset_class", |v| v == "us_equity"),
    ("etp", |v| v == "plain"),
    ("etp_classified_at", |v| UtcNanos::parse_rfc3339(v).is_ok()),
    ("etp_source", |v| v == "nasdaq_trader_symbol_directory"),
    ("exchange", |v| matches!(v, "arca" | "nasdaq")),
    ("increment", |v| v == "whole"),
    ("instrument_id", |v| !v.is_empty()),
    ("symbol", |v| !v.is_empty()),
];

fn is_dec_523(members: &Object) -> bool {
    members.len() == SNAPSHOT.len()
        && SNAPSHOT.iter().all(|(name, admits)| {
            members
                .get(*name)
                .and_then(Value::as_str)
                .is_some_and(admits)
        })
}

fn refused(reason: &'static str) -> ControlError {
    ControlError::Refused { reason }
}

fn paper_only(owner: &Owner) -> Result<(), ControlError> {
    if owner.environment == Environment::Paper {
        Ok(())
    } else {
        Err(refused("paper_only"))
    }
}

/// Stores `canonical`, whose reference is `hash`, and commits the registration `key` names with
/// `hash` as its content hash, unless a re-run finds it committed (DEC-290). The object is stored
/// first, so no committed event names a missing one.
fn registration(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    owner: &Owner,
    mut key: Vec<(&'static str, Value)>,
    (canonical, hash): (&[u8], Digest),
    schema_version: u64,
    now: Now,
) -> Result<Submitted, ControlError> {
    let reference = format!("sha256:{}", hash.to_hex());
    key.push(("content_hash", text(&reference)));
    let fixed = Confirmation::Fixed(false);
    let decided = match decide(journal, owner, EVENT, key, fixed, Repeat::FindsEarlier)? {
        Decision::Committed(earlier) => return Ok(earlier),
        Decision::Fresh(decided) => decided,
    };
    store
        .put_artifact(canonical)
        .map_err(|e| ControlError::Journal(format!("the artifact store: {e}")))?;
    let shape = Shape {
        schema_version,
        artifact_refs: vec![reference],
        config_refs: Vec::new(),
    };
    commit_choice(journal, owner, decided, shape, now)
}

/// Stores `object`, a JSON object, in canonical form and commits one `ConfigSnapshotRegistered`
/// of `kind` naming its hash: schema version 2 for `policy_set` and `model_registry`, 1 for the
/// rest. An instrument snapshot must be exactly DEC-523's object, and a version-2 object's own
/// `kind` must be `kind`. A re-run of a committed registration commits nothing (DEC-290).
///
/// # Errors
/// [`ControlError::Refused`] with `paper_only` for an owner not in paper,
/// `config_object_invalid` for bytes that are not a JSON object, `instrument_snapshot_invalid`
/// for a snapshot outside DEC-523, or `config_kind_mismatch`, each before anything is stored or
/// committed; [`ControlError::Journal`] as `commit` reports.
pub fn register(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    owner: &Owner,
    kind: ConfigKind,
    object: &[u8],
    now: Now,
) -> Result<Submitted, ControlError> {
    paper_only(owner)?;
    let value = parse(object)
        .ok()
        .filter(|v| v.as_object().is_some())
        .ok_or_else(|| refused("config_object_invalid"))?;
    match kind {
        ConfigKind::InstrumentSnapshot => {
            if !value.as_object().is_some_and(is_dec_523) {
                return Err(refused("instrument_snapshot_invalid"));
            }
        }
        ConfigKind::PolicySet | ConfigKind::ModelRegistry => {
            if value.get("kind").and_then(Value::as_str) != Some(kind.code()) {
                return Err(refused("config_kind_mismatch"));
            }
        }
        ConfigKind::FeeConfig | ConfigKind::TradingCalendar | ConfigKind::RuleSet => {}
    }
    let canonical = to_canonical(&value);
    let key = vec![
        ("admits_instruments", Value::Null),
        ("kind", text(kind.code())),
        ("model_id", Value::Null),
        ("model_version", Value::Null),
        ("params", Value::Array(Vec::new())),
    ];
    let hash = Digest::of(&canonical);
    let version = kind.schema_version();
    registration(journal, store, owner, key, (&canonical, hash), version, now)
}

/// Stores the content object `mandate_modelhost::content` computes for `model_id` at
/// `model_version`, and commits one `ConfigSnapshotRegistered` of kind `model_version` with its
/// hash, its sorted parameter names, and `admits_instruments: false` (DEC-504 item 3). No caller
/// supplies a hash: the host computes it from the code it runs.
///
/// # Errors
/// [`ControlError::Refused`] with `paper_only`, `model_unknown` for a model the host has no
/// content for, or the host's own code for any other refusal, before anything is stored or
/// committed; [`ControlError::Journal`] as `commit` reports.
pub fn register_model(
    journal: &mut dyn ControlJournal,
    store: &mut dyn ArtifactStore,
    owner: &Owner,
    model_id: &str,
    model_version: &str,
    now: Now,
) -> Result<Submitted, ControlError> {
    paper_only(owner)?;
    let content = mandate_modelhost::content(model_id, model_version).map_err(|refusal| {
        if refusal == Refusal::UnknownModel {
            refused("model_unknown")
        } else {
            refused(refusal.code())
        }
    })?;
    let object = parse(&content.canonical).map_err(|_| refused("content_object"))?;
    let mut names: Vec<&str> = object
        .get("params_schema")
        .and_then(|schema| schema.get("params"))
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|p| p.get("name").and_then(Value::as_str))
        .collect();
    names.sort_unstable();
    let key = vec![
        ("admits_instruments", Value::Bool(false)),
        ("kind", text("model_version")),
        ("model_id", text(model_id)),
        ("model_version", text(model_version)),
        (
            "params",
            Value::Array(names.into_iter().map(text).collect()),
        ),
    ];
    let stored = (content.canonical.as_slice(), content.hash);
    registration(journal, store, owner, key, stored, 1, now)
}
