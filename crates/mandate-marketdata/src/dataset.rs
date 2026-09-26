//! Parquet partitions and the dataset manifest (DEC-89). A dataset directory holds one
//! `<YYYY-MM-DD>.parquet` per UTC day with data and a canonical-JSON `manifest.json` that records
//! the dataset, each decimal column's scale, and every fetched day, including empty ones. Writes
//! compare bytes first: identical content is left untouched, and different content for a stored
//! day is a conflict that changes nothing.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use mandate_canon::{Digest, Int, Key, Object, Value};
use mandate_time::Date;

use crate::model::{AssetClass, DatasetId, Feed, Kind, ModelError, Records, Symbol};
use crate::number::NumberError;
use crate::timestamp::TimestampError;

mod partition;

pub use partition::{encode, read, schema};

/// The manifest's `format` value; bumped when the layout or a column changes.
pub const FORMAT: &str = "mandate-marketdata/1";
/// File name of the manifest in a dataset directory.
pub const MANIFEST: &str = "manifest.json";
/// Scale of trade prices (trading domain spec §2.1: at most 9 decimal places).
pub const PRICE_SCALE: u8 = 9;
/// Scale of trade sizes (trading domain spec §2.1: at most 9 decimal places).
pub const SIZE_SCALE: u8 = 9;
/// Scale of every bar column: bars are vendor aggregates that the spec does not bound, and
/// BTC/USD bars arrive with ten fractional digits in prices and VWAP (DEC-89).
pub const BAR_SCALE: u8 = 18;

#[derive(Debug, thiserror::Error)]
pub enum DatasetError {
    #[error("column `{column}` row {row}: {source}")]
    Number {
        column: &'static str,
        row: usize,
        source: NumberError,
    },
    #[error("row {row} time: {source}")]
    Time { row: usize, source: TimestampError },
    #[error("parquet: {0}")]
    Parquet(String),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not a manifest of this dataset: {reason}")]
    Manifest { path: PathBuf, reason: String },
    #[error(
        "{day} differs from what {dir} holds; the stored data is left as it is (move the dataset directory away to download it again)"
    )]
    Conflict { dir: PathBuf, day: Date },
}

impl DatasetError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Number { .. } => "number",
            Self::Time { .. } => "time",
            Self::Parquet(_) => "parquet",
            Self::Io { .. } => "io",
            Self::Manifest { .. } => "manifest",
            Self::Conflict { .. } => "conflict",
        }
    }
}

/// The decimal columns of a kind and their scales, as the manifest records them.
pub fn decimal_scales(kind: Kind) -> &'static [(&'static str, u8)] {
    match kind {
        Kind::Bars(_) => &[
            ("open", BAR_SCALE),
            ("high", BAR_SCALE),
            ("low", BAR_SCALE),
            ("close", BAR_SCALE),
            ("volume", BAR_SCALE),
            ("vwap", BAR_SCALE),
        ],
        Kind::Trades => &[("price", PRICE_SCALE), ("size", SIZE_SCALE)],
    }
}

/// Whether a day's partition was written or already held these records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Written,
    Unchanged,
}

/// What storing one day did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub day: Date,
    pub rows: u64,
    /// The partition's size and SHA-256; `None` for a day without records, which has no file.
    pub file: Option<(u64, Digest)>,
    pub status: Status,
}

