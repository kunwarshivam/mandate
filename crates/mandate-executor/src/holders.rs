//! Who holds an instrument on the account (DEC-2849, trading domain spec §8.1): each agent's lots,
//! the owner's residual, the deficit, and what each agent may sell and protect. Every figure is a
//! function of the executor's state, so a replay reaches the same (I8).

use std::collections::BTreeMap;

use mandate_accounting::{InstrumentId, Side};
use mandate_num::{Qty, SignedQty};

use crate::error::ExecutorError;
use crate::protection::{long, sold, still_selling};
use crate::state::ExecutorState;
use crate::types::AgentId;

/// An instrument's holders (DEC-2849's definitions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Holders {
    /// Each agent with lots, Lᵢ > 0.
    pub(crate) lots: BTreeMap<AgentId, Qty>,
    /// R = N − Σᵢ Lᵢ, the owner's residual.
    pub(crate) residual: SignedQty,
    /// D = max(0, −R).
    pub(crate) deficit: Qty,
    /// N, the long position as the broker holds it ([`long`]).
    pub(crate) position: Qty,
}

/// `instrument`'s holders. Agent i's lots are its own orders' buy fills, less what its sells sold
/// ([`sold`]), what it released to the owner (DEC-2354) and what an account switch's close took
/// from it ([`ExecutorState::closed`]), floored at zero: a sell beyond them comes out of R.
pub(crate) fn holders(
    view: &ExecutorState,
    instrument: &InstrumentId,
) -> Result<Holders, ExecutorError> {
    let mut raw: BTreeMap<&AgentId, SignedQty> = BTreeMap::new();
    for (id, order) in view
        .orders
        .iter()
        .filter(|(_, order)| &order.instrument == instrument)
    {
        let Some(agent) = order.agent.as_ref() else {
            continue;
        };
        let entry = raw.entry(agent).or_insert(SignedQty::ZERO);
        *entry = entry.checked_add(match order.side {
            Side::Buy => SignedQty::from(order.filled_qty),
            Side::Sell => SignedQty::from(sold(view, id, order)).negated(),
        })?;
    }
    let mut lots = BTreeMap::new();
    for (agent, bought) in raw {
        let key = (agent.clone(), instrument.clone());
        let gone = [view.released.get(&key), view.closed.get(&key)]
            .into_iter()
            .flatten()
            .try_fold(Qty::ZERO, |sum, qty| sum.checked_add(*qty))?;
        let held = bought.checked_add(SignedQty::from(gone).negated())?;
        if held > SignedQty::ZERO {
            lots.insert(agent.clone(), held.abs());
        }
    }
    let position = long(view, instrument)?;
    let agents = lots
        .values()
        .try_fold(Qty::ZERO, |sum, qty| sum.checked_add(*qty))?;
    let residual = SignedQty::from(position).checked_add(SignedQty::from(agents).negated())?;
    let deficit = residual.min(SignedQty::ZERO).abs();
    Ok(Holders {
        lots,
        residual,
        deficit,
        position,
    })
}

/// I4: what a rule-13 sell for `agent` is sized to before the clip, min(Lᵢ, N): its sub-ledger
/// (trading domain spec §5.5).
pub(crate) fn sub_ledger(
    view: &ExecutorState,
    agent: &AgentId,
    instrument: &InstrumentId,
) -> Result<Qty, ExecutorError> {
    let holders = holders(view, instrument)?;
    Ok(lots_of(&holders, agent).min(holders.position))
}

/// I3: `agent`'s sell authority, Sᵢ = max(0, Lᵢ − D), for its discretionary and new sells (the
/// founder's "every agent shrinks by full gap", DEC-2849 item 6).
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "lane X sizes exits and legs on it (DEC-2849 items 7 and 8)"
    )
)]
pub(crate) fn sell_authority(
    view: &ExecutorState,
    agent: &AgentId,
    instrument: &InstrumentId,
) -> Result<Qty, ExecutorError> {
    let holders = holders(view, instrument)?;
    Ok(lots_of(&holders, agent)
        .checked_sub(holders.deficit)
        .unwrap_or(Qty::ZERO))
}

