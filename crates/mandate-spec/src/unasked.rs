//! The unasked dollars ([mandate spec §4.2](../../../docs/specs/mandate.md#42-warnings-and-the-confirmation-screen);
//! DEC-189, DEC-695): an upper bound on the order value of openings and increases the agent could
//! have decided `auto` (§6.2) from the risk clock to the end of its risk day. It is display only:
//! it changes no decision, and nothing in the gate or the autonomy decision reads it.
//!
//! The same function as `unasked_usd` in `reference/mandate/ref.py`, which the tests' figures come
//! from (E10-7 slice S1a).

use std::collections::BTreeMap;

use mandate_domain::AutonomyDecision;
use mandate_num::{Qty, Rounding, Usd, UsdExact};
use mandate_time::UtcNanos;

use crate::condition::{Condition, ConditionField, ConditionValue, Operator};
use crate::document::{Autonomy, Delegation, DelegationId, Lifts};
use crate::risk::{RiskDay, risk_day};
use crate::validate::ValidatedMandate;
use crate::{Pointer, SchemaDec, SpecError};

/// One delegation's journaled usage (§6.5 condition 4): the orders it lifted and their total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelegationUsage {
    pub orders: u32,
    pub total_usd: Usd,
}

/// §4.3's effective policy as the figure reads it: is `auto` allowed, is the version nonconforming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectivePolicy {
    pub auto_allowed: bool,
    pub nonconforming: bool,
}

/// The figure's inputs besides the mandate (§4.2's table); `None` is an input that is not known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnaskedInputs {
    /// The journaled risk clock *t* (§5.2); for an agent not yet deployed, the validation instant.
    pub now: Option<UtcNanos>,
    /// §5.3's openings and increases submitted in the risk day; 0 for an agent not yet deployed.
    pub orders_today: Option<u32>,
    /// Usage per delegation. A delegation with no entry has used nothing.
    pub usage: Option<BTreeMap<DelegationId, DelegationUsage>>,
    /// Not known reads as allowing `auto` and conforming, which gives the larger figure.
    pub policy: Option<EffectivePolicy>,
}

/// §4.2's unasked dollars, rounded up to the cent, with exact decimals throughout.
///
/// `Ok(None)` is "not known": the risk clock, today's order count, or delegation usage is missing.
/// It is never shown as 0 (`AGENTS.md` rule 3). The figure is a known 0 when the review date has
/// passed at *t* (§6.2 step 5b), the version is `policy_nonconforming`, or `auto` is not allowed
/// (step 5c). Otherwise it is the sum of the largest slices that the orders left today can take:
/// one unlimited slice for the `auto` default or `auto` rules, and each delegation's slices from its
/// remaining caps. There is no gross-exposure cap, because headroom can grow within the day
/// (DEC-695 item 4).
///
/// Every amount is a [`UsdExact`], because the schema lets a decimal carry 28 integer and 28
/// fractional digits, which a [`Usd`] cannot hold or multiply; the one narrowing is the final
/// rounding up to the cent. An order count stays a `u32`, as `max_orders_per_day` is at most
/// 10,000 and a delegation's `max_orders` at most 1,000.
pub fn unasked_usd(
    mandate: &ValidatedMandate,
    inputs: &UnaskedInputs,
) -> Result<Option<Usd>, SpecError> {
    let (Some(now), Some(orders_today), Some(usage)) =
        (inputs.now, inputs.orders_today, inputs.usage.as_ref())
    else {
        return Ok(None);
    };
    let mandate = mandate.mandate();
    let (autonomy, risk) = (&mandate.autonomy, &mandate.risk);
    let day = risk_day(now)?;
    let review_passed = autonomy.review_by.is_some_and(|by| day.day > by);
    let policy_forbids = inputs
        .policy
        .is_some_and(|policy| policy.nonconforming || !policy.auto_allowed);
    if review_passed || policy_forbids {
        return Ok(Some(Usd::ZERO));
    }
    let per_order = exact(&risk.max_order_usd, "/risk/max_order_usd")?;
    let mut slices = Vec::new();
    let mut unlimited = (autonomy.default == AutonomyDecision::Auto).then_some(per_order);
    for rule in autonomy
        .rules
        .iter()
        .filter(|rule| rule.then == AutonomyDecision::Auto)
    {
        let cap = capped(per_order, &[order_bound(&rule.when)?])?;
        unlimited = Some(match unlimited {
            Some(held) => larger(held, cap)?,
            None => cap,
        });
    }
    if let Some(size) = unlimited
        && size.is_positive()?
    {
        insert_by_size(&mut slices, Slice { size, count: None })?;
    }
    for delegation in &autonomy.delegations {
        for slice in delegation_slices(autonomy, per_order, delegation, now, &day, usage)? {
            insert_by_size(&mut slices, slice)?;
        }
    }
    let mut left = risk.max_orders_per_day.saturating_sub(orders_today);
    let mut total = UsdExact::zero();
    for slice in slices {
        let take = slice.count.map_or(left, |count| count.min(left));
        total = total.checked_add(slice.size.checked_mul(whole(take)?)?)?;
        left = left.saturating_sub(take);
    }
    Ok(Some(total.round(CENT_PLACES, Rounding::Ceiling)?))
}

