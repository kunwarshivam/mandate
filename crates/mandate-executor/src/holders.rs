//! Who holds an instrument on the account (DEC-2849, trading domain spec §8.1): each agent's lots,
//! the owner's residual, the deficit, and what each agent may sell and protect. Every figure is a
//! function of the executor's state, so a replay reaches the same (I8).

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{InstrumentId, Side};
use mandate_num::{Qty, SignedQty};

use crate::error::ExecutorError;
use crate::ids::{ClientOrderId, IntentId};
use crate::protection::{long, remainders, sold, still_selling};
use crate::state::{EVERY_AGENT, ExecutorState};
use crate::types::{AgentId, IntentBody, Order, OrderState, Purpose};

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
/// from it ([`ExecutorState::closed`]), plus what a sell fill or release took beyond its lots as it
/// was folded ([`ExecutorState::oversold`]): a sell beyond them comes out of R, and never out of a
/// later buy. A sell's broker-reported fill that no fill applied ([`sold`] on an ended order) is
/// not folded as an event, so the sum is also floored at zero.
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
        let back = view.oversold.get(&key).copied().unwrap_or(Qty::ZERO);
        let held = bought
            .checked_add(SignedQty::from(gone).negated())?
            .checked_add(SignedQty::from(back))?;
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

/// I3: what a discretionary exit for `agent` may still sell in `instrument`: its sell authority
/// [`sell_authority`] less its own working sells, as the gate's `available` counts sells (sent,
/// not protective, not ended), and less what its own exit ladders' stepped rungs left to send
/// ([`remainders`]) but `except`'s, the ladder asking, never below zero. The founder's reading
/// (DEC-2849 I3): an exit for more is trimmed to this, and one with nothing left is refused
/// `sell_exceeds_available`.
pub(crate) fn discretionary_room(
    view: &ExecutorState,
    agent: &AgentId,
    instrument: &InstrumentId,
    except: Option<&IntentId>,
) -> Result<Qty, ExecutorError> {
    let working = view
        .orders
        .values()
        .filter(|order| {
            &order.instrument == instrument
                && order.agent.as_ref() == Some(agent)
                && order.side == Side::Sell
                && order.purpose != Purpose::Protective
                && !order.state.is_terminal()
                && order.state != OrderState::Intent
        })
        .try_fold(Qty::ZERO, |total, order| {
            total.checked_add(order.qty.checked_sub(order.filled_qty).unwrap_or(Qty::ZERO))
        })?;
    let laddered = remainders(view, instrument)?
        .into_iter()
        .filter(|(intent, _)| {
            Some(intent) != except && ladder_agent(view, instrument, intent) == Some(agent)
        })
        .try_fold(Qty::ZERO, |total, (_, left)| total.checked_add(left))?;
    Ok(sell_authority(view, agent, instrument)?
        .checked_sub(working.checked_add(laddered)?)
        .unwrap_or(Qty::ZERO))
}

/// I3 for an exit ladder's next rung: `qty`, and for a discretionary exit no more than its agent's
/// [`discretionary_room`] beside its own ladder, so a deficit that arose after the exit was gated
/// trims each later rung too.
pub(crate) fn rung_room(
    view: &ExecutorState,
    instrument: &InstrumentId,
    intent: &IntentId,
    qty: Qty,
) -> Result<Qty, ExecutorError> {
    let discretionary = matches!(
        view.bodies.get(intent),
        Some(IntentBody::Order {
            purpose: Purpose::DiscretionaryExit,
            ..
        })
    );
    match ladder_agent(view, instrument, intent) {
        Some(agent) if discretionary => {
            Ok(qty.min(discretionary_room(view, agent, instrument, Some(intent))?))
        }
        _ => Ok(qty),
    }
}

/// The agent whose exit ladder `intent` is in `instrument`: its exit sequence's, or its lone
/// ladder's.
fn ladder_agent<'s>(
    view: &'s ExecutorState,
    instrument: &InstrumentId,
    intent: &IntentId,
) -> Option<&'s AgentId> {
    view.exiting
        .get(instrument)
        .filter(|sequence| &sequence.intent == intent)
        .map(|sequence| &sequence.agent)
        .or_else(|| {
            view.ladders
                .get(&(instrument.clone(), intent.clone()))
                .map(|lone| &lone.agent)
        })
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

/// I9 for the legs re-placement sends (DEC-2849 item 8): each holder's gap, its coverage Pᵢ less
/// its own resting protective legs and its own working exits, floored at zero, for every holder
/// with a gap. The owner's residual R is in no holder's gap, so no agent's legs cover the owner's
/// shares, and an unattributed leg counts in no holder's legs.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "pending E7-4 (DEC-2849 item 8): re-placement sends each holder's legs on it"
    )
)]
pub(crate) fn leg_gaps(
    _view: &ExecutorState,
    _instrument: &InstrumentId,
) -> Result<BTreeMap<AgentId, Qty>, ExecutorError> {
    Err(ExecutorError::Unimplemented { story: "E7-4" })
}

/// I9 for the legs a resize cuts (DEC-2849 item 8): each agent's resting protective legs beyond its
/// coverage Pᵢ, for every agent with an excess, an agent whose lots are gone included. So the
/// broker-side sum of resting legs comes back within N once the executor has read the change.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "pending E7-4 (DEC-2849 item 8): a resize cancels and re-places on it"
    )
)]
pub(crate) fn leg_excess(
    _view: &ExecutorState,
    _instrument: &InstrumentId,
) -> Result<BTreeMap<AgentId, Qty>, ExecutorError> {
    Err(ExecutorError::Unimplemented { story: "E7-4" })
}

/// DEC-2849 item 8, §5.4 under shared positions: the resting protective orders an exit of `qty` by
/// `agent` cancels before it sells. They are the agent's own legs and every leg no holder owns
/// (unattributed, or `*`). Another holder's legs keep resting while they fit beside the exit: what
/// they have left to sell (an OCO's two legs once, as the larger), every working sell and `qty`
/// together within N. Otherwise they are cancelled too, so a rule-13 exit is never cut by another
/// holder's legs (I4) and the sequence re-places them after it. An exit of no agent (`*`) cancels
/// every leg.
pub(crate) fn exit_cancels(
    view: &ExecutorState,
    instrument: &InstrumentId,
    agent: &AgentId,
    qty: Qty,
) -> Result<BTreeSet<ClientOrderId>, ExecutorError> {
    let legs: Vec<&Order> = view
        .orders
        .values()
        .filter(|order| {
            &order.instrument == instrument
                && order.side == Side::Sell
                && order.purpose == Purpose::Protective
                && !order.state.is_terminal()
                && order.state != OrderState::Intent
        })
        .collect();
    if agent.0 == EVERY_AGENT {
        return Ok(legs
            .iter()
            .map(|order| order.client_order_id.clone())
            .collect());
    }
    let (others, own): (Vec<&Order>, Vec<&Order>) = legs.into_iter().partition(|order| {
        order
            .agent
            .as_ref()
            .is_some_and(|holder| holder != agent && holder.0 != EVERY_AGENT)
    });
    let mut by_order: BTreeMap<&str, Qty> = BTreeMap::new();
    for order in &others {
        let left = order.qty.checked_sub(order.filled_qty)?;
        let counted = by_order.entry(oco_parent(order)).or_insert(Qty::ZERO);
        *counted = (*counted).max(left);
    }
    let kept = by_order
        .values()
        .try_fold(Qty::ZERO, |sum, left| sum.checked_add(*left))?;
    let fits = kept
        .checked_add(still_selling(view, instrument)?)?
        .checked_add(qty)?
        <= long(view, instrument)?;
    Ok(own
        .into_iter()
        .chain(others.into_iter().filter(|_| !fits))
        .map(|order| order.client_order_id.clone())
        .collect())
}

