//! The protective sequences and the exit price ladder (E7-4, trading-domain spec §5.4 to §5.6).

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_num::{Fraction, Price, ShareIncrement, SignedQty};

use mandate_accounting::Side;
use mandate_canon::Value;

use crate::batch::Batch;
use crate::error::ExecutorError;
use crate::ids::{ClientOrderId, IntentId};
use crate::intent::{gate_and_submit, intent_of, journal_submission};
use crate::orders::transition;
use crate::payload::text;
use crate::ports::Ports;
use crate::state::{ExecutorState, ExitSequence, IntentOutcome};
use crate::types::{
    BrokerRequest, ExitTier, IntentBody, MarketObservation, OcoLegs, OrderState, OrderType,
    Purpose, RiskClock, SubmitOrder, TimeInForce,
};

/// The stub still standing at §5.4's order path after slice 3a: an **add** in an instrument whose
/// protection rests (a new bracket, slice 2's) and a **passive** exit there (a new OCO keeping the
/// stop, slice 3b's). A marketable exit takes [`begin_exit`] instead and never reaches it, and a
/// protective order is the re-placement itself.
///
/// **`AGENTS.md` rule 13:** the passive exit answering this stub would be a denied exit once
/// reachable. Nothing creates protection on an unprotected position before slice 2 — the pin
/// `no_protection_is_created_on_an_unprotected_position` holds that — so no instrument is protected
/// while the stub stands, and slice 2 lands no earlier than 3a, 3b and 4 (DEC-160).
pub(crate) fn sequenced(
    state: &ExecutorState,
    instrument: &InstrumentId,
    purpose: Purpose,
) -> Result<(), ExecutorError> {
    if purpose != Purpose::Protective && rests(state, instrument) {
        return Err(ExecutorError::Unimplemented { story: "E7-4" });
    }
    Ok(())
}

/// A quote is kept as the instrument's latest observation (the shell's [`crate::handle`] does
/// that before the step). The one it may not pass is the triggered-stop watchdog's input (§5.4,
/// slice 4): a sane mark at or below a resting stop. Until slice 4 that answers its stub; the
/// refusal is of this quote's step alone, so no exit, cancel or reconciliation waits on it.
pub(crate) fn watched(
    state: &ExecutorState,
    observation: &MarketObservation,
) -> Result<(), ExecutorError> {
    let stop = state
        .protection
        .get(&observation.instrument)
        .filter(|protection| !protection.resting.is_empty())
        .and_then(|protection| protection.prices)
        .map(|prices| prices.stop);
    if let (true, Some(mark), Some(stop)) = (observation.sane, observation.mark, stop)
        && mark <= stop
    {
        return Err(ExecutorError::Unimplemented { story: "E7-4" });
    }
    Ok(())
}

/// Whether protective orders rest in `instrument`.
fn rests(state: &ExecutorState, instrument: &InstrumentId) -> bool {
    state
        .protection
        .get(instrument)
        .is_some_and(|protection| !protection.resting.is_empty())
}

/// Whether an exit waits for its sequence's cancels to be confirmed: nothing is submitted beside
/// a protective order that has not been confirmed gone (§5.4).
pub(crate) fn awaits_cancel(
    state: &ExecutorState,
    instrument: &InstrumentId,
    purpose: Purpose,
) -> bool {
    purpose != Purpose::Protective
        && !purpose.adds_risk()
        && state.exiting.contains_key(instrument)
        && rests(state, instrument)
}