/// I9: every holder's protective coverage Pᵢ. The deficit is charged once, largest lots first,
/// ties by agent id, each taking as much of what is left as its lots allow, so Σᵢ Pᵢ is
/// min(N, Σᵢ Lᵢ).
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "lane X sizes exits and legs on it (DEC-2849 items 7 and 8)"
    )
)]
pub(crate) fn coverages(
    view: &ExecutorState,
    instrument: &InstrumentId,
) -> Result<BTreeMap<AgentId, Qty>, ExecutorError> {
    let holders = holders(view, instrument)?;
    let mut order: Vec<(&AgentId, Qty)> = holders
        .lots
        .iter()
        .map(|(agent, lots)| (agent, *lots))
        .collect();
    order.sort_by(|(a, x), (b, y)| y.cmp(x).then_with(|| a.cmp(b)));
    let mut left = holders.deficit;
    let mut covered = BTreeMap::new();
    for (agent, lots) in order {
        let charged = lots.min(left);
        left = left.checked_sub(charged)?;
        covered.insert(agent.clone(), lots.checked_sub(charged)?);
    }
    Ok(covered)
}

/// I9: `agent`'s protective coverage Pᵢ, from [`coverages`].
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "lane X sizes exits and legs on it (DEC-2849 items 7 and 8)"
    )
)]
pub(crate) fn coverage(
    view: &ExecutorState,
    agent: &AgentId,
    instrument: &InstrumentId,
) -> Result<Qty, ExecutorError> {
    Ok(coverages(view, instrument)?
        .get(agent)
        .copied()
        .unwrap_or(Qty::ZERO))
}

/// What no agent's coverage holds, N − Σᵢ Pᵢ: the owner's residual when it is positive, else
/// nothing.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "lane X sizes exits and legs on it (DEC-2849 items 7 and 8)"
    )
)]
pub(crate) fn uncovered(
    view: &ExecutorState,
    instrument: &InstrumentId,
) -> Result<Qty, ExecutorError> {
    let covered = coverages(view, instrument)?
        .values()
        .try_fold(Qty::ZERO, |sum, qty| sum.checked_add(*qty))?;
    Ok(long(view, instrument)?
        .checked_sub(covered)
        .unwrap_or(Qty::ZERO))
}

/// I4's clip: N less every working sell in `instrument` as [`still_selling`] counts them, never
/// below zero.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "lane X sizes exits and legs on it (DEC-2849 items 7 and 8)"
    )
)]
pub(crate) fn available_to_sell(
    view: &ExecutorState,
    instrument: &InstrumentId,
) -> Result<Qty, ExecutorError> {
    Ok(long(view, instrument)?
        .checked_sub(still_selling(view, instrument)?)
        .unwrap_or(Qty::ZERO))
}

/// Takes `quantity` of an account switch's close in `instrument` out of its holders, for
/// [`ExecutorState::closed`] (DEC-2849 I5): out of the owner's positive residual first, then the
/// agents' lots in [`coverages`]' order, largest first and ties by agent id. Read before the close's
/// fill is applied.
pub(crate) fn close_takes(
    view: &ExecutorState,
    instrument: &InstrumentId,
    quantity: Qty,
) -> Result<Vec<(AgentId, Qty)>, ExecutorError> {
    let holders = holders(view, instrument)?;
    let owners = holders.residual.max(SignedQty::ZERO).abs();
    let mut left = quantity.checked_sub(owners).unwrap_or(Qty::ZERO);
    let mut order: Vec<(&AgentId, Qty)> = holders
        .lots
        .iter()
        .map(|(agent, lots)| (agent, *lots))
        .collect();
    order.sort_by(|(a, x), (b, y)| y.cmp(x).then_with(|| a.cmp(b)));
    let mut takes = Vec::new();
    for (agent, lots) in order {
        let taken = lots.min(left);
        if taken > Qty::ZERO {
            left = left.checked_sub(taken)?;
            takes.push((agent.clone(), taken));
        }
    }
    Ok(takes)
}