/// The id an OCO's or a bracket's leg counts under: its parent's, without the `-tp` or `-sl`
/// (§2.3), so the pair counts once (I9); any other order's own.
fn oco_parent(order: &Order) -> &str {
    let id = order.client_order_id.as_str();
    id.strip_suffix("-tp")
        .or_else(|| id.strip_suffix("-sl"))
        .unwrap_or(id)
}

/// I4's clip: N less every working sell in `instrument` as [`still_selling`] counts them, never
/// below zero.
pub(crate) fn available_to_sell(
    view: &ExecutorState,
    instrument: &InstrumentId,
) -> Result<Qty, ExecutorError> {
    Ok(long(view, instrument)?
        .checked_sub(still_selling(view, instrument)?)
        .unwrap_or(Qty::ZERO))
}

/// I4 for the exits the executor raises for an agent itself (DEC-2849 item 7): the watchdog exit of
/// its triggered protection, and its plain entry's bound exit. Each is that agent's own, sized
/// min(Lᵢ, N), clipped to what is still available to sell ([`available_to_sell`]), so it never
/// sells another holder's or the owner's shares (the founder's ruling of 2026-10-27). The size
/// never reads the agent's mode.
pub(crate) fn rule_13_size(
    view: &ExecutorState,
    agent: &AgentId,
    instrument: &InstrumentId,
) -> Result<Qty, ExecutorError> {
    Ok(sub_ledger(view, agent, instrument)?.min(available_to_sell(view, instrument)?))
}