/// §5.4's marketable exit sequence, its first half, before the gate runs: the exit's instrument
/// has every resting protective order cancelled — attributed or not (DEC-160 3d) — with the
/// interval journaled from its start and the sequence (intent, entry, agent, prices) recorded
/// there, so a restart resumes it from the journal. Asked again after a restart, the cancels are
/// sent again. A passive exit (a sell limit above the latest bid) and an add are not this
/// sequence. Nothing is denied here (`AGENTS.md` rule 13).
pub(crate) fn begin_exit(
    batch: &mut Batch<'_, '_>,
    intent: &IntentId,
) -> Result<(), ExecutorError> {
    let (agent, body) = intent_of(batch, intent)?;
    let IntentBody::Order {
        instrument,
        limit,
        purpose,
        ..
    } = body
    else {
        return Ok(());
    };
    let passive = batch
        .view
        .quotes
        .get(&instrument)
        .and_then(|quote| quote.bid)
        .is_some_and(|bid| limit > bid);
    if purpose == Purpose::Protective || purpose.adds_risk() || passive {
        return Ok(());
    }
    let Some(protection) = batch.view.protection.get(&instrument).cloned() else {
        return Ok(());
    };
    if protection.resting.is_empty() {
        return Ok(());
    }
    if !batch.view.exiting.contains_key(&instrument) {
        let entry = match protection
            .resting
            .iter()
            .find_map(ClientOrderId::protected_entry)
        {
            Some(entry) => entry,
            None => ClientOrderId::for_intent(intent)?,
        };
        let mut pairs = vec![
            ("instrument", text(instrument.as_str())),
            ("action", text("unprotected_start")),
            ("orders", text(named(&protection.resting))),
            ("intent_id", text(intent.0.0.clone())),
            ("entry", text(entry.as_str())),
            ("agent", text(agent.0.clone())),
        ];
        if let Some(prices) = protection.prices {
            pairs.push(("stop", text(prices.stop.to_string())));
            if let Some(take_profit) = prices.take_profit {
                pairs.push(("take_profit", text(take_profit.to_string())));
            }
        }
        batch.journal("ProtectionChanged", None, pairs)?;
    }
    for id in &protection.resting {
        let asking = batch
            .view
            .orders
            .get(id)
            .is_some_and(|order| !order.cancel_unconfirmed && !order.state.is_terminal());
        if asking {
            transition(
                batch,
                id,
                OrderState::PendingCancel,
                vec![("cancel_requested", Value::Bool(true))],
            )?;
        }
    }
    for id in protection.resting {
        batch.broker(BrokerRequest::Cancel {
            client_order_id: id,
        });
    }
    Ok(())
}

/// A protective order's cancel confirmed (§5.4): it leaves the instrument's protection, and once
/// none rests the sequence's waiting exits are gated again on that fresh state and submitted.
pub(crate) fn protection_cancelled(
    batch: &mut Batch<'_, '_>,
    instrument: &InstrumentId,
    id: &ClientOrderId,
) -> Result<(), ExecutorError> {
    batch.journal(
        "ProtectionChanged",
        None,
        vec![
            ("instrument", text(instrument.as_str())),
            ("action", text("cancelled")),
            ("orders", text(id.as_str())),
        ],
    )?;
    if rests(&batch.view, instrument) {
        return Ok(());
    }
    let waiting: Vec<IntentId> = batch
        .view
        .intents
        .values()
        .filter(|record| record.outcome == IntentOutcome::Received)
        .map(|record| record.intent_id.clone())
        .filter(|intent| {
            ClientOrderId::for_intent(intent).is_ok_and(|id| !batch.view.orders.contains_key(&id))
                && batch.view.bodies.get(intent).is_some_and(|body| {
                    matches!(body, IntentBody::Order { instrument: named, .. } if named == instrument)
                })
        })
        .collect();
    for intent in waiting {
        gate_and_submit(batch, &intent)?;
    }
    Ok(())
}