/// The output root that dataset directories live under.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn dataset_dir(&self, dataset: &DatasetId) -> PathBuf {
        self.root.join(dataset.relative_dir())
    }

    /// Stores one day of `dataset` and records it in the manifest. Nothing is rewritten when the
    /// day is already stored with the same bytes; a stored day whose content differs, a stored
    /// partition for a day that is now empty, and a manifest of another dataset are errors that
    /// change nothing.
    pub fn put_day(
        &self,
        dataset: &DatasetId,
        day: Date,
        records: &Records,
    ) -> Result<Outcome, DatasetError> {
        let dir = self.dataset_dir(dataset);
        let manifest_path = dir.join(MANIFEST);
        let stored = fs_read_optional(&manifest_path)?;
        let mut manifest = match &stored {
            Some(bytes) => {
                Manifest::parse(dataset, bytes).map_err(|reason| DatasetError::Manifest {
                    path: manifest_path.clone(),
                    reason,
                })?
            }
            None => Manifest::default(),
        };
        let conflict = || DatasetError::Conflict {
            dir: dir.clone(),
            day,
        };
        let partition = dir.join(partition_name(day));
        let rows = u64::try_from(records.len()).map_err(|_| conflict())?;
        let listed = manifest.days.get(&day).copied();
        let on_disk = fs_read_optional(&partition)?;
        let mut wrote = false;
        let entry = if records.is_empty() {
            if on_disk.is_some() || listed.is_some_and(|e| e.rows > 0) {
                return Err(conflict());
            }
            DayEntry { rows, file: None }
        } else {
            let bytes = encode(dataset, records)?;
            let file = (
                u64::try_from(bytes.len()).map_err(|_| conflict())?,
                Digest::of(&bytes),
            );
            let entry = DayEntry {
                rows,
                file: Some(file),
            };
            match on_disk {
                Some(existing) if existing == bytes => {}
                Some(_) => return Err(conflict()),
                None if listed.is_some_and(|e| e != entry) => return Err(conflict()),
                None => {
                    write_atomically(&dir, &partition, &bytes)?;
                    wrote = true;
                }
            }
            entry
        };
        if listed.is_some_and(|e| e != entry) {
            return Err(conflict());
        }
        manifest.days.insert(day, entry);
        let rendered = manifest
            .render(dataset)
            .map_err(|reason| DatasetError::Manifest {
                path: manifest_path.clone(),
                reason,
            })?;
        if stored.as_deref() != Some(rendered.as_slice()) {
            write_atomically(&dir, &manifest_path, &rendered)?;
            wrote = true;
        }
        Ok(Outcome {
            day,
            rows,
            file: entry.file,
            status: if wrote {
                Status::Written
            } else {
                Status::Unchanged
            },
        })
    }
}

/// The partition file name of `day`.
pub fn partition_name(day: Date) -> String {
    format!("{day}.parquet")
}

/// One day as a manifest lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListedDay {
    pub day: Date,
    pub rows: u64,
    /// The partition's size and SHA-256; `None` for a day without records, which has no file.
    pub file: Option<(u64, Digest)>,
}

/// Reads the manifest in `dir`: the dataset it records and every day it lists, in date order.
/// Anything but the canonical manifest this crate writes is refused.
pub fn read_manifest(dir: &Path) -> Result<(DatasetId, Vec<ListedDay>), DatasetError> {
    let path = dir.join(MANIFEST);
    let bytes = fs::read(&path).map_err(io_error(&path))?;
    let invalid = |reason: String| DatasetError::Manifest {
        path: path.clone(),
        reason,
    };
    let value = mandate_canon::parse(&bytes).map_err(|e| invalid(e.to_string()))?;
    let dataset = value
        .get("dataset")
        .ok_or_else(|| "there is no `dataset`".to_owned())
        .and_then(dataset_from_value)
        .map_err(invalid)?;
    let manifest = Manifest::parse(&dataset, &bytes).map_err(invalid)?;
    let days = manifest
        .days
        .iter()
        .map(|(day, entry)| ListedDay {
            day: *day,
            rows: entry.rows,
            file: entry.file,
        })
        .collect();
    Ok((dataset, days))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DayEntry {
    rows: u64,
    file: Option<(u64, Digest)>,
}

#[derive(Debug, Default)]
struct Manifest {
    days: BTreeMap<Date, DayEntry>,
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

fn dataset_value(dataset: &DatasetId) -> Result<Value, String> {
    let (kind, timeframe) = match dataset.kind() {
        Kind::Bars(timeframe) => ("bars", Some(timeframe.to_string())),
        Kind::Trades => ("trades", None),
    };
    let mut members = vec![
        ("asset_class", text(dataset.asset_class().as_str())),
        ("feed", text(dataset.feed().as_str())),
        ("kind", text(kind)),
        ("source", text("alpaca")),
        ("symbol", text(dataset.symbol().as_str())),
    ];
    if let Some(timeframe) = timeframe {
        members.push(("timeframe", Value::Str(timeframe)));
    }
    object(members)
}

fn dataset_from_value(value: &Value) -> Result<DatasetId, String> {
    let field = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("the dataset has no `{name}`"))
    };
    let model = |e: ModelError| e.to_string();
    let asset_class: AssetClass = field("asset_class")?.parse().map_err(model)?;
    let feed: Feed = field("feed")?.parse().map_err(model)?;
    let kind = match field("kind")? {
        "bars" => Kind::Bars(field("timeframe")?.parse().map_err(model)?),
        "trades" => Kind::Trades,
        other => return Err(format!("unknown kind `{other}`")),
    };
    let symbol = Symbol::parse(field("symbol")?).map_err(model)?;
    DatasetId::new(asset_class, feed, kind, symbol).map_err(model)
}

