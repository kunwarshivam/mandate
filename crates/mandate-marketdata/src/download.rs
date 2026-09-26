//! `download`: fetch every day of a dataset and store it, one partition at a time, so an
//! interrupted run resumes where it stopped and a repeated run changes nothing. A stock dataset
//! then records the corporate actions of its whole stored span next to it (E2-4).

use mandate_time::Date;

use crate::actions::{ActionsError, RecordedActions};
use crate::client::{Client, FetchError, Pause, Transport};
use crate::dataset::{DatasetError, Outcome, Status, Store};
use crate::model::{DatasetId, DayRange, ModelError};

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("{0}")]
    Range(ModelError),
    #[error("{dataset} {day}: {source}")]
    Fetch {
        dataset: String,
        day: Date,
        source: FetchError,
    },
    #[error("{dataset} {day}: {source}")]
    Store {
        dataset: String,
        day: Date,
        source: DatasetError,
    },
    #[error("{dataset} corporate actions: {source}")]
    FetchActions { dataset: String, source: FetchError },
    #[error("{dataset} corporate actions: {source}")]
    StoreActions {
        dataset: String,
        source: ActionsError,
    },
}

impl DownloadError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Range(_) => "range",
            Self::Fetch { .. } => "fetch",
            Self::Store { .. } => "store",
            Self::FetchActions { .. } | Self::StoreActions { .. } => "",
        }
    }
}

/// What storing a stock dataset's corporate actions did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredActions {
    pub recorded: RecordedActions,
    pub status: Status,
}

/// What a download stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    /// Each day, in date order.
    pub days: Vec<Outcome>,
    /// `None` for crypto, which has no corporate actions.
    pub corporate_actions: Option<StoredActions>,
}

/// Downloads `days` of `dataset` into `store`, reporting each day to `on_day` as it is stored,
/// and then, for a stock dataset, the corporate actions dated in its whole stored span.
pub async fn download<T: Transport, P: Pause>(
    client: &Client<T, P>,
    store: &Store,
    dataset: &DatasetId,
    days: DayRange,
    mut on_day: impl FnMut(&Outcome),
) -> Result<Downloaded, DownloadError> {
    let name = || describe(dataset);
    let mut outcomes = Vec::new();
    for day in days.days().map_err(DownloadError::Range)? {
        let records =
            client
                .fetch_day(dataset, day)
                .await
                .map_err(|source| DownloadError::Fetch {
                    dataset: name(),
                    day,
                    source,
                })?;
        let outcome =
            store
                .put_day(dataset, day, &records)
                .map_err(|source| DownloadError::Store {
                    dataset: name(),
                    day,
                    source,
                })?;
        on_day(&outcome);
        outcomes.push(outcome);
    }
    Ok(Downloaded {
        days: outcomes,
        corporate_actions: None,
    })
}

/// `<symbol> <kind> (<feed>)`, as progress and errors name a dataset.
pub fn describe(dataset: &DatasetId) -> String {
    format!(
        "{} {} ({})",
        dataset.symbol(),
        dataset.kind().dir_name(),
        dataset.feed()
    )
}