/// The sequence's last step (§5.4): once no protective order rests and the exit is finished — its
/// order terminal, or its intent denied or abandoned — protection is re-placed for whatever the
/// position still holds, at the prices the sequence recorded, and the interval ends. A flat
/// position ends it with nothing to place; a sequence with no take-profit to re-use (crypto's,
/// slice 5) stays open, bounded and alerted by [`bound`].
pub(crate) fn settle(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let sequences: Vec<(InstrumentId, ExitSequence)> = batch
        .view
        .exiting
        .iter()
        .map(|(instrument, sequence)| (instrument.clone(), sequence.clone()))
        .collect();
    for (instrument, sequence) in sequences {
        if rests(&batch.view, &instrument) {
            continue;
        }
        let finished = match batch
            .view
            .orders
            .get(&ClientOrderId::for_intent(&sequence.intent)?)
        {
            Some(order) => order.state.is_terminal(),
            None => batch
                .view
                .intents
                .get(&sequence.intent)
                .is_some_and(|record| {
                    matches!(
                        record.outcome,
                        IntentOutcome::Denied | IntentOutcome::Abandoned
                    )
                }),
        };
        if finished {
            replace(batch, &instrument, &sequence)?;
        }
    }
    Ok(())
}

fn replace(
    batch: &mut Batch<'_, '_>,
    instrument: &InstrumentId,
    sequence: &ExitSequence,
) -> Result<(), ExecutorError> {
    let held = batch
        .view
        .positions
        .get(instrument)
        .copied()
        .unwrap_or(SignedQty::ZERO);
    let end = |batch: &mut Batch<'_, '_>| {
        batch.journal(
            "ProtectionChanged",
            None,
            vec![
                ("instrument", text(instrument.as_str())),
                ("action", text("unprotected_end")),
            ],
        )
    };
    if held == SignedQty::ZERO || held.is_negative() {
        end(batch)?;
        return Ok(());
    }
    let Some((stop, take_profit)) = sequence.prices.and_then(|prices| {
        prices
            .take_profit
            .map(|take_profit| (prices.stop, take_profit))
    }) else {
        return Ok(());
    };
    let qty = held.abs();
    let request = SubmitOrder {
        client_order_id: ClientOrderId::for_protection(&sequence.entry, &batch.id_after(1))?,
        instrument: instrument.clone(),
        side: Side::Sell,
        qty,
        order_type: OrderType::Limit,
        tif: TimeInForce::Gtc,
        limit_price: None,
        stop_price: None,
        bracket: None,
        oco: Some(OcoLegs {
            take_profit,
            stop,
            qty,
        }),
        extended_hours: false,
        purpose: Purpose::Protective,
    };
    journal_submission(batch, &request, None, &sequence.agent, 1)?;
    batch.journal(
        "ProtectionChanged",
        None,
        vec![
            ("instrument", text(instrument.as_str())),
            ("action", text("placed")),
            ("orders", text(request.client_order_id.as_str())),
            ("qty", text(qty.to_string())),
            ("stop", text(stop.to_string())),
            ("take_profit", text(take_profit.to_string())),
        ],
    )?;
    end(batch)?;
    batch.broker(BrokerRequest::Submit(request));
    Ok(())
}

/// §5.4's bounded interval: one open for `max_unprotected_s` has the sequence's working exit
/// cancelled — its confirmation then re-places protection through [`settle`] — and the owner
/// alerted, once, under a record the fold marks alerted.
pub(crate) fn bound(batch: &mut Batch<'_, '_>) -> Result<(), ExecutorError> {
    let now = batch.at().secs();
    let limit = batch.ports.config.max_unprotected_s;
    let due: Vec<InstrumentId> = batch
        .view
        .unprotected
        .iter()
        .filter(|interval| {
            interval.ended_at.is_none()
                && !interval.alerted
                && now.saturating_sub(interval.started_at.secs()) >= limit
        })
        .map(|interval| interval.instrument.clone())
        .collect();
    for instrument in due {
        let exit = batch
            .view
            .exiting
            .get(&instrument)
            .map(|sequence| ClientOrderId::for_intent(&sequence.intent))
            .transpose()?;
        if let Some(id) = exit
            && batch
                .view
                .orders
                .get(&id)
                .is_some_and(|order| !order.state.is_terminal() && !order.cancel_unconfirmed)
        {
            transition(
                batch,
                &id,
                OrderState::PendingCancel,
                vec![("cancel_requested", Value::Bool(true))],
            )?;
            batch.broker(BrokerRequest::Cancel {
                client_order_id: id,
            });
        }
        let alerted = batch.journal(
            "ProtectionChanged",
            None,
            vec![
                ("instrument", text(instrument.as_str())),
                ("action", text("interval_limit")),
            ],
        )?;
        batch.notify(alerted, "unprotected_interval_limit");
    }
    Ok(())
}

