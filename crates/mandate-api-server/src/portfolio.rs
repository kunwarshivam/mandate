//! The Portfolio page's reads ([DEC-2413](../../../docs/project/decisions/DEC-2413.md); workspace
//! API spec §4.7): `GET /dashboard`, `GET /dashboard/history`, and `GET /agents/{id}/pnl`, each
//! folded from journal rows alone (API-14).
//!
//! - **The account's value** is the broker's, as reconciliation journals it: the latest
//!   `AccountSnapshotRecorded` on each account stream the control stream names (journal spec §9.2,
//!   §9.35). Its `equity`, `cash` and `buying_power` are served as recorded; `last_equity`, the
//!   prior session's close, only from a version 2 snapshot, and the day's change is `equity` less
//!   it, signed. The latest snapshot counts whatever its version, so a version 1 after a version 2
//!   reports no prior close.
//! - **The equity history** is each account's snapshots in the range, the last one of each bucket
//!   of the range's width, by the snapshot's `recorded_at`.
//! - **An agent's P&L** is its sub-ledger's (trading spec §8.1, §8.2), through the positions read's
//!   own fold (`mandate_gate_context::Positions`): gross realized and unrealized, never net of fees
//!   or with income, which the journal does not attribute to an agent.
//!
//! A snapshot a read folds that does not read fails the read `journal_unavailable`: no figure is
//! served from part of the journal, and none is a zero standing in for one not reported.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::Response;
use mandate_authn::Request;
use mandate_canon::Value;
use mandate_journal::StoredEvent;
use mandate_num::Usd;
use mandate_time::UtcNanos;

use crate::alerts::followed;
use crate::http::App;
use crate::reads::{agent, deployed, envelope};
use crate::routes::{
    Refused, account_of, agent_members, agent_rows, document, object, path_refused, reader, served,
    string,
};

/// The record each account read folds.
const SNAPSHOT: &str = "AccountSnapshotRecorded";

/// A snapshot the read folds does not read: the read is `journal_unavailable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unread;

/// A decimal member of a snapshot's payload, `None` when it is absent or not a decimal.
fn figure(payload: &Value, member: &str) -> Option<Usd> {
    payload
        .get(member)
        .and_then(Value::as_str)
        .and_then(|text| Usd::parse(text).ok())
}

/// The payload of `row`, `None` when its body does not parse.
fn payload(row: &StoredEvent) -> Option<Value> {
    envelope(row)?.get("payload").cloned()
}

/// `read-models/dashboard`'s `snapshot` (module docs): the latest `AccountSnapshotRecorded` on
/// `rows`, `Ok(None)` when the stream holds none, and `Err` when that snapshot does not read.
pub fn latest_snapshot(rows: &[StoredEvent]) -> Result<Option<Value>, Unread> {
    let Some(row) = rows.iter().rev().find(|row| row.event_type == SNAPSHOT) else {
        return Ok(None);
    };
    let read = payload(row).ok_or(Unread)?;
    let member = |name: &str| figure(&read, name).ok_or(Unread);
    let equity = member("equity")?;
    let last_equity = match read.get("last_equity") {
        None => None,
        Some(_) => Some(member("last_equity")?),
    };
    let day_change = last_equity
        .map(|close| equity.checked_sub(close))
        .transpose()
        .map_err(|_| Unread)?;
    let decimal = |figure: Option<Usd>| figure.map_or(Value::Null, |f| string(&f.to_string()));
    Ok(Some(object(vec![
        ("equity", string(&equity.to_string())),
        ("cash", string(&member("cash")?.to_string())),
        ("buying_power", string(&member("buying_power")?.to_string())),
        ("last_equity", decimal(last_equity)),
        ("day_change", decimal(day_change)),
        ("environment", string(&row.environment)),
        ("observed_at", string(&row.recorded_at)),
    ])))
}

/// The ranges `GET /dashboard/history` serves: how far back each reaches from `served_at`, and the
/// width of the bucket each point stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Day,
    Week,
    Month,
    Quarter,
    Year,
    All,
}

/// Seconds in a day.
const DAY: i64 = 86_400;

impl Range {
    /// The range a query names: `range=` and one of `1d`, `1w`, `1m`, `3m`, `1y`, `all`, or no
    /// query, which is `1m`; `None` for anything else.
    pub fn from_query(query: Option<&str>) -> Option<Self> {
        Some(match query {
            None | Some("range=1m") => Self::Month,
            Some("range=1d") => Self::Day,
            Some("range=1w") => Self::Week,
            Some("range=3m") => Self::Quarter,
            Some("range=1y") => Self::Year,
            Some("range=all") => Self::All,
            Some(_) => return None,
        })
    }

    /// The wire name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Day => "1d",
            Self::Week => "1w",
            Self::Month => "1m",
            Self::Quarter => "3m",
            Self::Year => "1y",
            Self::All => "all",
        }
    }

    /// How far back the range reaches, in seconds; `None` for `all`.
    pub fn span_s(self) -> Option<i64> {
        match self {
            Self::Day => Some(DAY),
            Self::Week => Some(604_800),
            Self::Month => Some(2_592_000),
            Self::Quarter => Some(7_776_000),
            Self::Year => Some(31_536_000),
            Self::All => None,
        }
    }

    /// The width of one point's bucket, in seconds: five minutes over a day, an hour over a week,
    /// and a UTC day beyond.
    pub fn bucket_s(self) -> i64 {
        match self {
            Self::Day => 300,
            Self::Week => 3_600,
            Self::Month | Self::Quarter | Self::Year | Self::All => DAY,
        }
    }
}