/// Takes `quantity` of an account switch's close in `instrument` out of its holders, for
/// [`ExecutorState::closed`] (DEC-2849 I5): out of the owner's positive residual first, then the
/// agents' lots in [`coverages`]' order, largest first and ties by agent id. A fill that takes the
/// whole position takes every agent's lots, so a complete close zeroes them all even while a
/// deficit holds. Read before the close's fill is applied.
pub(crate) fn close_takes(
    view: &ExecutorState,
    instrument: &InstrumentId,
    quantity: Qty,
) -> Result<Vec<(AgentId, Qty)>, ExecutorError> {
    let holders = holders(view, instrument)?;
    if quantity >= holders.position {
        return Ok(holders.lots.into_iter().collect());
    }
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

/// Agent i's lots Lᵢ in `instrument`, from [`holders`].
pub(crate) fn lots(
    view: &ExecutorState,
    agent: &AgentId,
    instrument: &InstrumentId,
) -> Result<Qty, ExecutorError> {
    Ok(lots_of(&holders(view, instrument)?, agent))
}

/// An agent's sell as one of its fills is folded: its lots and what the order had sold before the
/// fill, for [`note_oversold`].
#[derive(Debug)]
pub(crate) struct Selling {
    agent: AgentId,
    instrument: InstrumentId,
    order: ClientOrderId,
    lots: Qty,
    sold: Qty,
}

/// [`Selling`] for a fill of `order`, read before the fill is applied: `None` unless it is a sell
/// one of the agents placed.
pub(crate) fn selling(
    view: &ExecutorState,
    order: &ClientOrderId,
) -> Result<Option<Selling>, ExecutorError> {
    let Some((placed, agent)) = view
        .orders
        .get(order)
        .filter(|placed| placed.side == Side::Sell)
        .and_then(|placed| Some((placed, placed.agent.clone()?)))
    else {
        return Ok(None);
    };
    Ok(Some(Selling {
        lots: lots(view, &agent, &placed.instrument)?,
        sold: sold(view, order, placed),
        instrument: placed.instrument.clone(),
        order: order.clone(),
        agent,
    }))
}

/// Notes in [`ExecutorState::oversold`] what the fill [`selling`] read took beyond the agent's
/// lots, once the fill is applied: the growth of what the order sold, less the lots it had.
pub(crate) fn note_oversold(
    state: &mut ExecutorState,
    selling: Selling,
) -> Result<(), ExecutorError> {
    let now = state
        .orders
        .get(&selling.order)
        .map_or(selling.sold, |placed| sold(state, &selling.order, placed));
    let beyond = now
        .checked_sub(selling.sold)
        .unwrap_or(Qty::ZERO)
        .checked_sub(selling.lots)
        .unwrap_or(Qty::ZERO);
    oversell(state, selling.agent, selling.instrument, beyond)
}

/// Adds `beyond` to `agent`'s [`ExecutorState::oversold`] in `instrument`.
pub(crate) fn oversell(
    state: &mut ExecutorState,
    agent: AgentId,
    instrument: InstrumentId,
    beyond: Qty,
) -> Result<(), ExecutorError> {
    if beyond > Qty::ZERO {
        let total = state
            .oversold
            .entry((agent, instrument))
            .or_insert(Qty::ZERO);
        *total = total.checked_add(beyond)?;
    }
    Ok(())
}

fn lots_of(holders: &Holders, agent: &AgentId) -> Qty {
    holders.lots.get(agent).copied().unwrap_or(Qty::ZERO)
}

/// A filled buy of `qty` by `agent` in `instrument`, so a test fixture's position is that agent's
/// lots (DEC-2849): the position itself is the fixture's to set.
#[cfg(test)]
pub(crate) fn bought(
    state: &mut ExecutorState,
    agent: &str,
    instrument: &InstrumentId,
    qty: &str,
) -> Result<(), ExecutorError> {
    let id = ClientOrderId::parse(&format!("md-bought-{agent}-{instrument}"))?;
    state.orders.insert(
        id.clone(),
        crate::types::Order {
            client_order_id: id,
            intent_id: None,
            agent: Some(AgentId(agent.to_owned())),
            instrument: instrument.clone(),
            side: Side::Buy,
            qty: Qty::parse(qty)?,
            filled_qty: Qty::parse(qty)?,
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use mandate_accounting::{InstrumentId, Side};
    use mandate_canon::Value;
    use mandate_num::Qty;
    use proptest::prelude::*;

    use super::{
        available_to_sell, coverage, coverages, exit_cancels, holders, leg_excess, leg_gaps,
        sell_authority, sub_ledger, uncovered,
    };
    use crate::error::ExecutorError;
    use crate::fold::{fees_charged, fill_applied, position_released};
    use crate::ids::{ClientOrderId, Leg};
    use crate::payload::{object, text};
    use crate::state::{AccountSwitch, EVERY_AGENT, ExecutorState};
    use crate::types::{
        AccountRef, AccountScope, AgentId, EventId, Mode, Order, OrderState, Purpose, RiskClock,
        Seq, WorkspaceId,
    };

    const AGENTS: u8 = 3;
    const INSTRUMENTS: [&str; 2] = ["AAPL", "MSFT"];

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

    fn instrument(index: usize) -> Result<InstrumentId, ExecutorError> {
        Ok(InstrumentId::new(
            INSTRUMENTS.get(index).copied().unwrap_or("AAPL"),
        )?)
    }

    /// One generated event on one of two instruments (DEC-2849's fuzz): an agent's buy or sell
    /// filled, the sell possibly beyond its lots and possibly leaving a remainder working; an
    /// owner's buy or sell outside Mandate; a release from an agent to the owner, possibly beyond
    /// its lots; a fill of the account switch's close; or an asset fee taken from the position.
    #[derive(Debug, Clone)]
    enum Event {
        Buy(u8, usize, i64),
        Sell(u8, usize, i64, i64),
        OwnerBuy(usize, i64),
        OwnerSell(usize, i64),
        Release(u8, usize, i64),
        Close(usize, i64),
        AssetFee(usize, i64),
        /// Agent `a`'s resting protective leg of `q` in instrument `i` (DEC-2849 item 8).
        Protect(u8, usize, i64),
        /// Agent `a`'s OCO for `q` in `i`: its take-profit and stop legs, `…-tp` and `…-sl`, each
        /// for `q`, which count once, as the larger (I9).
        Oco(u8, usize, i64),
        /// A resting protective leg of `q` in `i` that names no agent, or `*` when `.2`: no
        /// holder's, so it counts in no holder's legs (§5.4's unattributed leg).
        Unattributed(usize, i64, bool),
        /// Agent `a`'s working sell of `q` in `i` for [`EXITS`]`[k]`, none of it filled yet.
        Exit(u8, usize, i64, usize),
        /// The `k`-th sell rested in `i` (by [`Event::Protect`], [`Event::Oco`],
        /// [`Event::Unattributed`] or [`Event::Exit`], live or not) is cancelled.
        Cancel(usize, usize),
        /// The `k`-th sell rested in `i` fills `f` more, all of what it has left when `f` is at
        /// least that. Only an agent's own leg or exit fills here; an OCO's leg or a leg of no
        /// agent is only cancelled.
        Fill(usize, usize, i64),
    }

    /// The sells that count as an agent's working exits beside its legs: every sell that is not
    /// protective and adds no risk, the watchdog's and the bound's `RiskExit` among them. An
    /// opening's sell remainder, which adds risk, counts in none.
    const EXITS: [Purpose; 4] = [
        Purpose::RiskExit,
        Purpose::DiscretionaryExit,
        Purpose::OwnerExit,
        Purpose::Flatten,
    ];

    fn event() -> impl Strategy<Value = Event> {
        let at = 0_usize..INSTRUMENTS.len();
        prop_oneof![
            (0_u8..AGENTS, at.clone(), 1_i64..=20).prop_map(|(a, i, q)| Event::Buy(a, i, q)),
            (0_u8..AGENTS, at.clone(), 1_i64..=20, 0_i64..=3)
                .prop_map(|(a, i, q, w)| Event::Sell(a, i, q, w)),
            (at.clone(), 1_i64..=20).prop_map(|(i, q)| Event::OwnerBuy(i, q)),
            (at.clone(), 1_i64..=20).prop_map(|(i, q)| Event::OwnerSell(i, q)),
            (0_u8..AGENTS, at.clone(), 1_i64..=20).prop_map(|(a, i, q)| Event::Release(a, i, q)),
            (at.clone(), 1_i64..=20).prop_map(|(i, q)| Event::Close(i, q)),
            (at, 1_i64..=2).prop_map(|(i, q)| Event::AssetFee(i, q)),
        ]
    }

    /// [`event`]'s events and the legs and exits item 8 reads.
    fn leg_event() -> impl Strategy<Value = Event> {
        let at = 0_usize..INSTRUMENTS.len();
        prop_oneof![
            6 => event(),
            2 => (0_u8..AGENTS, at.clone(), 1_i64..=20).prop_map(|(a, i, q)| Event::Protect(a, i, q)),
            1 => (0_u8..AGENTS, at.clone(), 1_i64..=20).prop_map(|(a, i, q)| Event::Oco(a, i, q)),
            1 => (at.clone(), 1_i64..=20, any::<bool>())
                .prop_map(|(i, q, star)| Event::Unattributed(i, q, star)),
            2 => (0_u8..AGENTS, at.clone(), 1_i64..=10, 0..EXITS.len())
                .prop_map(|(a, i, q, k)| Event::Exit(a, i, q, k)),
            1 => (at.clone(), 0_usize..8).prop_map(|(i, k)| Event::Cancel(i, k)),
            2 => (at, 0_usize..8, 1_i64..=10).prop_map(|(i, k, f)| Event::Fill(i, k, f)),
        ]
    }

    /// The oracle for one instrument: each holder's whole shares, written apart from `holders`.
    /// `lots` per agent; `owner`, the owner's residual, which may go negative; `working`, what the
    /// agents' working sells have left to sell. The position is the lots and the residual summed.
    #[derive(Debug, Default)]
    struct Oracle {
        lots: BTreeMap<u8, i64>,
        owner: i64,
        working: i64,
        /// Every sell rested in the instrument, in the order it rested, live or not.
        resting: Vec<Resting>,
        /// Each agent's own orders' buy fills less their sell fills, as DEC-160's single-holder
        /// rule reads them: releases, closes and fees aside.
        raw: BTreeMap<u8, i64>,
    }

    /// One rested sell as the oracle keeps it, apart from the executor's order.
    #[derive(Debug)]
    struct Resting {
        id: ClientOrderId,
        /// The agent it names; `None` for a leg of no agent or of `*`.
        agent: Option<u8>,
        /// Whether a leg of no agent names `*`, which no later fill attributes.
        star: bool,
        protective: bool,
        /// The OCO whose leg it is, by the OCO's parent id.
        pair: Option<String>,
        qty: i64,
        filled: i64,
        live: bool,
    }

    impl Resting {
        fn new(
            id: ClientOrderId,
            agent: Option<u8>,
            protective: bool,
            pair: Option<String>,
            qty: i64,
        ) -> Self {
            Self {
                id,
                agent,
                protective,
                pair,
                qty,
                filled: 0,
                live: true,
                star: false,
            }
        }

        fn left(&self) -> i64 {
            if self.live {
                self.qty.saturating_sub(self.filled)
            } else {
                0
            }
        }
    }

    impl Oracle {
        fn lots(&self, a: u8) -> i64 {
            self.lots.get(&a).copied().unwrap_or(0)
        }

        fn add(&mut self, a: u8, q: i64) {
            let lots = self.lots.entry(a).or_insert(0);
            *lots = lots.saturating_add(q);
        }

        /// Agent `a` loses up to `q` of its lots, never below zero; it returns what it lost.
        fn take(&mut self, a: u8, q: i64) -> i64 {
            let taken = self.lots(a).min(q);
            self.add(a, taken.saturating_neg());
            taken
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

        /// Agent `a`'s own orders filled `q` more, bought when positive, sold when negative.
        fn traded(&mut self, a: u8, q: i64) {
            let raw = self.raw.entry(a).or_insert(0);
            *raw = raw.saturating_add(q);
        }

        /// DEC-160's rule, retried after a fill as the fold retries it: a leg that names no agent
        /// (not `*`) takes the one agent whose own orders' net fills are the whole position.
        fn reattribute(&mut self) {
            let holding: Vec<(u8, i64)> = self
                .raw
                .iter()
                .filter(|(_, net)| **net != 0)
                .map(|(a, net)| (*a, *net))
                .collect();
            let position = self.position();
            let [(a, net)] = holding.as_slice() else {
                return;
            };
            if *net != position {
                return;
            }
            for rest in &mut self.resting {
                if rest.agent.is_none() && !rest.star {
                    rest.agent = Some(*a);
                }
            }
        }

        /// Each agent's live protective legs, what they have left to sell, an OCO's two legs
        /// counted once, as the larger.
        fn legs(&self) -> BTreeMap<u8, i64> {
            let mut by_order: BTreeMap<(u8, String), i64> = BTreeMap::new();
            for rest in self.resting.iter().filter(|rest| rest.protective) {
                let Some(a) = rest.agent else {
                    continue;
                };
                let key = rest
                    .pair
                    .clone()
                    .unwrap_or_else(|| rest.id.as_str().to_owned());
                let counted = by_order.entry((a, key)).or_insert(0);
                *counted = (*counted).max(rest.left());
            }
            let mut legs = BTreeMap::new();
            for ((a, _), left) in by_order {
                let sum = legs.entry(a).or_insert(0_i64);
                *sum = sum.saturating_add(left);
            }
            legs.retain(|_, left| *left > 0);
            legs
        }

        /// Each agent's live exits, what they have left to sell.
        fn exits(&self) -> BTreeMap<u8, i64> {
            let mut exits = BTreeMap::new();
            for rest in self.resting.iter().filter(|rest| !rest.protective) {
                if let Some(a) = rest.agent {
                    let sum = exits.entry(a).or_insert(0_i64);
                    *sum = sum.saturating_add(rest.left());
                }
            }
            exits
        }

        /// Item 8b's cancels for an exit of `q` by agent `a`, or by no agent for `None`: its own
        /// live legs and every live leg of no holder; and every other holder's live legs as well
        /// when what those have left (an OCO's pair once, as the larger), the working sells and
        /// `q` pass the position. An exit of no agent cancels every live leg.
        fn exit_cancels(&self, a: Option<u8>, q: i64) -> BTreeSet<ClientOrderId> {
            let live: Vec<&Resting> = self
                .resting
                .iter()
                .filter(|rest| rest.protective && rest.left() > 0)
                .collect();
            let Some(a) = a else {
                return live.iter().map(|rest| rest.id.clone()).collect();
            };
            let (others, mine): (Vec<&Resting>, Vec<&Resting>) = live
                .into_iter()
                .partition(|rest| rest.agent.is_some_and(|b| b != a));
            let mut by_order: BTreeMap<String, i64> = BTreeMap::new();
            for rest in &others {
                let key = rest
                    .pair
                    .clone()
                    .unwrap_or_else(|| rest.id.as_str().to_owned());
                let counted = by_order.entry(key).or_insert(0);
                *counted = (*counted).max(rest.left());
            }
            let kept = by_order
                .values()
                .fold(0_i64, |sum, left| sum.saturating_add(*left));
            let fits = kept.saturating_add(self.working).saturating_add(q) <= self.position();
            mine.into_iter()
                .chain(others.into_iter().filter(|_| !fits))
                .map(|rest| rest.id.clone())
                .collect()
        }

        /// Item 8's gaps: each holder's Pᵢ less its own legs and exits, where that is above zero.
        fn gaps(&self) -> BTreeMap<u8, i64> {
            let (legs, exits) = (self.legs(), self.exits());
            self.coverage()
                .into_iter()
                .map(|(a, p)| {
                    let own = legs
                        .get(&a)
                        .copied()
                        .unwrap_or(0)
                        .saturating_add(exits.get(&a).copied().unwrap_or(0));
                    (a, p.saturating_sub(own))
                })
                .filter(|(_, gap)| *gap > 0)
                .collect()
        }

        /// Item 8's excess: each agent's legs beyond its Pᵢ, zero for an agent holding no lots.
        fn excess(&self) -> BTreeMap<u8, i64> {
            let covered = self.coverage();
            self.legs()
                .into_iter()
                .map(|(a, legs)| {
                    (
                        a,
                        legs.saturating_sub(covered.get(&a).copied().unwrap_or(0)),
                    )
                })
                .filter(|(_, over)| *over > 0)
                .collect()
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

        /// A close fill of `q`: the whole position zeroes every holder; a part comes out of the
        /// owner's positive residual first, then the agents in coverage order.
        fn close(&mut self, q: i64) {
            if q >= self.position() {
                self.lots.clear();
                self.owner = 0;
                return;
            }
            let from_owner = q.min(self.owner.max(0));
            let mut left = q.saturating_sub(from_owner);
            for a in self.order() {
                left = left.saturating_sub(self.take(a, left));
            }
            self.owner = self.owner.saturating_sub(from_owner);
        }
    }

    /// The executor's state, driven through the fold's own `fill_applied`, `position_released`
    /// and `fees_charged`, beside one oracle per instrument.
    struct Run {
        view: ExecutorState,
        oracles: BTreeMap<usize, Oracle>,
        seq: u64,
        /// Whether the event being applied folded a fill, after which the fold reattributes.
        folded: bool,
    }

    impl Run {
        fn new() -> Self {
            Self {
                view: state(),
                oracles: BTreeMap::new(),
                seq: 0,
                folded: false,
            }
        }

        fn next(&mut self) -> Seq {
            self.seq = self.seq.saturating_add(1);
            Seq(self.seq)
        }

        /// A fill of `q` `side` in instrument `i`, naming `order` when it has one.
        fn fill(
            &mut self,
            i: usize,
            side: &str,
            q: i64,
            order: Option<&ClientOrderId>,
        ) -> Result<(), ExecutorError> {
            let seq = self.next();
            let mut fields: Vec<(&str, Value)> = vec![
                ("fill_id", text(format!("f-{}", seq.0))),
                (
                    "instrument",
                    text(INSTRUMENTS.get(i).copied().unwrap_or("AAPL")),
                ),
                ("side", text(side)),
                ("qty_gross", text(q.to_string())),
                ("price", text("100")),
            ];
            if let Some(order) = order {
                fields.push(("client_order_id", text(order.as_str())));
            }
            self.folded = true;
            fill_applied(&mut self.view, &object(fields)?, seq)
        }

        /// Places agent `a`'s order for `qty` and fills `filled` of it; a remainder stays working.
        fn order(
            &mut self,
            a: u8,
            i: usize,
            side: Side,
            qty: i64,
            filled: i64,
        ) -> Result<(), ExecutorError> {
            let id = ClientOrderId::parse(&format!("md-{}-{a}", self.seq.saturating_add(1)))?;
            self.view.orders.insert(
                id.clone(),
                Order {
                    client_order_id: id.clone(),
                    intent_id: None,
                    agent: Some(agent(a)),
                    instrument: instrument(i)?,
                    side,
                    qty: shares(qty)?,
                    filled_qty: Qty::ZERO,
                    state: OrderState::Accepted,
                    attempt: 1,
                    purpose: Purpose::Open,
                    absent_lookups: 0,
                    first_absence_at: None,
                    cancel_unconfirmed: false,
                    replaced_by: None,
                    created_on: None,
                },
            );
            let label = if side == Side::Buy { "buy" } else { "sell" };
            self.fill(i, label, filled, Some(&id))?;
            if filled >= qty
                && let Some(order) = self.view.orders.get_mut(&id)
            {
                order.state = OrderState::Filled;
            }
            Ok(())
        }

        /// Places a sell of `qty` for `purpose` naming `owner`, under `id` or else a fresh one,
        /// accepted and none of it filled; it returns the id.
        fn rest(
            &mut self,
            (owner, id): (Option<AgentId>, Option<ClientOrderId>),
            i: usize,
            purpose: Purpose,
            qty: i64,
        ) -> Result<ClientOrderId, ExecutorError> {
            let seq = self.next();
            let id = match id {
                Some(id) => id,
                None => ClientOrderId::parse(&format!("md-r{}", seq.0))?,
            };
            self.view.orders.insert(
                id.clone(),
                Order {
                    client_order_id: id.clone(),
                    intent_id: None,
                    agent: owner,
                    instrument: instrument(i)?,
                    side: Side::Sell,
                    qty: shares(qty)?,
                    filled_qty: Qty::ZERO,
                    state: OrderState::Accepted,
                    attempt: 1,
                    purpose,
                    absent_lookups: 0,
                    first_absence_at: None,
                    cancel_unconfirmed: false,
                    replaced_by: None,
                    created_on: None,
                },
            );
            Ok(id)
        }

        /// Applies `event` to the oracle and to the fold. An event the account could not have (a
        /// sale of more than it holds) is skipped.
        fn apply(&mut self, event: &Event) -> Result<(), ExecutorError> {
            let i = match *event {
                Event::Buy(_, i, _)
                | Event::Sell(_, i, _, _)
                | Event::OwnerBuy(i, _)
                | Event::OwnerSell(i, _)
                | Event::Release(_, i, _)
                | Event::Close(i, _)
                | Event::AssetFee(i, _)
                | Event::Protect(_, i, _)
                | Event::Oco(_, i, _)
                | Event::Unattributed(i, _, _)
                | Event::Exit(_, i, _, _)
                | Event::Cancel(i, _)
                | Event::Fill(i, _, _) => i,
            };
            let mut oracle = self.oracles.remove(&i).unwrap_or_default();
            let held = oracle.position();
            self.folded = false;
            match *event {
                Event::Buy(a, _, q) => {
                    self.order(a, i, Side::Buy, q, q)?;
                    oracle.add(a, q);
                    oracle.traded(a, q);
                }
                Event::Sell(a, _, q, w) if q <= held => {
                    self.order(a, i, Side::Sell, q.saturating_add(w), q)?;
                    oracle.traded(a, q.saturating_neg());
                    let beyond = q.saturating_sub(oracle.take(a, q));
                    oracle.owner = oracle.owner.saturating_sub(beyond);
                    oracle.working = oracle.working.saturating_add(w);
                }
                Event::OwnerBuy(_, q) => {
                    self.fill(i, "buy", q, None)?;
                    oracle.owner = oracle.owner.saturating_add(q);
                }
                Event::OwnerSell(_, q) | Event::AssetFee(_, q) if q <= held => {
                    if matches!(event, Event::OwnerSell(..)) {
                        self.fill(i, "sell", q, None)?;
                    } else {
                        fees_charged(
                            &mut self.view,
                            &object(vec![
                                ("family", text("crypto_asset")),
                                ("day", text("2026-10-27")),
                                (
                                    "instrument",
                                    text(INSTRUMENTS.get(i).copied().unwrap_or("AAPL")),
                                ),
                                ("accrued", text(q.to_string())),
                            ])?,
                        )?;
                    }
                    oracle.owner = oracle.owner.saturating_sub(q);
                }
                Event::Release(a, _, q) => {
                    position_released(
                        &mut self.view,
                        &object(vec![
                            ("agent_id", text(agent(a).0)),
                            (
                                "instrument",
                                text(INSTRUMENTS.get(i).copied().unwrap_or("AAPL")),
                            ),
                            ("qty", text(q.to_string())),
                        ])?,
                    )?;
                    let moved = oracle.take(a, q);
                    oracle.owner = oracle.owner.saturating_add(moved);
                }
                Event::Close(_, q) if q <= held => {
                    let requested = self.next();
                    self.view.account_switch = Some(AccountSwitch {
                        event: EventId("k-1".to_owned()),
                        seq: requested,
                        reference: AccountRef("acct-1".to_owned()),
                        at: RiskClock::from_secs(0),
                        requests: BTreeMap::from([(instrument(i)?, (1, requested))]),
                        bound: BTreeSet::new(),
                    });
                    self.fill(i, "sell", q, None)?;
                    self.view.account_switch = None;
                    oracle.close(q);
                }
                Event::Protect(a, _, q) => {
                    let id = self.rest((Some(agent(a)), None), i, Purpose::Protective, q)?;
                    oracle
                        .resting
                        .push(Resting::new(id, Some(a), true, None, q));
                }
                Event::Oco(a, _, q) => {
                    let parent = ClientOrderId::parse(&format!("md-e{}-p{}", a, self.seq))?;
                    for leg in [Leg::TakeProfit, Leg::Stop] {
                        let id = Some(parent.leg(leg)?);
                        let id = self.rest((Some(agent(a)), id), i, Purpose::Protective, q)?;
                        let pair = Some(parent.as_str().to_owned());
                        oracle
                            .resting
                            .push(Resting::new(id, Some(a), true, pair, q));
                    }
                }
                Event::Unattributed(_, q, star) => {
                    let owner = star.then(|| AgentId(EVERY_AGENT.to_owned()));
                    let id = self.rest((owner, None), i, Purpose::Protective, q)?;
                    let mut leg = Resting::new(id, None, true, None, q);
                    leg.star = star;
                    oracle.resting.push(leg);
                }
                Event::Exit(a, _, q, k) if q <= held => {
                    let purpose = EXITS.get(k).copied().unwrap_or(Purpose::RiskExit);
                    let id = self.rest((Some(agent(a)), None), i, purpose, q)?;
                    oracle
                        .resting
                        .push(Resting::new(id, Some(a), false, None, q));
                    oracle.working = oracle.working.saturating_add(q);
                }
                Event::Cancel(_, k) => {
                    let len = oracle.resting.len();
                    if let Some(rest) = oracle.resting.get_mut(k.checked_rem(len).unwrap_or(0)) {
                        if !rest.protective {
                            oracle.working = oracle.working.saturating_sub(rest.left());
                        }
                        rest.live = false;
                        if let Some(order) = self.view.orders.get_mut(&rest.id) {
                            order.state = OrderState::Canceled;
                        }
                    }
                }
                Event::Fill(_, k, f) => {
                    let len = oracle.resting.len();
                    let at = k.checked_rem(len).unwrap_or(0);
                    let fill = oracle.resting.get(at).and_then(|rest| {
                        let a = rest.agent?;
                        let q = f.min(rest.left());
                        (rest.pair.is_none() && q > 0 && q <= held)
                            .then(|| (a, q, rest.id.clone(), rest.protective))
                    });
                    if let Some((a, q, id, protective)) = fill {
                        self.fill(i, "sell", q, Some(&id))?;
                        oracle.traded(a, q.saturating_neg());
                        let beyond = q.saturating_sub(oracle.take(a, q));
                        oracle.owner = oracle.owner.saturating_sub(beyond);
                        if !protective {
                            oracle.working = oracle.working.saturating_sub(q);
                        }
                        if let Some(rest) = oracle.resting.get_mut(at) {
                            rest.filled = rest.filled.saturating_add(q);
                            rest.live = rest.filled < rest.qty;
                            if let Some(order) = self.view.orders.get_mut(&id) {
                                order.state = if rest.live {
                                    OrderState::PartiallyFilled
                                } else {
                                    OrderState::Filled
                                };
                            }
                        }
                    }
                }
                _ => {}
            }
            if self.folded {
                oracle.reattribute();
            }
            self.oracles.insert(i, oracle);
            Ok(())
        }
    }

    /// DEC-2849 I1, I3, I4, I5 and I9 against each instrument's oracle.
    fn check(run: &Run) -> Result<(), TestCaseError> {
        let fail = |e: ExecutorError| TestCaseError::fail(e.to_string());
        let view = &run.view;
        prop_assert!(
            view.closed
                .values()
                .chain(view.oversold.values())
                .all(|taken| *taken > Qty::ZERO),
            "a close or an oversell notes only what it took"
        );
        for (i, oracle) in &run.oracles {
            let at = instrument(*i).map_err(fail)?;
            let held = holders(view, &at).map_err(fail)?;
            let n = |v: i64| shares(v).map_err(fail);
            let mut want = BTreeMap::new();
            for a in 0..AGENTS {
                if oracle.lots(a) > 0 {
                    want.insert(agent(a), n(oracle.lots(a))?);
                }
            }
            prop_assert_eq!(&held.lots, &want, "lots in {}", i);
            prop_assert_eq!(held.position, n(oracle.position())?, "N in {}", i);
            prop_assert_eq!(held.deficit, n(oracle.deficit())?, "D in {}", i);
            let covered = oracle.coverage();
            let mut sum_s = Qty::ZERO;
            let mut sum_p = Qty::ZERO;
            for a in 0..AGENTS {
                let s = sell_authority(view, &agent(a), &at).map_err(fail)?;
                prop_assert_eq!(
                    s,
                    n(oracle.lots(a).saturating_sub(oracle.deficit()))?,
                    "S of {} in {}",
                    a,
                    i
                );
                let p = coverage(view, &agent(a), &at).map_err(fail)?;
                prop_assert_eq!(p, n(covered.get(&a).copied().unwrap_or(0))?, "P of {}", a);
                let l = sub_ledger(view, &agent(a), &at).map_err(fail)?;
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
            let rest = uncovered(view, &at).map_err(fail)?;
            prop_assert_eq!(
                sum_p.checked_add(rest).map_err(|e| fail(e.into()))?,
                held.position,
                "Σ P + U = N"
            );
            prop_assert_eq!(coverages(view, &at).map_err(fail)?.len(), want.len());
            prop_assert_eq!(
                available_to_sell(view, &at).map_err(fail)?,
                n(oracle.position().saturating_sub(oracle.working))?,
                "N less the working sells in {}",
                i
            );
        }
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// DEC-2849's attribution, folded through `fill_applied`, `position_released` and
        /// `fees_charged`, over random agent fills (sells beyond lots and working remainders
        /// included), owner trades outside Mandate, releases beyond lots, account closes and asset
        /// fees on three agents and two instruments: every holder's lots, the deficit, sell
        /// authority, coverage, sub-ledger and what is available to sell are the oracle's,
        /// Σ Sᵢ ≤ N, Σ Pᵢ = min(N, Σ Lᵢ), and Σ Pᵢ + U = N.
        #[test]
        fn holders_are_the_oracles(events in proptest::collection::vec(event(), 1..60)) {
            let mut run = Run::new();
            for event in &events {
                run.apply(event).map_err(|e| TestCaseError::fail(e.to_string()))?;
                check(&run)?;
            }
        }
    }

    /// DEC-2849 item 7, I4: over the same random events, the size of every agent's own rule-13 exit
    /// (its watchdog's, its bound's) is the oracle's min(Lᵢ, N, N less the working sells): never more
    /// than its own lots, never the owner's or another holder's, and never past what is left.
    #[test]
    fn rule_13_sizes_are_the_oracles() -> Result<(), String> {
        let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(256));
        runner
            .run(&proptest::collection::vec(event(), 1..60), |events| {
                let fail = |e: ExecutorError| TestCaseError::fail(e.to_string());
                let n = |v: i64| shares(v).map_err(fail);
                let mut run = Run::new();
                for event in &events {
                    run.apply(event).map_err(fail)?;
                    for (i, oracle) in &run.oracles {
                        let at = instrument(*i).map_err(fail)?;
                        let left = oracle.position().saturating_sub(oracle.working);
                        for a in 0..AGENTS {
                            let want = n(oracle.lots(a).min(oracle.position()).min(left))?;
                            prop_assert_eq!(
                                super::rule_13_size(&run.view, &agent(a), &at).map_err(fail)?,
                                want,
                                "the rule-13 size of {} in {}",
                                a,
                                i
                            );
                            let mut stopped = run.view.clone();
                            stopped.modes.insert(agent(a), Mode::Stopped);
                            prop_assert_eq!(
                                super::rule_13_size(&stopped, &agent(a), &at).map_err(fail)?,
                                want,
                                "stopped, {}'s exit is sized the same in {}",
                                a,
                                i
                            );
                        }
                    }
                }
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// DEC-2849 item 7, I4, by hand: an agent's own rule-13 exit sells its own lots and no more.
    /// Its 10 beside the owner's 4 is 10, never 14; after the owner sells 4 outside Mandate it is
    /// the 6 left; beside another agent's 4, each is its own; and with 2 sold and 3 still working
    /// from its 10, it is the 5 not yet selling. DEC-2849's worked example, 5 and 5 with the owner
    /// selling 4 outside Mandate, is 5 for each (min(Lᵢ, N), never Sᵢ = 1); and another agent's
    /// working sell of 3 leaves this agent's 10 the 7 not yet selling. A stopped agent is sized the
    /// same: the size never reads the mode. Whether a stopped or killed agent's exit is held, and
    /// how its own flatten clips its watchdog, are the watchdog's and the kill's (E7-4, the
    /// implementation's own tests).
    #[test]
    fn a_rule_13_exit_sells_its_own_lots_and_no_more() -> Result<(), ExecutorError> {
        let at = instrument(0)?;
        for (events, sizes) in [
            (
                vec![Event::Buy(0, 0, 10), Event::OwnerBuy(0, 4)],
                vec![(0, 10)],
            ),
            (
                vec![Event::Buy(0, 0, 10), Event::OwnerSell(0, 4)],
                vec![(0, 6)],
            ),
            (
                vec![Event::Buy(0, 0, 6), Event::Buy(1, 0, 4)],
                vec![(0, 6), (1, 4)],
            ),
            (
                vec![Event::Buy(0, 0, 10), Event::Sell(0, 0, 2, 3)],
                vec![(0, 5)],
            ),
            (
                vec![
                    Event::Buy(0, 0, 5),
                    Event::Buy(1, 0, 5),
                    Event::OwnerSell(0, 4),
                ],
                vec![(0, 5), (1, 5)],
            ),
            (
                vec![
                    Event::Buy(0, 0, 10),
                    Event::Buy(1, 0, 2),
                    Event::Sell(1, 0, 2, 3),
                ],
                vec![(0, 7), (1, 0)],
            ),
        ] {
            let mut run = Run::new();
            for event in &events {
                run.apply(event)?;
            }
            for (a, size) in sizes {
                assert_eq!(
                    super::rule_13_size(&run.view, &agent(a), &at)?,
                    shares(size)?,
                    "{events:?}: agent {a}"
                );
                let mut stopped = run.view.clone();
                stopped.modes.insert(agent(a), Mode::Stopped);
                assert_eq!(
                    super::rule_13_size(&stopped, &agent(a), &at)?,
                    shares(size)?,
                    "{events:?}: agent {a}, stopped"
                );
            }
        }
        Ok(())
    }

    /// DEC-2849's worked example: A and B each hold 5, and the owner sells 4 outside Mandate.
    #[test]
    fn the_worked_example() -> Result<(), ExecutorError> {
        let mut run = Run::new();
        for event in [
            Event::Buy(0, 0, 5),
            Event::Buy(1, 0, 5),
            Event::OwnerSell(0, 4),
        ] {
            run.apply(&event)?;
        }
        let aapl = InstrumentId::new("AAPL")?;
        let view = &run.view;
        assert_eq!(holders(view, &aapl)?.deficit, shares(4)?);
        for (a, s, p) in [(0, 1, 1), (1, 1, 5)] {
            assert_eq!(sell_authority(view, &agent(a), &aapl)?, shares(s)?);
            assert_eq!(coverage(view, &agent(a), &aapl)?, shares(p)?);
            assert_eq!(sub_ledger(view, &agent(a), &aapl)?, shares(5)?);
        }
        run.apply(&Event::Release(0, 0, 4))?;
        assert_eq!(
            holders(&run.view, &aapl)?.deficit,
            Qty::ZERO,
            "a release ends it"
        );
        Ok(())
    }

    /// DEC-2850's interim reading, pinned on its example: the owner holds 5 of their own, agent 0
    /// buys 10 and a crypto asset fee of 1 is taken. The fee names no buy, so it comes out of the
    /// owner's residual and agent 0 keeps 10 lots, where DEC-2849 would give it 9. The journal
    /// admits no crypto asset fee until that is fixed (backlog row E6-15u).
    #[test]
    fn an_asset_fee_comes_out_of_the_owners_residual() -> Result<(), ExecutorError> {
        let mut run = Run::new();
        for event in [
            Event::OwnerBuy(0, 5),
            Event::Buy(0, 0, 10),
            Event::AssetFee(0, 1),
        ] {
            run.apply(&event)?;
        }
        let aapl = InstrumentId::new("AAPL")?;
        let held = holders(&run.view, &aapl)?;
        assert_eq!(held.lots, BTreeMap::from([(agent(0), shares(10)?)]));
        assert_eq!(held.position, shares(14)?);
        assert_eq!(held.deficit, Qty::ZERO);
        assert_eq!(sell_authority(&run.view, &agent(0), &aapl)?, shares(10)?);
        Ok(())
    }

    /// A sell beyond the agent's lots leaves them at zero and takes the excess from the owner, and
    /// a later buy is the agent's whole: the excess never comes out of it.
    #[test]
    fn an_oversell_never_takes_a_later_buy() -> Result<(), ExecutorError> {
        let mut run = Run::new();
        for event in [
            Event::OwnerBuy(0, 10),
            Event::Buy(0, 0, 2),
            Event::Sell(0, 0, 5, 0),
            Event::Buy(0, 0, 4),
        ] {
            run.apply(&event)?;
        }
        let aapl = InstrumentId::new("AAPL")?;
        let held = holders(&run.view, &aapl)?;
        assert_eq!(held.lots, BTreeMap::from([(agent(0), shares(4)?)]));
        assert_eq!(held.position, shares(11)?);
        Ok(())
    }

    /// The legs each holder must still be given and the legs a resize must cut, as the oracle
    /// computes them: item 8's gap and excess for every agent in every instrument.
    fn legs_are_the_oracles(run: &Run) -> Result<(), TestCaseError> {
        let fail = |e: ExecutorError| TestCaseError::fail(e.to_string());
        for (i, oracle) in &run.oracles {
            let at = instrument(*i).map_err(fail)?;
            let want = |by: BTreeMap<u8, i64>| -> Result<BTreeMap<AgentId, Qty>, TestCaseError> {
                by.into_iter()
                    .map(|(a, q)| Ok((agent(a), shares(q).map_err(fail)?)))
                    .collect()
            };
            prop_assert_eq!(
                leg_gaps(&run.view, &at).map_err(fail)?,
                want(oracle.gaps())?,
                "the gaps in {}",
                i
            );
            prop_assert_eq!(
                leg_excess(&run.view, &at).map_err(fail)?,
                want(oracle.excess())?,
                "the excess in {}",
                i
            );
        }
        Ok(())
    }

    /// DEC-2849 item 8 (I9), over the attribution fuzz's events with each agent's resting legs and
    /// working exits: every holder's gap is its Pᵢ less its own legs and exits, never the owner's
    /// residual and never another holder's legs, and every agent's excess is its legs beyond its
    /// Pᵢ, an agent whose lots are gone included.
    #[test]
    #[ignore = "pending E7-4"]
    fn holder_legs_are_the_oracles() -> Result<(), String> {
        let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(256));
        runner
            .run(&proptest::collection::vec(leg_event(), 1..60), |events| {
                let mut run = Run::new();
                for event in &events {
                    run.apply(event)
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;
                    legs_are_the_oracles(&run)?;
                }
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// One hand case: its events, then each holder's gap and each agent's excess it leaves.
    type LegCase = (Vec<Event>, Vec<(u8, i64)>, Vec<(u8, i64)>);

    /// DEC-2849 item 8 by hand. An agent's ten beside the owner's six needs legs for its ten, not
    /// sixteen. Two holders' ten and five each need their own, and one's legs never fill the
    /// other's gap. In the worked example (each holds five, the owner sold four), five-share legs
    /// each leave agent 0 four over its Pᵢ of one. A released agent's legs are all excess.
    ///
    /// R3 on #2879: a cancelled leg covers nothing, and a filled one only what it has left, its
    /// fill having sold the agent's lots; a part-filled exit counts its remainder. A leg of no
    /// agent, or of `*`, is no holder's: it fills no gap and is no agent's excess. An OCO's two
    /// legs count once, and one of them cancelled leaves the other covering. A discretionary exit,
    /// an owner exit and a flatten count as working exits beside a risk exit.
    #[test]
    #[ignore = "pending E7-4"]
    fn each_holders_legs_cover_its_own_coverage() -> Result<(), ExecutorError> {
        let at = instrument(0)?;
        let cases: Vec<LegCase> = vec![
            (
                vec![Event::Buy(0, 0, 10), Event::OwnerBuy(0, 6)],
                vec![(0, 10)],
                vec![],
            ),
            (
                vec![
                    Event::Buy(0, 0, 10),
                    Event::Buy(1, 0, 5),
                    Event::Protect(0, 0, 10),
                ],
                vec![(1, 5)],
                vec![],
            ),
            (
                vec![
                    Event::Buy(0, 0, 10),
                    Event::Buy(1, 0, 5),
                    Event::Protect(0, 0, 4),
                    Event::Exit(0, 0, 3, 0),
                ],
                vec![(0, 3), (1, 5)],
                vec![],
            ),
            (
                vec![
                    Event::Buy(0, 0, 5),
                    Event::Buy(1, 0, 5),
                    Event::Protect(0, 0, 5),
                    Event::Protect(1, 0, 5),
                    Event::OwnerSell(0, 4),
                ],
                vec![],
                vec![(0, 4)],
            ),
            (
                vec![
                    Event::Buy(0, 0, 10),
                    Event::Protect(0, 0, 10),
                    Event::Release(0, 0, 10),
                ],
                vec![],
                vec![(0, 10)],
            ),
            (
                vec![
                    Event::Buy(0, 0, 10),
                    Event::Protect(0, 0, 10),
                    Event::Cancel(0, 0),
                ],
                vec![(0, 10)],
                vec![],
            ),
            (
                vec![
                    Event::Buy(0, 0, 10),
                    Event::Protect(0, 0, 4),
                    Event::Fill(0, 0, 4),
                ],
                vec![(0, 6)],
                vec![],
            ),
            (
                vec![
                    Event::Buy(0, 0, 10),
                    Event::Protect(0, 0, 10),
                    Event::Fill(0, 0, 3),
                ],
                vec![],
                vec![],
            ),
            (
                vec![
                    Event::Buy(0, 0, 10),
                    Event::Exit(0, 0, 6, 0),
                    Event::Fill(0, 0, 2),
                ],
                vec![(0, 4)],
                vec![],
            ),
            (
                vec![Event::Buy(0, 0, 10), Event::Unattributed(0, 10, false)],
                vec![(0, 10)],
                vec![],
            ),
            (
                vec![Event::Buy(0, 0, 10), Event::Unattributed(0, 4, true)],
                vec![(0, 10)],
                vec![],
            ),
            (
                vec![
                    Event::Buy(0, 0, 4),
                    Event::Unattributed(0, 10, false),
                    Event::Unattributed(0, 10, true),
                    Event::Protect(0, 0, 6),
                ],
                vec![],
                vec![(0, 2)],
            ),
            (
                vec![Event::Buy(0, 0, 10), Event::Oco(0, 0, 10)],
                vec![],
                vec![],
            ),
            (
                vec![
                    Event::Buy(0, 0, 10),
                    Event::Oco(0, 0, 10),
                    Event::Cancel(0, 1),
                ],
                vec![],
                vec![],
            ),
            (
                vec![Event::Buy(0, 0, 10), Event::Oco(0, 0, 12)],
                vec![],
                vec![(0, 2)],
            ),
            (
                vec![
                    Event::Buy(0, 0, 10),
                    Event::Exit(0, 0, 3, 1),
                    Event::Exit(0, 0, 2, 2),
                    Event::Exit(0, 0, 1, 3),
                ],
                vec![(0, 4)],
                vec![],
            ),
        ];
        for (events, gaps, excess) in cases {
            let mut run = Run::new();
            for event in &events {
                run.apply(event)?;
            }
            let by = |pairs: Vec<(u8, i64)>| -> Result<BTreeMap<AgentId, Qty>, ExecutorError> {
                pairs
                    .into_iter()
                    .map(|(a, q)| Ok((agent(a), shares(q)?)))
                    .collect()
            };
            assert_eq!(leg_gaps(&run.view, &at)?, by(gaps)?, "{events:?}: gaps");
            assert_eq!(
                leg_excess(&run.view, &at)?,
                by(excess)?,
                "{events:?}: excess"
            );
        }
        Ok(())
    }
    /// The cancels each agent's exit of min(Lᵢ, N), and an exit of no agent of N, would make, as
    /// the oracle computes them (DEC-2849 item 8b).
    fn exit_cancels_are_the_oracles(run: &Run) -> Result<(), TestCaseError> {
        let fail = |e: ExecutorError| TestCaseError::fail(e.to_string());
        for (i, oracle) in &run.oracles {
            let at = instrument(*i).map_err(fail)?;
            let position = oracle.position();
            let mut exits: Vec<(AgentId, Option<u8>, i64)> =
                vec![(AgentId(EVERY_AGENT.to_owned()), None, position)];
            for a in 0..AGENTS {
                if oracle.lots(a) > 0 {
                    exits.push((agent(a), Some(a), oracle.lots(a).min(position)));
                }
            }
            for (who, a, q) in exits {
                let qty = shares(q).map_err(fail)?;
                prop_assert_eq!(
                    exit_cancels(&run.view, &at, &who, qty).map_err(fail)?,
                    oracle.exit_cancels(a, q),
                    "{}'s exit of {} in {}",
                    who.0,
                    q,
                    i
                );
            }
        }
        Ok(())
    }

    /// DEC-2849 item 8b (I4, I9), over the attribution fuzz's events with legs, OCOs, legs of no
    /// holder, exits, cancels and fills: an exit cancels its own agent's legs and every leg no
    /// holder owns, keeps another holder's legs while they fit beside it within N, and cancels
    /// them too once they would not, so another holder's legs never cut it.
    #[test]
    fn exit_cancels_are_the_oracles_over_random_legs() -> Result<(), String> {
        let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(256));
        runner
            .run(&proptest::collection::vec(leg_event(), 1..60), |events| {
                let mut run = Run::new();
                for event in &events {
                    run.apply(event)
                        .map_err(|e| TestCaseError::fail(e.to_string()))?;
                    exit_cancels_are_the_oracles(&run)?;
                }
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// One hand case for item 8b: its events, the exiting agent (`None` for no agent) and its
    /// quantity, and the legs it cancels, by the order they rested in.
    type ExitCase = (Vec<Event>, Option<u8>, i64, Vec<usize>);

    /// DEC-2849 item 8b by hand. With agent 0's ten and agent 1's five each protected, agent 0's
    /// exit of ten cancels only its own leg; a `*` leg beside them goes too. Once the owner has
    /// sold three outside Mandate, agent 1's five no longer fit beside the ten within N = 12, so
    /// they go as well; so do they beside agent 1's own working exit of two. An exit of no agent
    /// cancels every leg. An OCO's two legs count once in what agent 1 keeps.
    #[test]
    fn an_exit_cancels_its_own_legs_and_keeps_anothers_that_fit() -> Result<(), ExecutorError> {
        let at = instrument(0)?;
        let two = || {
            vec![
                Event::Buy(0, 0, 10),
                Event::Buy(1, 0, 5),
                Event::Protect(0, 0, 10),
                Event::Protect(1, 0, 5),
            ]
        };
        let with = |more: Vec<Event>| {
            let mut events = two();
            events.extend(more);
            events
        };
        let cases: Vec<ExitCase> = vec![
            (two(), Some(0), 10, vec![0]),
            (
                with(vec![Event::Unattributed(0, 3, true)]),
                Some(0),
                10,
                vec![0, 2],
            ),
            (with(vec![Event::OwnerSell(0, 3)]), Some(0), 10, vec![0, 1]),
            (with(vec![Event::Exit(1, 0, 2, 0)]), Some(0), 10, vec![0, 1]),
            (two(), None, 15, vec![0, 1]),
            (
                vec![
                    Event::Buy(0, 0, 10),
                    Event::Buy(1, 0, 5),
                    Event::Protect(0, 0, 10),
                    Event::Oco(1, 0, 5),
                ],
                Some(0),
                10,
                vec![0],
            ),
        ];
        for (events, exiting, qty, cancelled) in cases {
            let mut run = Run::new();
            for event in &events {
                run.apply(event)?;
            }
            let rested: Vec<ClientOrderId> = run
                .oracles
                .get(&0)
                .map(|oracle| oracle.resting.iter().map(|rest| rest.id.clone()).collect())
                .unwrap_or_default();
            let want: BTreeSet<ClientOrderId> = cancelled
                .into_iter()
                .filter_map(|k| rested.get(k).cloned())
                .collect();
            let who = exiting.map_or_else(|| AgentId(EVERY_AGENT.to_owned()), agent);
            assert_eq!(
                exit_cancels(&run.view, &at, &who, shares(qty)?)?,
                want,
                "{events:?}: {exiting:?}'s exit of {qty}"
            );
        }
        Ok(())
    }
}
