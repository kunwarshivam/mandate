//! The instruments the reader polls (DEC-2278 item 2, DEC-2280 item 1): one followed agent's active
//! theses' instruments and its held instruments, by asset id. The theses come from the research
//! core's own replay of the agent stream ([`replay_theses`]), as the runtime folds them, so a thesis
//! that ends, expires, or is invalidated drops out of the next poll. The held instruments come from
//! the gate context's own fold of the account stream ([`Journaled::held`]): the agent's sub-ledger,
//! as the executor computes it.
//!
//! Each fold is read on its own. A stream one of them cannot read gives none of its instruments and
//! is reported, and the other's still count: news only ever leads to an opinion, so polling less
//! adds no risk.

use std::collections::BTreeSet;

use mandate_canon::parse;
use mandate_gate_context::{Journaled, Row};
use mandate_journal::StoredEvent;
use mandate_research::{ReplayError, ThesisRecord, replay_theses};
use mandate_time::UtcNanos;

/// One followed agent's instruments, and which of its streams could not be read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Watched {
    pub assets: BTreeSet<String>,
    pub theses_unreadable: bool,
    pub held_unreadable: bool,
}

/// `agent`'s active theses' instruments at `at`, from its agent stream's rows, and its held
/// instruments, from the rows of its account stream `account_stream`.
pub fn followed(
    agent: &str,
    agent_rows: &[StoredEvent],
    account_stream: &str,
    account_rows: &[StoredEvent],
    at: UtcNanos,
) -> Watched {
    let mut followed = Watched::default();
    match theses(agent_rows, at) {
        Some(assets) => followed.assets.extend(assets),
        None => followed.theses_unreadable = true,
    }
    match held(agent, account_stream, account_rows) {
        Some(assets) => followed.assets.extend(assets),
        None => followed.held_unreadable = true,
    }
    followed
}

/// The instruments of the agent's theses active at `at`, or `None` when a thesis record cannot be
/// read.
fn theses(agent_rows: &[StoredEvent], at: UtcNanos) -> Option<BTreeSet<String>> {
    let records = agent_rows
        .iter()
        .filter(|row| {
            matches!(
                row.event_type.as_str(),
                "ThesisProposed" | "ThesisRevised" | "ThesisInvalidated"
            )
        })
        .map(|row| {
            let payload = parse(&row.body)
                .ok()
                .and_then(|body| body.get("payload").cloned())
                .ok_or(ReplayError::Unreadable { member: "payload" })?;
            Ok(ThesisRecord {
                event_type: row.event_type.as_str(),
                payload,
            })
        });
    let replayed = replay_theses(records, at).ok()?;
    Some(
        replayed
            .active
            .iter()
            .map(|entry| entry.instrument.as_str().to_owned())
            .collect(),
    )
}

/// The instruments `agent` holds on `account_stream`, or `None` when a row its fold reads cannot
/// be read.
fn held(
    agent: &str,
    account_stream: &str,
    account_rows: &[StoredEvent],
) -> Option<BTreeSet<String>> {
    let mut fold = Journaled::new(account_stream, "", agent);
    for row in account_rows {
        let payload = parse(&row.body)
            .ok()
            .and_then(|body| body.get("payload").cloned())?;
        fold.fold(Row {
            stream: &row.stream_id,
            event_type: &row.event_type,
            payload: &payload,
        })
        .ok()?;
    }
    Some(fold.held().into_keys().collect())
}
