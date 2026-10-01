//! Family E of the `mandate` suite: the thirty-two `escalation` cases (spec §6.1, §6.4, §11;
//! DEC-280, DEC-292), one kind with an `op`.
//!
//! **`ask_permit`** (MC-E25 to MC-E28) folds the case's `ledger` into a
//! `mandate_approval::AskLedger` in its order and judges each query with
//! `mandate_approval::ask_permit`: the ten-per-risk-day budget, an owner skip until the next risk
//! day, and a timeout for one `timeout_s` (DEC-156 item 5). **`deliver_now`** (MC-E30, MC-E32) judges
//! each query's channel at its instant with `mandate_approval::deliver_now` under the case's quiet
//! hours, in America/New_York wall time (DEC-156 item 6). Both read the library the runtime calls,
//! so these cases pin the rules, not yet the runtime's use of them (DEC-278 item 13).
//!
//! **`lifecycle`** (the other twenty-six) scripts the runtime's steps and is not interpreted yet: its
//! cases fail naming the op, so none can pass before an arm drives `mandate-runtime` (DEC-292).
//!
//! **Every key is read (DEC-85).** The case, each ledger entry, each query, each expectation and the
//! quiet hours are swept against the members this module reads, and a member it does not know fails
//! the case naming it. Each `expect` list is as long as its `queries`, so no query goes unjudged.

use mandate_approval::{
    AskEvent, AskLedger, AskPermit, Channel, Delivery, QuietHours, RiskClock, Suppression,
    ask_permit, deliver_now,
};
use mandate_time::NewYorkTime;

use super::{instant, unknown_members};
use crate::{Json, at, ensure, expect_eq, list_at, str_at, u64_at};

/// The members an `ask_permit` case carries at its top level.
const ASK_PERMIT_KEYS: &[&str] = &["id", "kind", "op", "title", "ledger", "queries", "expect"];
/// The members a `deliver_now` case carries at its top level.
const DELIVER_NOW_KEYS: &[&str] = &[
    "id",
    "kind",
    "op",
    "title",
    "quiet_hours",
    "queries",
    "expect",
];

pub(super) fn escalation_case(case: &Json) -> Result<(), String> {
    match str_at(case, "op")? {
        "ask_permit" => ask_permit_case(case),
        "deliver_now" => deliver_now_case(case),
        "lifecycle" => Err(super::not_implemented(
            "the `lifecycle` op, which drives `mandate-runtime` (DEC-292)",
        )),
        other => Err(format!(
            "escalation op `{other}` is not one the harness knows"
        )),
    }
}

/// `op: ask_permit`: each query's permit against the whole ledger.
fn ask_permit_case(case: &Json) -> Result<(), String> {
    unknown_members(case, ASK_PERMIT_KEYS)
        .map_err(|unknown| format!("case keys not interpreted: {unknown}"))?;
    let mut ledger = AskLedger::default();
    for (index, entry) in list_at(case, "ledger")?.iter().enumerate() {
        ledger.events.push(
            ask_event(entry)
                .map_err(|e| format!("ledger entry {}: {e}", index.saturating_add(1)))?,
        );
    }
    let (queries, expected) = paired(case)?;
    for (index, (query, expect)) in queries.iter().zip(expected).enumerate() {
        let number = index.saturating_add(1);
        unknown_members(query, &["instrument", "at"])
            .map_err(|unknown| format!("query {number}: members not interpreted: {unknown}"))?;
        unknown_members(expect, &["suppressed"]).map_err(|unknown| {
            format!("expectation {number}: members not interpreted: {unknown}")
        })?;
        let permit = ask_permit(&ledger, str_at(query, "instrument")?, clock(query, "at")?)
            .map_err(|e| format!("query {number}: {e}"))?;
        let got = match permit {
            AskPermit::Ask => None,
            AskPermit::Suppressed(Suppression::Budget) => Some("budget"),
            AskPermit::Suppressed(Suppression::SkippedToday) => Some("skipped_today"),
            AskPermit::Suppressed(Suppression::RecentTimeout) => Some("recent_timeout"),
        };
        let want = match expect.get("suppressed") {
            Some(Json::Null) => None,
            Some(Json::String(reason)) => Some(reason.as_str()),
            _ => {
                return Err(format!(
                    "expectation {number}: `suppressed` is a code or null"
                ));
            }
        };
        expect_eq(&format!("query {number}: suppressed"), got, want)?;
    }
    Ok(())
}