/// The figure is rounded up to whole cents (§4.2 item 6).
const CENT_PLACES: u32 = 2;

/// `count` orders of `size` each; `None` is the unlimited slice, taken as often as orders are left.
struct Slice {
    size: UsdExact,
    count: Option<u32>,
}

/// One delegation's slices (§4.2 item 4): none unless its window meets the rest of the risk day,
/// else q = min(k, ⌊R ÷ c⌋) of size c and, when q < k, one of the remainder R − q × c.
///
/// The window is compared in whole seconds, as the reference model reads every instant, and the
/// risk clock ticks in whole seconds (§5.2). A size of 0 or less gives no slice. No orders left, or
/// no total left (R ≤ 0), needs no test of its own: q is then 0 and the remainder is not above 0, so
/// the one slice it gives has a count of 0.
fn delegation_slices(
    autonomy: &Autonomy,
    per_order: UsdExact,
    delegation: &Delegation,
    now: UtcNanos,
    day: &RiskDay,
    usage: &BTreeMap<DelegationId, DelegationUsage>,
) -> Result<Vec<Slice>, SpecError> {
    let (Some(starts_at), Some(expires_at)) = (delegation.starts_at, delegation.expires_at) else {
        return Err(SpecError::InvalidInput {
            what: "a delegation window V-041 refuses",
        });
    };
    if starts_at.secs() >= day.ends_at.secs() || now.secs() >= expires_at.secs() {
        return Ok(Vec::new());
    }
    let lifted = autonomy.rules.iter().find(|rule| match &delegation.lifts {
        Lifts::Rule(id) => rule.id == *id,
        Lifts::Default => false,
    });
    let lifted_bound = match lifted {
        Some(rule) => order_bound(&rule.when)?,
        None => None,
    };
    let size = capped(
        per_order,
        &[
            Some(exact(
                &delegation.max_order_usd,
                "/autonomy/delegations/max_order_usd",
            )?),
            order_bound(&delegation.when)?,
            lifted_bound,
        ],
    )?;
    let used = usage
        .get(&delegation.id)
        .copied()
        .unwrap_or(DelegationUsage {
            orders: 0,
            total_usd: Usd::ZERO,
        });
    let orders_left = delegation.max_orders.saturating_sub(used.orders);
    let max_total = exact(
        &delegation.max_total_usd,
        "/autonomy/delegations/max_total_usd",
    )?;
    let rest = max_total.checked_sub(UsdExact::of(used.total_usd))?;
    if !size.is_positive()? {
        return Ok(Vec::new());
    }
    let full = whole_slices(rest, size, orders_left)?;
    let remainder = rest.checked_sub(size.checked_mul(whole(full)?)?)?;
    let mut slices = vec![Slice {
        size,
        count: Some(full),
    }];
    if full < orders_left && remainder.is_positive()? {
        slices.push(Slice {
            size: remainder,
            count: Some(1),
        });
    }
    Ok(slices)
}

