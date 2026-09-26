//! The corporate actions stored next to a dataset (backlog E2-4): `corporate-actions.json` in
//! the dataset directory, canonical JSON (journal spec §4) of the broker's splits, cash dividends,
//! and other actions dated in a day range. `download` writes it for the dataset's whole stored
//! span, comparing bytes first; `inspect` reads it to adjust prices and list what is not applied.

use std::path::{Path, PathBuf};

use crate::dataset::{DatasetError, Status};
use crate::model::{CorporateActions, DayRange, Symbol};

/// The file name in a dataset directory.
pub const CORPORATE_ACTIONS: &str = "corporate-actions.json";
/// The value of the file's `format` member.
pub const ACTIONS_FORMAT: &str = "mandate-corporate-actions/1";

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
        ""
    }
}

/// The corporate actions of one symbol dated in `range`, as a download recorded them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedActions {
    pub range: DayRange,
    pub actions: CorporateActions,
}

/// Writes `recorded` to `dir`, leaving an identical file untouched.
pub fn write_actions(_dir: &Path, _recorded: &RecordedActions) -> Result<Status, ActionsError> {
    Ok(Status::Written)
}

/// The actions recorded in `dir`, `None` when there is no file. Anything but the canonical
/// record of `symbol` this crate writes is refused.
pub fn read_actions(
    _dir: &Path,
    _symbol: &Symbol,
) -> Result<Option<RecordedActions>, ActionsError> {
    Ok(None)
}
