//! The protective sequences and the exit price ladder (E7-4, trading-domain spec §5.4 to §5.6).

use mandate_accounting::InstrumentId;
use mandate_num::Price;

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

/// Whether one instrument's position is fully covered by resting protective orders right now.
///
/// Only the whole-share part of a fractional position can be protected, and the fraction is
/// disclosed rather than hidden (§5.4).
#[allow(
    dead_code,
    reason = "the tests PR ships the call site and its contract; `handle` and `reconcile` call it in the implementation PR (DEC-77, DEC-83)"
)]
pub(crate) fn is_protected(
    state: &ExecutorState,
    instrument: &InstrumentId,
    ports: &Ports<'_>,
) -> Result<bool, ExecutorError> {
    let _ = (state, instrument, ports);
    Err(ExecutorError::Unimplemented { story: "E7-4" })
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
    use mandate_accounting::InstrumentId;
    use mandate_num::Qty;

    use super::{sequenced, watched};
    use crate::error::ExecutorError;
    use crate::state::ExecutorState;
    use crate::types::{
        AccountRef, AccountScope, MarketObservation, Protection, Purpose, RiskClock, WorkspaceId,
    };

    /// Every source file of this crate but `fold.rs`, the effect-free reader that can journal
    /// nothing. A new file joins this list, or the pin below stops covering it.
    const SOURCES: [(&str, &str); 15] = [
        ("batch.rs", include_str!("batch.rs")),
        ("codec.rs", include_str!("codec.rs")),
        ("error.rs", include_str!("error.rs")),
        ("gate.rs", include_str!("gate.rs")),
        ("ids.rs", include_str!("ids.rs")),
        ("intent.rs", include_str!("intent.rs")),
        ("lib.rs", include_str!("lib.rs")),
        ("orders.rs", include_str!("orders.rs")),
        ("payload.rs", include_str!("payload.rs")),
        ("ports.rs", include_str!("ports.rs")),
        ("protection.rs", include_str!("protection.rs")),
        ("reconcile.rs", include_str!("reconcile.rs")),
        ("state.rs", include_str!("state.rs")),
        ("step.rs", include_str!("step.rs")),
        ("types.rs", include_str!("types.rs")),
    ];

    #[test]
    fn no_step_journals_protection_while_its_exits_answer_a_stub() {
        let written = concat!("\"Protection", "Changed\"");
        for (name, source) in SOURCES {
            assert!(
                !source.contains(written),
                "{name} names the protection event: a step that journals it makes `sequenced` \
                 reachable, and an exit in a protected instrument would answer a stub, a denied \
                 exit (AGENTS.md rule 13). Slice 2 writes it no earlier than slice 3 replaces \
                 the stub (DEC-160)"
            );
        }
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