fn named(ids: &[ClientOrderId]) -> String {
    ids.iter()
        .map(ClientOrderId::as_str)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Where a ladder price came from, so a journaled exit says which of §5.6's three fallbacks was
/// used rather than only what it priced at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LadderReference {
    /// The best bid of a fresh, sane quote.
    FreshQuote,
    /// The last sane bid within the last five minutes.
    LastSaneBid,
    /// The last trade.
    LastTrade,
}

/// One rung of the exit price ladder: the limit to submit, which reference it came from, which
/// step this is, and whether the offset has reached `max_exit_offset` — the floor at which the
/// order rests and the owner is alerted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LadderPrice {
    pub limit: Price,
    pub reference: LadderReference,
    pub step: u32,
    pub at_floor: bool,
}

/// Prices one rung of trading-domain spec §5.6's ladder for a sell (a buy is symmetric).
///
/// The reference bid is the best bid of a fresh, sane quote; failing that the last sane bid
/// within five minutes; failing that the last trade. The limit is reference × (1 − offset),
/// rounded per §2.1. A step happens only after `exit_step_s` has elapsed, with the offset raised
/// by `exit_offset_step` and repriced from the **current** reference, and the offset never
/// exceeds `max_exit_offset`.
///
/// An owner exit outside the regular session prices from the bid the owner confirmed and never
/// below the floor that `OwnerExitRequested` carries (§5.5, mandate spec §6.1).
#[allow(
    dead_code,
    reason = "the tests PR ships the call site and its contract; `handle` and `reconcile` call it in the implementation PR (DEC-77, DEC-83)"
)]
pub(crate) fn ladder_price(
    tier: ExitTier,
    observations: &[MarketObservation],
    step: u32,
    now: RiskClock,
    floor: Option<Price>,
) -> Result<LadderPrice, ExecutorError> {
    let _ = (tier, observations, step, now, floor);
    Err(ExecutorError::Unimplemented { story: "E7-4" })
}

/// Whether one instrument's position is fully covered by resting protective orders right now: the
/// public probe the tracer's step 17 reads (#171).
///
/// Only the whole-share part of a fractional equity position can be protected, and the fraction is
/// disclosed rather than hidden (§5.4, slice 5); a crypto stop-limit covers the whole position. A
/// flat instrument needs nothing. An instrument whose asset class or increment the snapshot does
/// not know is reported unprotected, never guessed covered (`AGENTS.md` rule 3).
pub fn is_protected(
    state: &ExecutorState,
    instrument: &InstrumentId,
    ports: &Ports<'_>,
) -> Result<bool, ExecutorError> {
    let held = state
        .positions
        .get(instrument)
        .copied()
        .unwrap_or(SignedQty::ZERO);
    if held == SignedQty::ZERO {
        return Ok(true);
    }
    if held.is_negative() {
        return Ok(false);
    }
    let whole = match (
        ports.instruments.asset_class(instrument),
        ports.instruments.increment(instrument),
    ) {
        (Some(AssetClass::Crypto), _) => held.abs(),
        (Some(_), Some(increment)) => match increment {
            ShareIncrement::Whole => held.abs(),
            ShareIncrement::Fractional => {
                held.abs().portion(Fraction::ONE, ShareIncrement::Whole)?
            }
        },
        _ => return Ok(false),
    };
    Ok(state.protective_sell_qty(instrument)? >= whole)
}