/// `read-models/equity-history`'s `points` for one account (module docs): of the snapshots
/// recorded from `now` less the range's span to `now`, the last of each bucket, oldest first, each
/// with its `recorded_at` and `equity`. `Err` when a snapshot in the range does not read.
pub fn equity_points(
    rows: &[StoredEvent],
    now: UtcNanos,
    range: Range,
) -> Result<Vec<Value>, Unread> {
    let from = range.span_s().map(|span| now.secs().saturating_sub(span));
    let mut buckets: BTreeMap<i64, (String, Usd)> = BTreeMap::new();
    for row in rows.iter().filter(|row| row.event_type == SNAPSHOT) {
        let at = UtcNanos::parse_rfc3339(&row.recorded_at)
            .map_err(|_| Unread)?
            .secs();
        if from.is_some_and(|from| at < from) || at > now.secs() {
            continue;
        }
        let equity = payload(row)
            .and_then(|read| figure(&read, "equity"))
            .ok_or(Unread)?;
        let bucket = at.div_euclid(range.bucket_s());
        buckets.insert(bucket, (row.recorded_at.clone(), equity));
    }
    Ok(buckets
        .into_values()
        .map(|(at, equity)| {
            object(vec![
                ("at", string(&at)),
                ("equity", string(&equity.to_string())),
            ])
        })
        .collect())
}

/// Each account stream the control stream names (`ConnectionRequested` or
/// `ConnectionEstablished`, as the alerts read follows them), with its reference and its rows.
async fn accounts(
    app: &App,
    workspace: &str,
    control: &[StoredEvent],
) -> Result<Vec<(String, Vec<StoredEvent>)>, Refused> {
    let mut read = Vec::new();
    for reference in followed(control).1 {
        let stream = format!("acct:{workspace}:{reference}");
        let rows = app
            .journal
            .rows(&stream)
            .await
            .map_err(|_| Refused::JournalUnavailable)?;
        read.push((reference, rows));
    }
    Ok(read)
}

/// `GET /v1/workspaces/{ws}/dashboard`: `read-models/dashboard`, the workspace's agents as
/// `GET /agents` lists them, and each account's latest snapshot.
pub async fn dashboard(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    if query.is_some() {
        return Refused::Invalid.response();
    }
    let read = match agent_rows(&app, &reader, &deployed(&reader.control)).await {
        Ok(read) => read,
        Err(refusal) => return refusal.response(),
    };
    let accounts = match accounts(&app, &reader.workspace, &reader.control).await {
        Ok(accounts) => accounts,
        Err(refusal) => return refusal.response(),
    };
    let agents = read
        .iter()
        .map(|(deployment, rows)| {
            let found = document(&app, deployment);
            object(agent_members(&agent(deployment, rows), found.as_ref()))
        })
        .collect();
    let mut listed = Vec::with_capacity(accounts.len());
    for (reference, rows) in &accounts {
        let snapshot = match latest_snapshot(rows) {
            Ok(snapshot) => snapshot.unwrap_or(Value::Null),
            Err(Unread) => return Refused::JournalUnavailable.response(),
        };
        listed.push(object(vec![
            ("account_ref", string(reference)),
            ("snapshot", snapshot),
        ]));
    }
    let streams: Vec<&[StoredEvent]> = read
        .iter()
        .map(|(_, rows)| rows.as_slice())
        .chain(accounts.iter().map(|(_, rows)| rows.as_slice()))
        .collect();
    served(
        &reader,
        &streams,
        vec![
            ("agents", Value::Array(agents)),
            ("accounts", Value::Array(listed)),
        ],
    )
}

/// `GET /v1/workspaces/{ws}/dashboard/history?range=`: `read-models/equity-history`, each
/// account's equity over the range.
pub async fn history(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    let Ok(Path(workspace)) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let Some(range) = Range::from_query(query.as_deref()) else {
        return Refused::Invalid.response();
    };
    let accounts = match accounts(&app, &reader.workspace, &reader.control).await {
        Ok(accounts) => accounts,
        Err(refusal) => return refusal.response(),
    };
    let mut listed = Vec::with_capacity(accounts.len());
    for (reference, rows) in &accounts {
        let points = match equity_points(rows, reader.now, range) {
            Ok(points) => points,
            Err(Unread) => return Refused::JournalUnavailable.response(),
        };
        listed.push(object(vec![
            ("account_ref", string(reference)),
            ("points", Value::Array(points)),
        ]));
    }
    let streams: Vec<&[StoredEvent]> = accounts.iter().map(|(_, rows)| rows.as_slice()).collect();
    served(
        &reader,
        &streams,
        vec![
            ("range", string(range.name())),
            ("accounts", Value::Array(listed)),
        ],
    )
}

/// `GET /v1/workspaces/{ws}/agents/{agent_id}/pnl`: `read-models/pnl`, from the account stream
/// of the connection the agent's deployed document names, as its positions read folds it.
pub async fn agent_pnl(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    let Ok(Path((workspace, id))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    let Some(deployment) = deployed(&reader.control)
        .into_iter()
        .find(|deployment| deployment.0 == id)
    else {
        return Refused::NotFound.response();
    };
    if query.is_some() {
        return Refused::Invalid.response();
    }
    let account = match account_of(&app, &reader, document(&app, &deployment).as_ref()).await {
        Ok(account) => account,
        Err(refusal) => return refusal.response(),
    };
    let (reference, rows) = account.map_or((Value::Null, Vec::new()), |(reference, rows)| {
        (string(&reference), rows)
    });
    let Some(pnl) = app.folds.pnl(&rows, &deployment.0) else {
        return Refused::JournalUnavailable.response();
    };
    let mut members = vec![
        ("agent_id", string(&deployment.0)),
        ("environment", string(&deployment.1)),
        ("account_ref", reference),
    ];
    members.extend(pnl);
    served(&reader, &[rows.as_slice()], members)
}
