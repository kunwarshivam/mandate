//! The recorded news the cell's reader runs on (E2-10c, DEC-2085): a [`Transport`] that answers
//! every request from a recording of Alpaca's news items and reaches no host, and the fixture key
//! the reader holds in place of the vault's. The binary runs on them only with `--recorded`, and
//! only at the `dev` or `ci` tier.
//!
//! A request is answered with one page, no continuation, holding the recorded items whose
//! `symbols` meet the request's `symbols`, in the recording's order, whatever its `start`: the
//! recording is fixed while the cell's clock moves on, so every round is answered again, and the
//! reader's own dedupe (journal rule 284) is what keeps each item journaled once. A request
//! without the key is refused as the vendor refuses it.

use std::collections::BTreeSet;
use std::rc::Rc;

use mandate_canon::{Key, Value, parse, to_canonical};
use mandate_sources::SourceError;
use mandate_sources::allowlist::PermittedUrl;
use mandate_sources::fetch::{NewsAuth, RawResponse, Transport};
use secrecy::SecretSlice;

use crate::key::{KeyRefusal, stored_key};

/// The fixture key: a paper key id that names no account, and no secret worth the name. The
/// recording ignores it but for its presence.
const RECORDED_KEY: &[u8] = b"PKRECORDEDNEWS:recorded-fixture-only";

/// The fixture key, read as a stored key is.
///
/// # Errors
/// [`stored_key`]'s, which the fixture key never meets.
pub fn recorded_key() -> Result<NewsAuth, KeyRefusal> {
    stored_key(Ok(Some(SecretSlice::from(RECORDED_KEY.to_vec()))))
}

/// A recording of Alpaca's news items, each with the tickers it names.
#[derive(Debug, Clone)]
pub struct RecordedNews {
    items: Rc<Vec<(BTreeSet<String>, Value)>>,
}

impl RecordedNews {
    /// The recording `file` holds: one page of the vendor's form, `{"news": [...]}`, each item
    /// naming its tickers as a list of strings under `symbols`.
    ///
    /// # Errors
    /// `recorded_news_malformed` for anything else.
    pub fn parse(file: &[u8]) -> Result<Self, &'static str> {
        let malformed = "recorded_news_malformed";
        let page = parse(file).map_err(|_| malformed)?;
        let Some(Value::Array(items)) = page.get("news") else {
            return Err(malformed);
        };
        let items = items
            .iter()
            .map(|item| {
                let Some(Value::Array(symbols)) = item.get("symbols") else {
                    return Err(malformed);
                };
                let symbols = symbols
                    .iter()
                    .map(|symbol| match symbol {
                        Value::Str(symbol) => Ok(symbol.clone()),
                        _ => Err(malformed),
                    })
                    .collect::<Result<BTreeSet<String>, _>>()?;
                Ok((symbols, item.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            items: Rc::new(items),
        })
    }

    /// The page answering a request for `asked`: the items naming any of them.
    fn page(&self, asked: &BTreeSet<&str>) -> Option<Vec<u8>> {
        let items = self
            .items
            .iter()
            .filter(|(symbols, _)| symbols.iter().any(|s| asked.contains(s.as_str())))
            .map(|(_, item)| item.clone())
            .collect();
        let page = Value::Object(
            [
                (Key::new("news").ok()?, Value::Array(items)),
                (Key::new("next_page_token").ok()?, Value::Null),
            ]
            .into_iter()
            .collect(),
        );
        Some(to_canonical(&page))
    }
}

/// The `symbols` a request's query names, comma-separated.
fn asked(url: &str) -> BTreeSet<&str> {
    url.split_once('?')
        .map(|(_, query)| query)
        .unwrap_or_default()
        .split('&')
        .filter_map(|pair| pair.strip_prefix("symbols="))
        .flat_map(|symbols| symbols.split(','))
        .filter(|symbol| !symbol.is_empty())
        .collect()
}

impl Transport for RecordedNews {
    async fn get(
        &self,
        url: &PermittedUrl,
        auth: Option<&NewsAuth>,
    ) -> Result<RawResponse, SourceError> {
        if auth.is_none() {
            return Err(SourceError::Status);
        }
        let body = self
            .page(&asked(url.as_str()))
            .ok_or(SourceError::Malformed)?;
        Ok(RawResponse { status: 200, body })
    }
}
