//! `download`: fetch every day of a dataset and store it, one partition at a time, so an
//! interrupted run resumes where it stopped and a repeated run changes nothing. A stock dataset
//! then records the corporate actions of its whole stored span next to it (E2-4).

use mandate_time::Date;

use crate::actions::{ActionsError, RecordedActions, write_actions};
use crate::client::{Client, FetchError, Pause, Transport};
use crate::dataset::{DatasetError, MANIFEST, Outcome, Status, Store, read_manifest};
use crate::model::{AssetClass, DatasetId, DayRange, ModelError};

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
            Self::FetchActions { .. } => "fetch_actions",
            Self::StoreActions { .. } => "store_actions",
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
    let corporate_actions = match dataset.asset_class() {
        AssetClass::UsEquity => Some(store_actions(client, store, dataset).await?),
        AssetClass::Crypto => None,
    };
    Ok(Downloaded {
        days: outcomes,
        corporate_actions,
    })
}

/// Fetches and records the actions dated in the dataset's stored span, first to last listed day.
async fn store_actions<T: Transport, P: Pause>(
    client: &Client<T, P>,
    store: &Store,
    dataset: &DatasetId,
) -> Result<StoredActions, DownloadError> {
    let stored = |source: ActionsError| DownloadError::StoreActions {
        dataset: describe(dataset),
        source,
    };
    let dir = store.dataset_dir(dataset);
    let (_, listed) = read_manifest(&dir).map_err(|e| stored(e.into()))?;
    let span = listed
        .first()
        .zip(listed.last())
        .and_then(|(first, last)| DayRange::new(first.day, last.day).ok())
        .ok_or_else(|| {
            stored(ActionsError::Invalid {
                path: dir.join(MANIFEST),
                reason: "the manifest lists no day".to_owned(),
            })
        })?;
    let actions = client
        .fetch_corporate_actions(dataset.symbol(), span)
        .await
        .map_err(|source| DownloadError::FetchActions {
            dataset: describe(dataset),
            source,
        })?;
    let recorded = RecordedActions {
        range: span,
        actions,
    };
    let status = write_actions(&dir, &recorded).map_err(stored)?;
    Ok(StoredActions { recorded, status })
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