/// One ledger entry, by its `event`: the members each kind carries and no other.
fn ask_event(entry: &Json) -> Result<AskEvent, String> {
    let event = str_at(entry, "event")?;
    let known: &[&str] = match event {
        "requested" | "owner_skipped" => &["event", "instrument", "at"],
        "timed_out" => &["event", "instrument", "at", "timeout_s"],
        "version_applied" => &["event", "at"],
        other => return Err(format!("`{other}` is not an ask-ledger event")),
    };
    unknown_members(entry, known)
        .map_err(|unknown| format!("members not interpreted: {unknown}"))?;
    let at = clock(entry, "at")?;
    let instrument = || str_at(entry, "instrument").map(str::to_owned);
    Ok(match event {
        "requested" => AskEvent::Requested {
            instrument: instrument()?,
            at,
        },
        "owner_skipped" => AskEvent::OwnerSkipped {
            instrument: instrument()?,
            at,
        },
        "timed_out" => AskEvent::TimedOut {
            instrument: instrument()?,
            at,
            timeout_s: i64::try_from(u64_at(entry, "timeout_s")?)
                .map_err(|_| "`timeout_s` does not fit an i64".to_owned())?,
        },
        _ => AskEvent::VersionApplied { at },
    })
}

/// `op: deliver_now`: each query's channel at its instant, under the case's quiet hours.
fn deliver_now_case(case: &Json) -> Result<(), String> {
    unknown_members(case, DELIVER_NOW_KEYS)
        .map_err(|unknown| format!("case keys not interpreted: {unknown}"))?;
    let quiet = match case.get("quiet_hours") {
        Some(Json::Null) => None,
        Some(window) => {
            unknown_members(window, &["start", "end"])
                .map_err(|unknown| format!("`quiet_hours` members not interpreted: {unknown}"))?;
            Some(QuietHours {
                start: wall(window, "start")?,
                end: wall(window, "end")?,
            })
        }
        None => return Err("a `deliver_now` case states its `quiet_hours`".to_owned()),
    };
    let (queries, expected) = paired(case)?;
    for (index, (query, expect)) in queries.iter().zip(expected).enumerate() {
        let number = index.saturating_add(1);
        unknown_members(query, &["channel", "at"])
            .map_err(|unknown| format!("query {number}: members not interpreted: {unknown}"))?;
        unknown_members(expect, &["status"]).map_err(|unknown| {
            format!("expectation {number}: members not interpreted: {unknown}")
        })?;
        let channel = match str_at(query, "channel")? {
            "cli_inbox" => Channel::CliInbox,
            "push" => Channel::Push,
            other => return Err(format!("query {number}: `{other}` is not a channel")),
        };
        let delivery = deliver_now(channel, quiet, clock(query, "at")?)
            .map_err(|e| format!("query {number}: {e}"))?;
        let got = match delivery {
            Delivery::Send => "delivered",
            Delivery::SuppressedQuietHours => "suppressed_quiet_hours",
        };
        expect_eq(
            &format!("query {number}: status"),
            got,
            str_at(expect, "status")?,
        )?;
    }
    Ok(())
}

/// The case's queries and expectations, which must be equally many and at least one.
fn paired(case: &Json) -> Result<(&[Json], &[Json]), String> {
    let queries = list_at(case, "queries")?;
    let expected = list_at(case, "expect")?;
    ensure(!queries.is_empty(), || {
        "a case states at least one query".to_owned()
    })?;
    ensure(queries.len() == expected.len(), || {
        format!(
            "{} queries but {} expectations",
            queries.len(),
            expected.len()
        )
    })?;
    Ok((queries, expected))
}

/// The risk-clock second at `key`, which must be a whole second: the risk clock has no fraction to
/// drop (mandate spec §5.2).
fn clock(value: &Json, key: &str) -> Result<RiskClock, String> {
    let when = instant(at(value, key)?, key)?;
    ensure(when.nanos() == 0, || {
        format!("`{key}` is not a whole second")
    })?;
    Ok(RiskClock(when.secs()))
}

fn wall(value: &Json, key: &str) -> Result<NewYorkTime, String> {
    NewYorkTime::parse(str_at(value, key)?).map_err(|e| format!("`{key}`: {e}"))
}