fn scales_value(kind: Kind) -> Result<Value, String> {
    let members = decimal_scales(kind)
        .iter()
        .map(|(column, scale)| int(u64::from(*scale)).map(|scale| (*column, scale)))
        .collect::<Result<Vec<_>, _>>()?;
    object(members)
}

impl Manifest {
    /// The manifest as canonical JSON (journal spec §4).
    fn render(&self, dataset: &DatasetId) -> Result<Vec<u8>, String> {
        let mut days = Vec::with_capacity(self.days.len());
        for (day, entry) in &self.days {
            let mut members = vec![
                ("date", Value::Str(day.to_string())),
                ("rows", int(entry.rows)?),
            ];
            if let Some((bytes, digest)) = entry.file {
                members.push(("file", Value::Str(partition_name(*day))));
                members.push(("bytes", int(bytes)?));
                members.push(("sha256", Value::Str(digest.to_hex())));
            }
            days.push(object(members)?);
        }
        let manifest = object(vec![
            ("dataset", dataset_value(dataset)?),
            ("days", Value::Array(days)),
            ("format", text(FORMAT)),
            ("scales", scales_value(dataset.kind())?),
        ])?;
        Ok(mandate_canon::to_canonical(&manifest))
    }

    /// Reads a manifest of `dataset`. Anything but the exact canonical form this crate writes
    /// for this dataset is refused, so a foreign or hand-edited manifest is never overwritten.
    fn parse(dataset: &DatasetId, bytes: &[u8]) -> Result<Self, String> {
        let value = mandate_canon::parse(bytes).map_err(|e| e.to_string())?;
        let format = value.get("format").and_then(Value::as_str);
        if format != Some(FORMAT) {
            return Err(format!("format is {format:?}, not {FORMAT:?}"));
        }
        let entries = value
            .get("days")
            .and_then(Value::as_array)
            .ok_or("`days` is not an array")?;
        let mut days = BTreeMap::new();
        for entry in entries {
            let date = entry
                .get("date")
                .and_then(Value::as_str)
                .and_then(|d| Date::parse(d).ok())
                .ok_or("a day has no valid `date`")?;
            let rows = entry
                .get("rows")
                .and_then(Value::as_int)
                .ok_or("a day has no `rows`")?;
            let file = match (entry.get("bytes"), entry.get("sha256")) {
                (Some(bytes), Some(sha)) => Some((
                    bytes.as_int().ok_or("`bytes` is not an integer")?,
                    sha.as_str()
                        .and_then(Digest::from_hex)
                        .ok_or("`sha256` is not a digest")?,
                )),
                _ => None,
            };
            days.insert(date, DayEntry { rows, file });
        }
        let manifest = Self { days };
        if manifest.render(dataset)? != bytes {
            return Err("it is not the canonical manifest of this dataset".to_owned());
        }
        Ok(manifest)
    }
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> DatasetError + '_ {
    |source| DatasetError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn fs_read_optional(path: &Path) -> Result<Option<Vec<u8>>, DatasetError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(path)(e)),
    }
}

/// Writes `bytes` to `path` through a synced temporary file in `dir` and a rename, so a crash
/// leaves either the old file or the new one.
fn write_atomically(dir: &Path, path: &Path, bytes: &[u8]) -> Result<(), DatasetError> {
    fs::create_dir_all(dir).map_err(io_error(dir))?;
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let mut file = File::create(&partial).map_err(io_error(&partial))?;
    file.write_all(bytes).map_err(io_error(&partial))?;
    file.sync_all().map_err(io_error(&partial))?;
    drop(file);
    fs::rename(&partial, path).map_err(io_error(path))?;
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(io_error(dir))
}
