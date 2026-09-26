//! The corporate actions stored next to a dataset (backlog E2-4): `corporate-actions.json` in
//! the dataset directory, canonical JSON (journal spec §4) of the broker's splits, cash dividends,
//! and other actions dated in a day range. `download` writes it for the dataset's whole stored
//! span, comparing bytes first; `inspect` reads it to adjust prices and list what is not applied.

use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use mandate_canon::{DecStr, Int, Key, Object, Value};
use mandate_time::Date;

use crate::dataset::{DatasetError, Status};
use crate::model::{
    CashDividend, CorporateActions, DayRange, OtherAction, Split, Symbol, split_ratio,
};

/// The file name in a dataset directory.
pub const CORPORATE_ACTIONS: &str = "corporate-actions.json";
/// The value of the file's `format` member.
pub const ACTIONS_FORMAT: &str = "mandate-corporate-actions/1";

/// Distinguishes the temporary files of concurrent writers in one process.
static PARTIALS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
pub enum ActionsError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not the corporate actions of this dataset: {reason}")]
    Invalid { path: PathBuf, reason: String },
    #[error(transparent)]
    Dataset(#[from] DatasetError),
}

impl ActionsError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "io",
            Self::Invalid { .. } => "actions_file",
            Self::Dataset(e) => e.code(),
        }
    }
}

/// The corporate actions of one symbol dated in `range`, as a download recorded them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedActions {
    pub range: DayRange,
    pub actions: CorporateActions,
}

/// Writes `recorded` to `dir`, leaving an identical file untouched.
pub fn write_actions(dir: &Path, recorded: &RecordedActions) -> Result<Status, ActionsError> {
    let path = dir.join(CORPORATE_ACTIONS);
    let bytes = render(recorded).map_err(|reason| ActionsError::Invalid {
        path: path.clone(),
        reason,
    })?;
    fs::create_dir_all(dir).map_err(io_error(dir))?;
    if read_optional(&path)?.is_some_and(|existing| existing == bytes) {
        return Ok(Status::Unchanged);
    }
    let partial = PathBuf::from(format!(
        "{}.{}.{}.partial",
        path.display(),
        std::process::id(),
        PARTIALS.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = File::create_new(&partial).map_err(io_error(&partial))?;
    file.write_all(&bytes).map_err(io_error(&partial))?;
    file.sync_all().map_err(io_error(&partial))?;
    drop(file);
    fs::rename(&partial, &path).map_err(io_error(&path))?;
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(io_error(dir))?;
    Ok(Status::Written)
}

/// The actions recorded in `dir`, `None` when there is no file. Anything but the canonical
/// record of `symbol` this crate writes is refused.
pub fn read_actions(dir: &Path, symbol: &Symbol) -> Result<Option<RecordedActions>, ActionsError> {
    let path = dir.join(CORPORATE_ACTIONS);
    let Some(bytes) = read_optional(&path)? else {
        return Ok(None);
    };
    let invalid = |reason: String| ActionsError::Invalid {
        path: path.clone(),
        reason,
    };
    let recorded = parse(&bytes).map_err(invalid)?;
    if recorded.actions.symbol != *symbol {
        return Err(invalid(format!(
            "it records {}, not {}",
            recorded.actions.symbol.as_str(),
            symbol.as_str()
        )));
    }
    Ok(Some(recorded))
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> ActionsError + '_ {
    |source| ActionsError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, ActionsError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(path)(e)),
    }
}

fn render(recorded: &RecordedActions) -> Result<Vec<u8>, String> {
    let actions = &recorded.actions;
    let splits = actions
        .splits
        .iter()
        .map(|split| {
            object(vec![
                ("ex_date", date(split.ex_date)),
                ("id", text(&split.id)),
                ("new_shares", int(split.ratio.new_shares())?),
                ("old_shares", int(split.ratio.old_shares())?),
            ])
        })
        .collect::<Result<_, _>>()?;
    let cash_dividends = actions
        .cash_dividends
        .iter()
        .map(|dividend| {
            object(vec![
                ("ex_date", date(dividend.ex_date)),
                ("foreign", Value::Bool(dividend.foreign)),
                ("id", text(&dividend.id)),
                ("payable_date", optional_date(dividend.payable_date)),
                ("rate", text(dividend.rate.as_str())),
                ("record_date", optional_date(dividend.record_date)),
                ("special", Value::Bool(dividend.special)),
            ])
        })
        .collect::<Result<_, _>>()?;
    let other = actions
        .other
        .iter()
        .map(|action| {
            object(vec![
                ("ex_date", optional_date(action.ex_date)),
                ("id", text(&action.id)),
                ("kind", text(&action.kind)),
                ("process_date", date(action.process_date)),
            ])
        })
        .collect::<Result<_, _>>()?;
    let record = object(vec![
        ("cash_dividends", Value::Array(cash_dividends)),
        ("first", date(recorded.range.first())),
        ("format", text(ACTIONS_FORMAT)),
        ("last", date(recorded.range.last())),
        ("other", Value::Array(other)),
        ("splits", Value::Array(splits)),
        ("symbol", text(actions.symbol.as_str())),
    ])?;
    Ok(mandate_canon::to_canonical(&record))
}

