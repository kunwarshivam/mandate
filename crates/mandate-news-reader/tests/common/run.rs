//! Running a reader: its key, its configuration, a recorded transport, a poll-once executor, the
//! vendor's pages, and what the news stream holds.

use std::cell::RefCell;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use mandate_canon::{Value, parse};
use mandate_journal::{Environment, StoredEvent};
use mandate_news_reader::{Config, Follow, NewsReader};
use mandate_sources::SourceError;
use mandate_sources::allowlist::{Allowlist, PermittedUrl};
use mandate_sources::fetch::{NewsAuth, RawResponse, Transport};
use mandate_time::UtcNanos;

use super::agents::thesis;
use super::fixtures::{ACCOUNT, AGENT, SPY, WS};
use super::journal::{Journal, Shelf, register};

/// The checked-in allowlist, whose Alpaca news entry is in effect by the founder's approval.
pub const ALLOWLIST: &str = include_str!("../../../../config/sources/allowlist-v3.toml");

pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("a recorded fetch never waits"),
    }
}

/// A paper key, as `mandate vault put news-data-key` stores it.
pub const PAPER_KEY: &[u8] = b"PKTESTKEYID0001:topsecret0001";

pub fn paper_auth() -> NewsAuth {
    NewsAuth::from_stored(PAPER_KEY).unwrap()
}

/// A transport that answers from a queue of recorded responses, then `Transport` errors, and
/// records every URL it was handed, with whether credentials came along.
#[derive(Clone, Default)]
pub struct Recorded {
    pub answers: Rc<RefCell<Vec<RawResponse>>>,
    pub sent: Rc<RefCell<Vec<(String, bool)>>>,
}

impl Recorded {
    pub fn answering(bodies: &[Vec<u8>]) -> Self {
        let this = Self::default();
        for body in bodies {
            this.answer(body.clone());
        }
        this
    }

    pub fn answer(&self, body: Vec<u8>) {
        self.answers
            .borrow_mut()
            .push(RawResponse { status: 200, body });
    }

    pub fn sent(&self) -> Vec<String> {
        self.sent
            .borrow()
            .iter()
            .map(|(url, _)| url.clone())
            .collect()
    }
}

impl Transport for Recorded {
    async fn get(
        &self,
        url: &PermittedUrl,
        auth: Option<&NewsAuth>,
    ) -> Result<RawResponse, SourceError> {
        self.sent
            .borrow_mut()
            .push((url.as_str().to_owned(), auth.is_some()));
        let mut answers = self.answers.borrow_mut();
        if answers.is_empty() {
            return Err(SourceError::Transport);
        }
        Ok(answers.remove(0))
    }
}

pub fn config() -> Config {
    Config {
        workspace: WS.to_owned(),
        environment: Environment::Paper,
        allowlist: Allowlist::parse(ALLOWLIST).unwrap(),
        follows: vec![Follow {
            agent: AGENT.to_owned(),
            account: ACCOUNT.to_owned(),
        }],
        liveness: Duration::from_secs(10),
    }
}

pub type Reader = NewsReader<Journal, Shelf, Recorded>;

pub fn started(journal: &Journal, shelf: &Shelf, transport: &Recorded, now: UtcNanos) -> Reader {
    started_with(journal, shelf, transport, &config(), now)
}

pub fn started_with(
    journal: &Journal,
    shelf: &Shelf,
    transport: &Recorded,
    config: &Config,
    now: UtcNanos,
) -> Reader {
    NewsReader::start(
        journal.clone(),
        shelf.clone(),
        transport.clone(),
        config,
        Ok(paper_auth()),
        now,
    )
}

/// The URL's `symbols` query, as asked.
pub fn asked(url: &str) -> String {
    url.split("symbols=")
        .nth(1)
        .and_then(|rest| rest.split('&').next())
        .unwrap()
        .to_owned()
}

/// The news stream's `NewsItemStored` rows.
pub fn stored(journal: &Journal) -> Vec<StoredEvent> {
    journal
        .news()
        .into_iter()
        .filter(|row| row.event_type == "NewsItemStored")
        .collect()
}

/// The stored items' ids, in stream order.
pub fn ids(journal: &Journal) -> Vec<String> {
    stored(journal)
        .iter()
        .map(|row| member(row, "item_id").as_str().unwrap().to_owned())
        .collect()
}

/// One news item as Alpaca's endpoint writes it.
pub fn item(id: u64, created: &str, updated: &str, symbols: &[&str]) -> String {
    let symbols: Vec<String> = symbols.iter().map(|s| format!("\"{s}\"")).collect();
    format!(
        r#"{{"id":{id},"headline":"Headline {id}","author":"Staff","created_at":"{created}","updated_at":"{updated}","summary":"Summary {id}.","content":"","url":"https://news.example/{id}","symbols":[{}],"source":"benzinga","images":[]}}"#,
        symbols.join(",")
    )
}

/// A page of `items`, with `next` as its continuation token.
pub fn page(items: &[String], next: Option<&str>) -> Vec<u8> {
    let next = next.map_or("null".to_owned(), |t| format!("\"{t}\""));
    format!(
        r#"{{"news":[{}],"next_page_token":{next}}}"#,
        items.join(",")
    )
    .into_bytes()
}

/// A stored row's payload member, as text.
pub fn member(row: &StoredEvent, name: &str) -> Value {
    parse(&row.body)
        .unwrap()
        .get("payload")
        .and_then(|p| p.get(name))
        .cloned()
        .unwrap_or(Value::Null)
}

/// A workspace whose agent holds a thesis on SPY, with SPY's ticker registered.
pub fn spy(journal: &Journal, shelf: &Shelf) {
    register(journal, shelf, SPY, "SPY");
    thesis(journal, "01THESISSPY", SPY, "2026-11-30T00:00:00Z");
}