/// Whether an exit of this purpose may be held at all. `AGENTS.md` rule 13 names exactly four
/// holds — agent mode `paused`, agent mode `stopped`, an `Unknown` order in the same instrument,
/// and the broker — and no pacing control is among them.
#[allow(
    dead_code,
    reason = "the tests PR ships the call site and its contract; `handle` and `reconcile` call it in the implementation PR (DEC-77, DEC-83)"
)]
pub(crate) fn exit_hold(
    state: &ExecutorState,
    instrument: &InstrumentId,
    purpose: Purpose,
) -> Result<Option<&'static str>, ExecutorError> {
    let _ = (state, instrument, purpose);
    Err(ExecutorError::Unimplemented { story: "E7-4" })
}

#[cfg(test)]
mod stub_tests {
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    use mandate_accounting::InstrumentId;
    use mandate_num::Qty;

    use super::{sequenced, watched};
    use crate::error::ExecutorError;
    use crate::state::ExecutorState;
    use crate::types::{
        AccountRef, AccountScope, MarketObservation, Protection, Purpose, RiskClock, WorkspaceId,
    };

    /// The files that may name the protection event without writing it, each with how many times:
    /// the executor's fold (the effect-free reader, which can journal nothing, so any count), the
    /// journal's catalogue (the registry of event types), and the runtime's list of account-stream
    /// types it reads. A new name in any of them, or anywhere else, fails the pin.
    const READERS: [(&str, Option<usize>); 3] = [
        ("crates/mandate-executor/src/fold.rs", None),
        ("crates/mandate-journal/src/catalogue.rs", Some(1)),
        ("crates/mandate-runtime/src/state.rs", Some(1)),
    ];

    /// Every production source under `dir`: Rust and Python files outside test directories.
    fn sources(dir: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() {
                if !matches!(name, "tests" | "target" | ".venv" | "__pycache__") {
                    sources(&path, found)?;
                }
            } else if (name.ends_with(".rs") || name.ends_with(".py")) && !name.starts_with("test_")
            {
                found.push(path);
            }
        }
        Ok(())
    }

    /// Nothing anywhere in the workspace — the executor, the connectors, the journal, the shell,
    /// the runtime, the Python tools — writes `ProtectionChanged` while `sequenced` answers a stub,
    /// so no exit in a protected instrument can reach it (`AGENTS.md` rule 13). Slice 2, the first
    /// writer, lands no earlier than slice 3, which replaces the stub, and deletes this pin with it
    /// (DEC-160).
    /// #174 ruling (b), 5861764910: slice 1 reads no leg id from a `BrokerOrder`. The broker's
    /// legs carry only their broker ids until the slice that reconciles legs keeps each leg's
    /// `client_order_id` (with its own `ready()` tests correction), so until then a broker-reported
    /// leg falls to the single holder or fails closed (DEC-160 3a). Every source file of this
    /// crate is scanned, test modules included, for a read of the field.
    #[test]
    fn no_leg_id_is_read_from_a_broker_order() -> io::Result<()> {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let read = concat!(".", "legs");
        for entry in fs::read_dir(&src)? {
            let path = entry?.path();
            let source = fs::read_to_string(&path)?;
            assert!(
                !source.contains(read),
                "{} reads a broker order's legs; leg ids from the broker wait for the leg \
                 reconciliation slice (#174 ruling (b))",
                path.display()
            );
        }
        Ok(())
    }

    #[test]
    fn no_step_journals_protection_while_its_exits_answer_a_stub() -> io::Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut found = Vec::new();
        sources(&root.join("crates"), &mut found)?;
        sources(&root.join("python"), &mut found)?;
        assert!(
            found.len() > 100,
            "the scan reached the workspace: {} files",
            found.len()
        );
        let named = concat!("Protection", "Changed");
        for path in found {
            let source = fs::read_to_string(&path)?;
            let relative = path
                .strip_prefix(&root)
                .map_or(path.clone(), Path::to_path_buf);
            let relative = relative.to_string_lossy().replace('\\', "/");
            let quoted = format!("\"{named}\"");
            let count = if relative.ends_with(".py") {
                source.matches(named).count()
            } else {
                source.matches(quoted.as_str()).count()
            };
            let allowed = READERS
                .iter()
                .find(|(reader, _)| *reader == relative)
                .map_or(Some(0), |(_, count)| *count);
            assert!(
                allowed.is_none_or(|allowed| count == allowed),
                "{relative} names the protection event {count} time(s): a step that journals it \
                 makes `sequenced` reachable, and an exit in a protected instrument would answer \
                 a stub, a denied exit (AGENTS.md rule 13). Slice 2 writes it no earlier than \
                 slice 3 replaces the stub (DEC-160)"
            );
        }
        Ok(())
    }

    fn protected() -> Result<(ExecutorState, InstrumentId), ExecutorError> {
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        let aapl = InstrumentId::new("AAPL")?;
        state.protection.insert(
            aapl.clone(),
            Protection {
                instrument: aapl.clone(),
                resting: Vec::new(),
                covered_qty: Qty::parse("10")?,
                prices: None,
            },
        );
        Ok((state, aapl))
    }

    #[test]
    fn only_a_non_protective_order_in_a_protected_instrument_answers_the_sequence_stub()
    -> Result<(), ExecutorError> {
        let (state, aapl) = protected()?;
        let msft = InstrumentId::new("MSFT")?;
        let stub = Err(ExecutorError::Unimplemented { story: "E7-4" });
        for purpose in [
            Purpose::Open,
            Purpose::Increase,
            Purpose::RiskExit,
            Purpose::OwnerExit,
            Purpose::DiscretionaryExit,
            Purpose::Flatten,
        ] {
            assert_eq!(sequenced(&state, &aapl, purpose), stub, "{purpose:?}");
            assert_eq!(sequenced(&state, &msft, purpose), Ok(()), "{purpose:?}");
        }
        assert_eq!(sequenced(&state, &aapl, Purpose::Protective), Ok(()));
        Ok(())
    }

    #[test]
    fn only_a_quote_in_a_protected_instrument_answers_the_watchdog_stub()
    -> Result<(), ExecutorError> {
        let (state, aapl) = protected()?;
        let quote = |instrument: InstrumentId| MarketObservation {
            instrument,
            bid: None,
            bid_size: None,
            ask: None,
            last_trade: None,
            mark: None,
            sane: true,
            observed_at: RiskClock::from_secs(10),
        };
        assert_eq!(
            watched(&state, &quote(aapl)),
            Err(ExecutorError::Unimplemented { story: "E7-4" })
        );
        assert_eq!(watched(&state, &quote(InstrumentId::new("MSFT")?)), Ok(()));
        Ok(())
    }
}