/// min(`orders_left`, ⌊max(0, `rest`) ÷ `size`⌋) for a positive `size`. The quotient is taken only
/// when `rest` is below `orders_left` slices, so it always fits a `u32`. Truncation is the floor for
/// a positive `rest`, and a quotient of 0 or below truncates to 0, which is the `max(0, …)`.
fn whole_slices(rest: UsdExact, size: UsdExact, orders_left: u32) -> Result<u32, SpecError> {
    if !rest.is_below(size.checked_mul(whole(orders_left)?)?)? {
        return Ok(orders_left);
    }
    let quotient = rest.truncated_quotient(size, Qty::parse("1")?)?;
    quotient
        .to_string()
        .parse()
        .map_err(|_| SpecError::InvalidInput {
            what: "a slice count past u32",
        })
}

/// Inserts `slice` before the first slice smaller than it, so the list stays largest first.
fn insert_by_size(slices: &mut Vec<Slice>, slice: Slice) -> Result<(), SpecError> {
    let mut at = slices.len();
    for (index, held) in slices.iter().enumerate() {
        if held.size.is_below(slice.size)? {
            at = index;
            break;
        }
    }
    slices.insert(at, slice);
    Ok(())
}

/// b(c) of §4.2 item 3: the largest `order_usd` an order matching `condition` can have, or `None`
/// when it bounds nothing. `all` takes its tightest member bound, `any` its loosest and only when
/// every member has one, and `not` or any other comparison gives none.
fn order_bound(condition: &Condition) -> Result<Option<UsdExact>, SpecError> {
    match condition {
        Condition::All(members) => {
            let mut tightest = None;
            for member in members {
                if let Some(bound) = order_bound(member)? {
                    tightest = Some(match tightest {
                        Some(held) => bound.min(held)?,
                        None => bound,
                    });
                }
            }
            Ok(tightest)
        }
        Condition::Any(members) => {
            let mut loosest = None;
            for member in members {
                let Some(bound) = order_bound(member)? else {
                    return Ok(None);
                };
                loosest = Some(match loosest {
                    Some(held) => larger(held, bound)?,
                    None => bound,
                });
            }
            Ok(loosest)
        }
        Condition::Compare {
            field: ConditionField::OrderUsd,
            op: Operator::Lt | Operator::Lte | Operator::Eq,
            value: ConditionValue::Decimal(value),
        } => Ok(Some(exact(value, "/autonomy/rules/when/value")?)),
        Condition::Not(_) | Condition::Compare { .. } => Ok(None),
    }
}

/// `per_order` narrowed by every bound that is present; an absent bound is left out (§4.2 item 4).
fn capped(per_order: UsdExact, bounds: &[Option<UsdExact>]) -> Result<UsdExact, SpecError> {
    let mut cap = per_order;
    for bound in bounds.iter().flatten() {
        cap = cap.min(*bound)?;
    }
    Ok(cap)
}

/// The greater of two amounts.
fn larger(a: UsdExact, b: UsdExact) -> Result<UsdExact, SpecError> {
    Ok(if a.is_below(b)? { b } else { a })
}

/// An order count as an exact amount, to multiply a slice by.
fn whole(count: u32) -> Result<UsdExact, SpecError> {
    Ok(UsdExact::parse(&count.to_string())?)
}

/// A document decimal as an exact amount, or [`SpecError::OutOfRange`] naming its field.
fn exact(value: &SchemaDec, path: &str) -> Result<UsdExact, SpecError> {
    UsdExact::parse(value.as_str()).map_err(|cause| SpecError::OutOfRange {
        path: Pointer::new(path),
        cause,
    })
}
