//! The protective sequences and the exit price ladder (E7-4, trading-domain spec §5.4 to §5.6).

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_num::{Fraction, Price, ShareIncrement, SignedQty};

use crate::error::ExecutorError;
use crate::ports::Ports;
use crate::state::ExecutorState;
use crate::types::{ExitTier, MarketObservation, Purpose, RiskClock};

/// The entry point of §5.4's sequences for an order the gate allowed. An order in an instrument
/// whose protection rests is cancel protection, confirm, re-gate, submit, re-place (E7-4 slices
/// 2, 3 and 5), never an order sent beside the resting legs, which could oversell the position or
/// leave it unprotected; until those slices land it answers their stub. A protective order is the
/// re-placement itself and passes.
///
/// **`AGENTS.md` rule 13 while the stub stands:** an exit in a protected instrument answers it,
/// which, once reachable, would deny that exit. It is unreachable only while nothing journals
/// `ProtectionChanged` — the fold reads it, no step writes it — which
/// `no_step_journals_protection_while_its_exits_answer_a_stub` pins. Slice 2, the first writer
/// (brackets), therefore lands no earlier than slice 3, which replaces this stub (DEC-160).
pub(crate) fn sequenced(
    state: &ExecutorState,
    instrument: &InstrumentId,
    purpose: Purpose,
) -> Result<(), ExecutorError> {
    if purpose != Purpose::Protective && state.protection.contains_key(instrument) {
        return Err(ExecutorError::Unimplemented { story: "E7-4" });
    }
    Ok(())
}

/// A market observation in an instrument whose protection rests is the input of the triggered-stop
/// watchdog and of crypto's watched take-profit (§5.4, E7-4 slices 4 and 5); until those land it
/// answers their stub rather than letting a stop at its trigger go unwatched. Unreachable for the
/// same reason as [`sequenced`]. Elsewhere a quote carries nothing the account stream folds.
pub(crate) fn watched(
    state: &ExecutorState,
    observation: &MarketObservation,
) -> Result<(), ExecutorError> {
    if state.protection.contains_key(&observation.instrument) {
        return Err(ExecutorError::Unimplemented { story: "E7-4" });
    }
    Ok(())
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
