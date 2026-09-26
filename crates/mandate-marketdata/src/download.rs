//! `download`: fetch every day of a dataset and store it, one partition at a time, so an
//! interrupted run resumes where it stopped and a repeated run changes nothing.

use mandate_time::Date;

use crate::client::{Client, FetchError, Pause, Transport};
use crate::dataset::{DatasetError, Outcome, Store};
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
}

impl DownloadError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Range(_) => "range",
            Self::Fetch { .. } => "fetch",
            Self::Store { .. } => "store",
        }
    }
}

/// Downloads `days` of `dataset` into `store`, reporting each day to `on_day` as it is stored.
pub async fn download<T: Transport, P: Pause>(
    client: &Client<T, P>,
    store: &Store,
    dataset: &DatasetId,
    days: DayRange,
    mut on_day: impl FnMut(&Outcome),
) -> Result<Vec<Outcome>, DownloadError> {
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
    Ok(outcomes)
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