/// Reads a record, refusing anything whose canonical rendering is not exactly `bytes`.
fn parse(bytes: &[u8]) -> Result<RecordedActions, String> {
    let value = mandate_canon::parse(bytes).map_err(|e| e.to_string())?;
    let format = value.get("format").and_then(Value::as_str);
    if format != Some(ACTIONS_FORMAT) {
        return Err(format!("format is {format:?}, not {ACTIONS_FORMAT:?}"));
    }
    let range = DayRange::new(date_of(&value, "first")?, date_of(&value, "last")?)
        .map_err(|e| e.to_string())?;
    let symbol = Symbol::parse(str_of(&value, "symbol")?).map_err(|e| e.to_string())?;
    let splits = array_of(&value, "splits")?
        .iter()
        .map(|split| {
            let (new, old) = (int_of(split, "new_shares")?, int_of(split, "old_shares")?);
            Ok(Split {
                id: str_of(split, "id")?.to_owned(),
                ex_date: date_of(split, "ex_date")?,
                ratio: split_ratio(new, old).map_err(|e| e.to_string())?,
            })
        })
        .collect::<Result<_, String>>()?;
    let cash_dividends = array_of(&value, "cash_dividends")?
        .iter()
        .map(|dividend| {
            Ok(CashDividend {
                id: str_of(dividend, "id")?.to_owned(),
                ex_date: date_of(dividend, "ex_date")?,
                record_date: optional_date_of(dividend, "record_date")?,
                payable_date: optional_date_of(dividend, "payable_date")?,
                rate: DecStr::parse(str_of(dividend, "rate")?).map_err(|e| e.to_string())?,
                special: bool_of(dividend, "special")?,
                foreign: bool_of(dividend, "foreign")?,
            })
        })
        .collect::<Result<_, String>>()?;
    let other = array_of(&value, "other")?
        .iter()
        .map(|action| {
            Ok(OtherAction {
                id: str_of(action, "id")?.to_owned(),
                kind: str_of(action, "kind")?.to_owned(),
                ex_date: optional_date_of(action, "ex_date")?,
                process_date: date_of(action, "process_date")?,
            })
        })
        .collect::<Result<_, String>>()?;
    let recorded = RecordedActions {
        range,
        actions: CorporateActions {
            symbol,
            splits,
            cash_dividends,
            other,
        },
    };
    if render(&recorded)? != bytes {
        return Err("it is not the canonical record this crate writes".to_owned());
    }
    Ok(recorded)
}

fn object(members: Vec<(&str, Value)>) -> Result<Value, String> {
    members
        .into_iter()
        .map(|(name, value)| {
            Key::new(name)
                .map(|key| (key, value))
                .map_err(|_| format!("`{name}` is outside the key grammar"))
        })
        .collect::<Result<Object, _>>()
        .map(Value::Object)
}

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

fn int(n: u64) -> Result<Value, String> {
    Int::new(n)
        .map(Value::Int)
        .ok_or_else(|| format!("{n} is above the canonical integer range"))
}

fn date(date: Date) -> Value {
    Value::Str(date.to_string())
}

fn optional_date(date: Option<Date>) -> Value {
    date.map_or(Value::Null, self::date)
}

fn member<'a>(value: &'a Value, name: &str) -> Result<&'a Value, String> {
    value
        .get(name)
        .ok_or_else(|| format!("there is no `{name}`"))
}

fn str_of<'a>(value: &'a Value, name: &str) -> Result<&'a str, String> {
    member(value, name)?
        .as_str()
        .ok_or_else(|| format!("`{name}` is not a string"))
}

fn int_of(value: &Value, name: &str) -> Result<u64, String> {
    member(value, name)?
        .as_int()
        .ok_or_else(|| format!("`{name}` is not an integer"))
}

fn bool_of(value: &Value, name: &str) -> Result<bool, String> {
    match member(value, name)? {
        Value::Bool(b) => Ok(*b),
        _ => Err(format!("`{name}` is not a boolean")),
    }
}

fn array_of<'a>(value: &'a Value, name: &str) -> Result<&'a [Value], String> {
    member(value, name)?
        .as_array()
        .ok_or_else(|| format!("`{name}` is not an array"))
}

fn date_of(value: &Value, name: &str) -> Result<Date, String> {
    Date::parse(str_of(value, name)?).map_err(|e| format!("`{name}`: {e}"))
}

fn optional_date_of(value: &Value, name: &str) -> Result<Option<Date>, String> {
    match member(value, name)? {
        Value::Null => Ok(None),
        _ => date_of(value, name).map(Some),
    }
}