fn lots_of(holders: &Holders, agent: &AgentId) -> Qty {
    holders.lots.get(agent).copied().unwrap_or(Qty::ZERO)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use mandate_accounting::{InstrumentId, Side};
    use mandate_num::{Qty, SignedQty};
    use proptest::prelude::*;

    use super::{
        available_to_sell, close_takes, coverage, coverages, holders, sell_authority, sub_ledger,
        uncovered,
    };
    use crate::error::ExecutorError;
    use crate::ids::ClientOrderId;
    use crate::state::ExecutorState;
    use crate::types::{
        AccountRef, AccountScope, AgentId, Order, OrderState, Purpose, WorkspaceId,
    };

    const AGENTS: u8 = 3;

    fn state() -> ExecutorState {
        ExecutorState::new(AccountScope {
            account: AccountRef("acct-1".to_owned()),
            workspace: WorkspaceId("ws1".to_owned()),
        })
    }

    fn shares(n: i64) -> Result<Qty, ExecutorError> {
        Ok(Qty::parse(&n.max(0).to_string())?)
    }

    fn agent(index: u8) -> AgentId {
        AgentId(format!("agent-{index}"))
    }

    /// One generated event on one instrument (DEC-2849's fuzz): an agent's buy or sell filled, an
    /// owner's buy or sell outside Mandate, a release from an agent to the owner, or a fill of the
    /// account switch's close.
    #[derive(Debug, Clone)]
    enum Event {
        Buy(u8, i64),
        Sell(u8, i64),
        OwnerBuy(i64),
        OwnerSell(i64),
        Release(u8, i64),
        Close(i64),
    }

    fn event() -> impl Strategy<Value = Event> {
        prop_oneof![
            (0_u8..AGENTS, 1_i64..=20).prop_map(|(a, q)| Event::Buy(a, q)),
            (0_u8..AGENTS, 1_i64..=20).prop_map(|(a, q)| Event::Sell(a, q)),
            (1_i64..=20).prop_map(Event::OwnerBuy),
            (1_i64..=20).prop_map(Event::OwnerSell),
            (0_u8..AGENTS, 1_i64..=20).prop_map(|(a, q)| Event::Release(a, q)),
            (1_i64..=20).prop_map(Event::Close),
        ]
    }

    /// The oracle: each holder's whole shares, written apart from `holders`. `lots` per agent and
    /// `owner`, the owner's residual, which may go negative; the position is their sum.
    #[derive(Debug, Default)]
    struct Oracle {
        lots: BTreeMap<u8, i64>,
        owner: i64,
    }

    impl Oracle {
        fn lots(&self, a: u8) -> i64 {
            self.lots.get(&a).copied().unwrap_or(0)
        }

        fn add(&mut self, a: u8, q: i64) {
            let lots = self.lots.entry(a).or_insert(0);
            *lots = lots.saturating_add(q);
        }

        fn agents(&self) -> i64 {
            self.lots.values().fold(0, |sum, q| sum.saturating_add(*q))
        }

        fn position(&self) -> i64 {
            self.agents().saturating_add(self.owner)
        }

        fn deficit(&self) -> i64 {
            self.owner.saturating_neg().max(0)
        }

        /// The agents in coverage order: largest lots first, ties by agent id.
        fn order(&self) -> Vec<u8> {
            let mut order: Vec<u8> = (0..AGENTS).filter(|a| self.lots(*a) > 0).collect();
            order.sort_by(|a, b| self.lots(*b).cmp(&self.lots(*a)).then(a.cmp(b)));
            order
        }

        fn coverage(&self) -> BTreeMap<u8, i64> {
            let mut left = self.deficit();
            let mut covered = BTreeMap::new();
            for a in self.order() {
                let charged = self.lots(a).min(left);
                covered.insert(a, self.lots(a).saturating_sub(charged));
                left = left.saturating_sub(charged);
            }
            covered
        }

        /// What a close of `q` takes from each agent: the owner's positive residual first.
        fn close(&self, q: i64) -> BTreeMap<u8, i64> {
            let mut left = q.saturating_sub(self.owner.max(0)).max(0);
            let mut taken = BTreeMap::new();
            for a in self.order() {
                let take = self.lots(a).min(left);
                taken.insert(a, take);
                left = left.saturating_sub(take);
            }
            taken
        }
    }

    /// Applies `event` to the oracle and to `view` the way the executor's fold would: an agent's
    /// fill as its order, the owner's as the position alone, a release as `released`, and a
    /// close's fill as [`close_takes`] read before the position falls. An event the account could
    /// not have (a sale of more than is held) is skipped.
    fn apply(
        oracle: &mut Oracle,
        view: &mut ExecutorState,
        event: &Event,
        id: &mut u64,
    ) -> Result<(), ExecutorError> {
        let instrument = InstrumentId::new("AAPL")?;
        let move_position = |view: &mut ExecutorState, delta: i64| -> Result<(), ExecutorError> {
            let entry = view
                .positions
                .entry(instrument.clone())
                .or_insert(SignedQty::ZERO);
            let change = SignedQty::from(shares(delta.saturating_abs())?);
            *entry = entry.checked_add(if delta < 0 { change.negated() } else { change })?;
            Ok(())
        };
        match *event {
            Event::Buy(a, q) | Event::Sell(a, q) => {
                let buy = matches!(event, Event::Buy(..));
                if !buy && (q > oracle.lots(a) || q > oracle.position()) {
                    return Ok(());
                }
                *id = id.saturating_add(1);
                let client_order_id = ClientOrderId::parse(&format!("md-{id}-{a}"))?;
                view.orders.insert(
                    client_order_id.clone(),
                    Order {
                        client_order_id,
                        intent_id: None,
                        agent: Some(agent(a)),
                        instrument: instrument.clone(),
                        side: if buy { Side::Buy } else { Side::Sell },
                        qty: shares(q)?,
                        filled_qty: shares(q)?,
                        state: OrderState::Filled,
                        attempt: 1,
                        purpose: Purpose::Open,
                        absent_lookups: 0,
                        first_absence_at: None,
                        cancel_unconfirmed: false,
                        replaced_by: None,
                        created_on: None,
                    },
                );
                let signed = if buy { q } else { q.saturating_neg() };
                move_position(view, signed)?;
                oracle.add(a, signed);
            }
            Event::OwnerBuy(q) | Event::OwnerSell(q) => {
                let buy = matches!(event, Event::OwnerBuy(_));
                if !buy && q > oracle.position() {
                    return Ok(());
                }
                let signed = if buy { q } else { q.saturating_neg() };
                move_position(view, signed)?;
                oracle.owner = oracle.owner.saturating_add(signed);
            }
            Event::Release(a, q) => {
                let q = q.min(oracle.lots(a));
                if q == 0 {
                    return Ok(());
                }
                let released = view
                    .released
                    .entry((agent(a), instrument.clone()))
                    .or_insert(Qty::ZERO);
                *released = released.checked_add(shares(q)?)?;
                oracle.add(a, q.saturating_neg());
                oracle.owner = oracle.owner.saturating_add(q);
            }
            Event::Close(q) => {
                let q = q.min(oracle.position());
                if q <= 0 {
                    return Ok(());
                }
                let expected = oracle.close(q);
                for (taken_by, taken) in close_takes(view, &instrument, shares(q)?)? {
                    let closed = view
                        .closed
                        .entry((taken_by, instrument.clone()))
                        .or_insert(Qty::ZERO);
                    *closed = closed.checked_add(taken)?;
                }
                move_position(view, q.saturating_neg())?;
                let from_agents = expected
                    .values()
                    .fold(0_i64, |sum, t| sum.saturating_add(*t));
                for (a, taken) in expected {
                    oracle.add(a, taken.saturating_neg());
                }
                oracle.owner = oracle.owner.saturating_sub(q.saturating_sub(from_agents));
            }
        }
        Ok(())
    }

    /// DEC-2849 I1, I3, I5 and I9 against the oracle, after every event.
    fn check(oracle: &Oracle, view: &ExecutorState) -> Result<(), TestCaseError> {
        let fail = |e: ExecutorError| TestCaseError::fail(e.to_string());
        let aapl = InstrumentId::new("AAPL").map_err(|e| TestCaseError::fail(e.to_string()))?;
        let held = holders(view, &aapl).map_err(fail)?;
        let n = |v: i64| shares(v).map_err(fail);
        let mut want = BTreeMap::new();
        for a in 0..AGENTS {
            if oracle.lots(a) > 0 {
                want.insert(agent(a), n(oracle.lots(a))?);
            }
        }
        prop_assert_eq!(&held.lots, &want, "lots");
        prop_assert_eq!(held.position, n(oracle.position())?, "N");
        prop_assert_eq!(held.deficit, n(oracle.deficit())?, "D");
        let covered = oracle.coverage();
        let mut sum_s = Qty::ZERO;
        let mut sum_p = Qty::ZERO;
        for a in 0..AGENTS {
            let s = sell_authority(view, &agent(a), &aapl).map_err(fail)?;
            prop_assert_eq!(
                s,
                n(oracle.lots(a).saturating_sub(oracle.deficit()))?,
                "S of {}",
                a
            );
            let p = coverage(view, &agent(a), &aapl).map_err(fail)?;
            prop_assert_eq!(p, n(covered.get(&a).copied().unwrap_or(0))?, "P of {}", a);
            let l = sub_ledger(view, &agent(a), &aapl).map_err(fail)?;
            prop_assert_eq!(
                l,
                n(oracle.lots(a).min(oracle.position()))?,
                "sub-ledger of {}",
                a
            );
            sum_s = sum_s.checked_add(s).map_err(|e| fail(e.into()))?;
            sum_p = sum_p.checked_add(p).map_err(|e| fail(e.into()))?;
        }
        prop_assert!(sum_s <= held.position, "Σ S ≤ N");
        prop_assert_eq!(
            sum_p,
            n(oracle.position().min(oracle.agents()))?,
            "Σ P = min(N, Σ L)"
        );
        let rest = uncovered(view, &aapl).map_err(fail)?;
        prop_assert_eq!(
            sum_p.checked_add(rest).map_err(|e| fail(e.into()))?,
            held.position,
            "Σ P + U = N"
        );
        prop_assert_eq!(coverages(view, &aapl).map_err(fail)?.len(), want.len());
        prop_assert_eq!(available_to_sell(view, &aapl).map_err(fail)?, held.position);
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// DEC-2849's attribution over random agent fills, owner trades outside Mandate, releases
        /// and account closes on three agents: every holder's lots, the deficit, sell authority,
        /// coverage and sub-ledger are the oracle's, Σ Sᵢ ≤ N, Σ Pᵢ = min(N, Σ Lᵢ), and Σ Pᵢ + U = N.
        #[test]
        fn holders_are_the_oracles(events in proptest::collection::vec(event(), 1..60)) {
            let mut oracle = Oracle::default();
            let mut view = state();
            let mut id = 0;
            for event in &events {
                apply(&mut oracle, &mut view, event, &mut id)
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                check(&oracle, &view)?;
            }
        }
    }

    /// DEC-2849's worked example: A and B each hold 5, and the owner sells 4 outside Mandate.
    #[test]
    fn the_worked_example() -> Result<(), ExecutorError> {
        let mut oracle = Oracle::default();
        let mut view = state();
        let mut id = 0;
        for event in [Event::Buy(0, 5), Event::Buy(1, 5), Event::OwnerSell(4)] {
            apply(&mut oracle, &mut view, &event, &mut id)?;
        }
        let aapl = InstrumentId::new("AAPL")?;
        assert_eq!(holders(&view, &aapl)?.deficit, shares(4)?);
        for (a, s, p) in [(0, 1, 1), (1, 1, 5)] {
            assert_eq!(sell_authority(&view, &agent(a), &aapl)?, shares(s)?);
            assert_eq!(coverage(&view, &agent(a), &aapl)?, shares(p)?);
            assert_eq!(sub_ledger(&view, &agent(a), &aapl)?, shares(5)?);
        }
        apply(&mut oracle, &mut view, &Event::Release(0, 4), &mut id)?;
        assert_eq!(
            holders(&view, &aapl)?.deficit,
            Qty::ZERO,
            "a release ends it"
        );
        Ok(())
    }
}