#[cfg(test)]
mod probe_tests {
    use mandate_accounting::{
        AssetClass, Config, CryptoFees, EquityFees, InstrumentId, TafCapBasis,
    };
    use mandate_num::{
        Bps, FeeCap, FeePerShare, FeeRate, Fraction, Qty, ShareIncrement, SignedQty,
    };
    use mandate_time::{Date, TradingCalendar};

    use super::is_protected;
    use crate::error::ExecutorError;
    use crate::ports::{IdGen, InstrumentSnapshot, MandateView, Ports};
    use crate::state::ExecutorState;
    use crate::types::{
        AccountRef, AccountScope, AgentId, EventId, ExecutorConfig, ExitTier, MandateVersion,
        Protection, Seq, WorkspaceId, WriterEpoch,
    };

    struct Nothing;

    impl IdGen for Nothing {
        fn event_id(&self, _epoch: WriterEpoch, _head: Seq, _ordinal: u32) -> EventId {
            EventId("e".to_owned())
        }
    }

    impl MandateView for Nothing {
        fn version(&self, _agent: &AgentId) -> Option<MandateVersion> {
            None
        }

        fn crypto_stop_limit_offset(&self, _agent: &AgentId) -> Option<Fraction> {
            None
        }

        fn covers(&self, _agent: &AgentId, _instrument: &InstrumentId) -> bool {
            false
        }
    }

    /// `AAPL` whole shares, `FRAC` fractional, `BTC/USD` crypto; anything else unknown.
    struct Snapshot;

