//! The §9.5 acceptance test (DEC-360 option (c), DEC-446): the vectors' account stream — a
//! conforming `IntentReceived` at version 2, its `intended` `ProtectionChanged`, the companion
//! `OrderRequestRecorded` and its causation-naming `OrderSubmitted`, with every payload stamped
//! `risk_clock` — appends cleanly, and its replay restores the protection the opening was handed
//! in with and rebuilds the exact request a resubmission after a confirmed absence sends again
//! (`AGENTS.md` rule 13, trading-domain spec §5.7).
//!
//! The cases are read from the committed fixture (`fixtures/refcases/journal.json`, generated from
//! `docs/specs/reference-cases/journal.yaml`), never transcribed: the chain appends in the
//! section's own `batches`, the rule-45 batch cases are judged at the journal's own batch check,
//! and the fold is judged against the section's `fold_oracle`.

use std::path::Path;

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::{Key, Value, parse, to_canonical};
use mandate_executor::{
    AccountRef, AccountScope, BracketLegs, ClientOrderId, EventId, ExecutorState, FoldedEvent,
    IntentId, IntentOutcome, OrderType, Purpose, Seq, SubmitOrder, TimeInForce, WorkspaceId, fold,
};
use mandate_journal::{AppendOutcome, Draft, MemoryJournal, StreamId};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;

fn account_section() -> Result<Value, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let fixture = parse(&bytes).map_err(|e| format!("{e:?}"))?;
    fixture
        .get("account_stream")
        .cloned()
        .ok_or_else(|| "no account_stream".to_owned())
}

fn list<'a>(value: &'a Value, name: &'a str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .unwrap_or_default()
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

/// A chain event's body as a writer's draft: without the journal-assigned fields.
fn draft_bytes(entry: &Value) -> Result<Vec<u8>, String> {
    let mut body = entry
        .get("body")
        .and_then(Value::as_object)
        .cloned()
        .ok_or("no body")?;
    let keys: Vec<Key> = ["seq", "prev_hash", "recorded_at"]
        .iter()
        .filter_map(|name| Key::new(name).ok())
        .collect();
    for key in &keys {
        body.remove(key);
    }
    Ok(to_canonical(&Value::Object(body)))
}

/// Applies a case's change: a dotted envelope path set to a value, or removed.
fn apply(body: &mut Value, change: &Value) -> Result<(), String> {
    let path = text(change, "path");
    let mut names: Vec<&str> = path.split('.').collect();
    let last = names.pop().ok_or("a path")?;
    let key = Key::new(last).map_err(|e| e.to_string())?;
    let mut node = body;
    for name in names {
        node = match node {
            Value::Object(map) => map
                .get_mut(name)
                .ok_or_else(|| format!("no object {name}"))?,
            _ => return Err(format!("no object {name}")),
        };
    }
    match node {
        Value::Object(map) => {
            if change.get("delete") == Some(&Value::Bool(true)) {
                map.remove(&key);
            } else {
                map.insert(key, change.get("value").cloned().ok_or("a value")?);
            }
            Ok(())
        }
        _ => Err(format!("no object {last}")),
    }
}

/// A stored row as the fold reads it.
fn folded(row: &mandate_journal::StoredEvent) -> Result<FoldedEvent, String> {
    let body = parse(&row.body).map_err(|e| format!("{e:?}"))?;
    Ok(FoldedEvent {
        stream: row.stream_id.clone(),
        seq: Seq(row.seq),
        event_id: EventId(row.event_id.clone()),
        event_type: row.event_type.clone(),
        causation_id: body
            .get("causation_id")
            .and_then(Value::as_str)
            .map(|id| EventId(id.to_owned())),
        payload: body.get("payload").cloned().unwrap_or(Value::Null),
    })
}

fn scope() -> AccountScope {
    AccountScope {
        account: AccountRef("01J8Z2ACCT00000000000000A2".to_owned()),
        workspace: WorkspaceId("ws_01J8Z2".to_owned()),
    }
}

/// The vectors' account stream: appended batch by batch, every batch commits — a conforming
/// `IntentReceived`, its `intended` record in the same batch, and the companion and its order in
/// the next append cleanly, `risk_clock` stamped (DEC-446 item 3).
#[test]
fn the_account_stream_chain_appends_cleanly() -> Result<(), String> {
    let section = account_section()?;
    let stream = StreamId::parse(text(&section, "stream_id")).ok_or("a stream id")?;
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&stream);
    let recorded_at =
        UtcNanos::parse("2026-09-21T14:00:00.000400000Z").map_err(|e| format!("{e:?}"))?;
    let mut appended = 0usize;
    for batch in list(&section, "batches") {
        let seqs = batch.as_array().ok_or("a batch of seqs")?;
        let drafts: Vec<Vec<u8>> = seqs
            .iter()
            .map(|seq| {
                let seq = seq.as_int().ok_or("a seq")?;
                let entry = list(&section, "chain")
                    .iter()
                    .find(|entry| entry.get("seq").and_then(Value::as_int) == Some(seq))
                    .ok_or_else(|| format!("no chain seq {seq}"))?;
                draft_bytes(entry)
            })
            .collect::<Result<_, _>>()?;
        let refs: Vec<&[u8]> = drafts.iter().map(|d| d.as_slice()).collect();
        let outcome = journal.append(&stream, appended as u64, epoch, recorded_at, &refs);
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "batch {batch:?}: {outcome:?}"
        );
        appended += drafts.len();
    }
    assert_eq!(appended, list(&section, "chain").len(), "the whole chain");
    Ok(())
}