    impl InstrumentSnapshot for Snapshot {
        fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass> {
            match instrument.as_str() {
                "AAPL" | "FRAC" | "NOINC" => Some(AssetClass::UsEquity),
                "BTC/USD" => Some(AssetClass::Crypto),
                _ => None,
            }
        }

        fn increment(&self, instrument: &InstrumentId) -> Option<ShareIncrement> {
            match instrument.as_str() {
                "AAPL" => Some(ShareIncrement::Whole),
                "FRAC" | "BTC/USD" => Some(ShareIncrement::Fractional),
                _ => None,
            }
        }

        fn exit_tier(&self, _instrument: &InstrumentId) -> Option<ExitTier> {
            None
        }
    }

    fn fees() -> Result<Config, ExecutorError> {
        let date = |raw: &str| Date::parse(raw).map_err(ExecutorError::from);
        Ok(Config {
            equities: EquityFees {
                sec_rate: FeeRate::parse("0")?,
                taf_per_share: FeePerShare::parse("0")?,
                taf_cap: FeeCap::parse("0")?,
                taf_cap_basis: TafCapBasis::PerExecution,
                cat_per_share: FeePerShare::parse("0")?,
            },
            crypto: CryptoFees {
                maker: Bps::parse("0")?,
                taker: Bps::parse("0")?,
            },
            calendar: TradingCalendar::new(date("2026-09-01")?, date("2026-12-31")?, [], [])?,
        })
    }

    /// `held` of `name`, covered by `covered` of resting protection (none when `None`).
    fn probe(name: &str, held: &str, covered: Option<&str>) -> Result<bool, ExecutorError> {
        let fees = fees()?;
        let config = ExecutorConfig::PROPOSED;
        let ports = Ports {
            ids: &Nothing,
            mandates: &Nothing,
            instruments: &Snapshot,
            config: &config,
            fees: &fees,
        };
        let mut state = ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        });
        let instrument = InstrumentId::new(name)?;
        state
            .positions
            .insert(instrument.clone(), SignedQty::parse(held)?);
        if let Some(covered) = covered {
            state.protection.insert(
                instrument.clone(),
                Protection {
                    instrument: instrument.clone(),
                    resting: Vec::new(),
                    covered_qty: Qty::parse(covered)?,
                    prices: None,
                },
            );
        }
        is_protected(&state, &instrument, &ports)
    }

    #[test]
    fn a_flat_instrument_needs_nothing_and_a_short_is_never_protected() -> Result<(), ExecutorError>
    {
        assert!(probe("AAPL", "0", None)?);
        assert!(!probe("AAPL", "-1", Some("1"))?);
        Ok(())
    }

    #[test]
    fn whole_shares_are_protected_only_when_all_of_them_are_covered() -> Result<(), ExecutorError> {
        assert!(probe("AAPL", "10", Some("10"))?);
        assert!(!probe("AAPL", "10", Some("9"))?);
        assert!(!probe("AAPL", "10", None)?);
        Ok(())
    }

    #[test]
    fn only_the_whole_share_part_of_a_fractional_position_needs_cover() -> Result<(), ExecutorError>
    {
        assert!(
            probe("FRAC", "10.5", Some("10"))?,
            "the half share is disclosed, not protected"
        );
        assert!(!probe("FRAC", "10.5", Some("9"))?);
        Ok(())
    }

    #[test]
    fn a_crypto_position_is_covered_whole() -> Result<(), ExecutorError> {
        assert!(probe("BTC/USD", "0.5", Some("0.5"))?);
        assert!(
            !probe("BTC/USD", "0.5", Some("0.4"))?,
            "no whole-share floor for crypto"
        );
        Ok(())
    }

    #[test]
    fn an_instrument_the_snapshot_does_not_know_is_reported_unprotected()
    -> Result<(), ExecutorError> {
        assert!(!probe("ZZZZ", "1", Some("1"))?, "unknown asset class");
        assert!(
            !probe("NOINC", "1", Some("1"))?,
            "an equity with no known increment"
        );
        Ok(())
    }
}