/// Rule 45's batches: the valid pair is accepted, and each invalid batch is refused whole at the
/// draft, reason and member the section lists.
#[test]
fn rule_45_batches_commit_or_are_refused_at_the_listed_draft() -> Result<(), String> {
    let section = account_section()?;
    let case_drafts = |case: &Value| -> Result<Vec<Vec<u8>>, String> {
        list(case, "drafts")
            .iter()
            .map(|member| {
                let seq = member
                    .get("base_seq")
                    .and_then(Value::as_int)
                    .ok_or("a base_seq")?;
                let entry = list(&section, "chain")
                    .iter()
                    .find(|entry| entry.get("seq").and_then(Value::as_int) == Some(seq))
                    .ok_or_else(|| format!("no chain seq {seq}"))?;
                let mut body = Value::Object(
                    entry
                        .get("body")
                        .and_then(Value::as_object)
                        .cloned()
                        .ok_or("no body")?,
                );
                if let Value::Object(map) = &mut body {
                    for name in ["seq", "prev_hash", "recorded_at"] {
                        if let Ok(key) = Key::new(name) {
                            map.remove(&key);
                        }
                    }
                }
                for change in member
                    .get("changes")
                    .and_then(Value::as_array)
                    .unwrap_or(&[])
                {
                    apply(&mut body, change)?;
                }
                Ok(to_canonical(&body))
            })
            .collect()
    };
    let judged = |case: &Value| -> Result<Result<(), (usize, String, String)>, String> {
        let drafts = case_drafts(case)?;
        let parsed = drafts
            .iter()
            .map(|bytes| Draft::parse(bytes).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, String>>()?;
        Ok(mandate_journal::check_batch(&parsed)
            .map_err(|(index, error)| (index, error.reason.code().to_owned(), error.path)))
    };
    for case in list(&section, "valid_batches") {
        let outcome = judged(case)?;
        assert!(outcome.is_ok(), "{}: {outcome:?}", text(case, "name"));
    }
    for case in list(&section, "invalid_batches") {
        let outcome = judged(case)?;
        let expect = case.get("expect").ok_or("an expect")?;
        assert_eq!(
            outcome.err(),
            Some((
                expect
                    .get("draft_index")
                    .and_then(Value::as_int)
                    .unwrap_or(u64::MAX) as usize,
                text(expect, "reason").to_owned(),
                text(expect, "path").to_owned(),
            )),
            "{}",
            text(case, "name")
        );
    }
    Ok(())
}

/// The replay: the chain folds to the oracle — the intent submitted with its protection restored
/// from the `intended` record, the resting protection the `placed` record journaled, one open
/// interval, and the exact request the companion rebuilds.
#[test]
fn the_replay_restores_protection_and_rebuilds_the_request() -> Result<(), String> {
    let section = account_section()?;
    let stream = StreamId::parse(text(&section, "stream_id")).ok_or("a stream id")?;
    let mut journal = MemoryJournal::new();
    let epoch = journal.take_ownership(&stream);
    let recorded_at =
        UtcNanos::parse("2026-09-21T14:00:00.000400000Z").map_err(|e| format!("{e:?}"))?;
    let mut drafts: Vec<Vec<u8>> = Vec::new();
    for entry in list(&section, "chain") {
        drafts.push(draft_bytes(entry)?);
    }
    let refs: Vec<&[u8]> = drafts.iter().map(|d| d.as_slice()).collect();
    let outcome = journal.append(&stream, 0, epoch, recorded_at, &refs);
    let AppendOutcome::Committed(rows) = outcome else {
        return Err("the chain did not commit".to_owned());
    };
    let mut state = ExecutorState::new(scope());
    for row in &rows {
        fold(&mut state, &folded(row)?).map_err(|e| e.to_string())?;
    }

    let oracle = section.get("fold_oracle").ok_or("a fold_oracle")?;
    let intent_oracle = oracle.get("intent").ok_or("an intent oracle")?;
    let intent = IntentId(EventId(text(intent_oracle, "intent_id").to_owned()));
    let record = state.intent(&intent).ok_or("the intent is unknown")?;
    assert_eq!(
        record.outcome,
        IntentOutcome::Submitted,
        "the intent's outcome at replay"
    );
    assert_eq!(record.agent.0, text(intent_oracle, "agent_id"));
    let prices = oracle
        .get("intent")
        .and_then(|intent| intent.get("protection"))
        .ok_or("the intent's prices")?;
    let restored = state
        .intent_protection(&intent)
        .ok_or("no protection restored")?;
    assert_eq!(
        restored.stop.to_string(),
        text(prices, "stop"),
        "the stop restored from the intended record (AGENTS.md rule 13)"
    );
    assert_eq!(
        restored.take_profit.map(|price| price.to_string()),
        Some(text(prices, "take_profit").to_owned()),
        "the take-profit restored from the intended record"
    );

    let request_oracle = oracle.get("request").ok_or("a request oracle")?;
    let order =
        ClientOrderId::parse(text(request_oracle, "client_order_id")).map_err(|e| e.to_string())?;
    let rebuilt = state
        .request_of(&order)
        .ok_or("no request rebuilt")?
        .clone();
    assert_eq!(
        rebuilt,
        SubmitOrder {
            client_order_id: order.clone(),
            instrument: InstrumentId::new(text(request_oracle, "instrument_id"))
                .map_err(|e| e.to_string())?,
            side: Side::Buy,
            qty: Qty::parse(text(request_oracle, "qty")).map_err(|e| e.to_string())?,
            order_type: OrderType::Limit,
            tif: TimeInForce::Gtc,
            limit_price: Some(
                Price::parse(text(request_oracle, "limit_price")).map_err(|e| e.to_string())?
            ),
            stop_price: None,
            bracket: Some(BracketLegs {
                take_profit: Price::parse(text(request_oracle, "take_profit"))
                    .map_err(|e| e.to_string())?,
                stop: Price::parse(text(request_oracle, "stop")).map_err(|e| e.to_string())?,
            }),
            oco: None,
            extended_hours: text(request_oracle, "extended_hours") == "true",
            purpose: Purpose::Open,
        },
        "the request the companion rebuilds is what a resubmission sends again (§5.7)"
    );

    let protection_oracle = oracle.get("protection").ok_or("a protection oracle")?;
    let instrument =
        InstrumentId::new(text(protection_oracle, "instrument_id")).map_err(|e| e.to_string())?;
    let protection = state
        .protection(&instrument)
        .map_err(|e| e.to_string())?
        .ok_or("no protection resting")?;
    let resting: Vec<String> = protection
        .resting
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();
    let expected_resting: Vec<String> = list(protection_oracle, "resting")
        .iter()
        .filter_map(|id| id.as_str())
        .map(str::to_owned)
        .collect();
    assert_eq!(
        resting, expected_resting,
        "the orders the placed record named"
    );
    assert_eq!(
        protection.covered_qty.to_string(),
        text(protection_oracle, "covered_qty"),
        "the covered quantity"
    );
    let resting_prices = protection.prices.ok_or("no prices resting")?;
    assert_eq!(
        resting_prices.stop.to_string(),
        text(protection_oracle, "stop")
    );
    assert_eq!(
        resting_prices.take_profit.map(|price| price.to_string()),
        Some(text(protection_oracle, "take_profit").to_owned())
    );
    assert!(
        state
            .unprotected_intervals()
            .iter()
            .any(|interval| interval.ended_at.is_none()),
        "the interval the unprotected_start opened is still open"
    );
    Ok(())
}
