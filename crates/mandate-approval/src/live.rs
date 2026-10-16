//! DEC-923's one live admission of `cli_confirm` (DEC-529 item 3, DEC-927): until E9-4's step-up
//! authentication lands, a grant on a `live` stream counts with `cli_confirm` evidence only when
//! the deployment is a Robinhood connection, its confirmed mandate is within DEC-529's limits, and
//! the approver is the workspace owner. Every other live deployment keeps refusing the method
//! (DEC-155 item 4).
//!
//! It compares values and decides no limit: the scope's `max_order_usd` and pinned instruments are
//! the confirmed mandate's own, which the gate and the builder still enforce. This only narrows
//! when the method counts. An exit is never asked ([`crate::AskablePurpose`]), so nothing here can
//! hold risk reduction (`AGENTS.md` rule 13).

use std::collections::BTreeSet;

use mandate_num::Usd;

use crate::ApprovalError;
use crate::admit::OpaqueUser;
use crate::content::AssetClass;

/// DEC-529 item 2's `max_order_usd`, in USD: the most a live `cli_confirm` grant may release, and
/// the most the confirmed mandate may allow per order.
pub const LIVE_CLI_CONFIRM_MAX_ORDER_USD: &str = "100";

/// The broker of the deployment's connection, as far as DEC-923 distinguishes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LiveBroker {
    Robinhood,
    /// Any other broker or venue: never admitted.
    Other,
}

/// What a trusted caller knows of a live deployment, from its connection record and its confirmed
/// mandate version. A caller that cannot read all of it passes no scope, which refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveScope {
    pub broker: LiveBroker,
    /// The confirmed mandate's `risk.max_order_usd`.
    pub max_order_usd: Usd,
    /// The confirmed mandate's `universe.pinned_instruments[].asset_id`.
    pub pinned_instruments: BTreeSet<String>,
    /// The workspace owner: the one user whose grant DEC-923 admits.
    pub workspace_owner: OpaqueUser,
}

/// The grant being judged: the bound order and who approved it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveGrant<'a> {
    pub instrument: &'a str,
    pub asset_class: AssetClass,
    /// The bound order's value, quantity times limit.
    pub order_usd: Usd,
    pub approver: &'a OpaqueUser,
}

/// Why DEC-923 does not admit a live `cli_confirm` grant, in the order they are checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LiveRefusal {
    /// No trusted scope: the caller could not read the connection record or the mandate.
    NoScope,
    NotRobinhood,
    /// The confirmed mandate's `max_order_usd` is above DEC-529's 100 USD.
    MandateCapAboveLimit,
    /// The confirmed mandate pins no instrument or more than one.
    NotOneInstrument,
    /// The bound order is not on the mandate's one pinned instrument.
    OtherInstrument,
    /// The bound order is not a US equity (DEC-529 item 13: no crypto).
    NotEquity,
    /// The bound order's value is above DEC-529's 100 USD.
    OrderAboveLimit,
    /// The approver is not the workspace owner.
    NotOwner,
}

impl LiveRefusal {
    /// The code the CLI prints for a local refusal.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::NoScope => "live_scope_unknown",
            Self::NotRobinhood => "live_not_robinhood",
            Self::MandateCapAboveLimit => "live_mandate_cap_above_limit",
            Self::NotOneInstrument => "live_not_one_instrument",
            Self::OtherInstrument => "live_other_instrument",
            Self::NotEquity => "live_not_equity",
            Self::OrderAboveLimit => "live_order_above_limit",
            Self::NotOwner => "live_not_owner",
        }
    }
}

/// DEC-923's verdict on one live `cli_confirm` grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveAdmission {
    Admitted,
    Refused(LiveRefusal),
}

/// Judges `grant` against `scope`; the first failing condition, in [`LiveRefusal`]'s order, names
/// the refusal.
///
/// # Errors
/// None in practice. The `Result` is the stub API's shape.
pub fn live_cli_confirm(
    scope: Option<&LiveScope>,
    grant: &LiveGrant<'_>,
) -> Result<LiveAdmission, ApprovalError> {
    let refused = |why| Ok(LiveAdmission::Refused(why));
    let Some(scope) = scope else {
        return refused(LiveRefusal::NoScope);
    };
    let Ok(cap) = Usd::parse(LIVE_CLI_CONFIRM_MAX_ORDER_USD) else {
        return refused(LiveRefusal::NoScope);
    };
    if scope.broker != LiveBroker::Robinhood {
        return refused(LiveRefusal::NotRobinhood);
    }
    if scope.max_order_usd > cap {
        return refused(LiveRefusal::MandateCapAboveLimit);
    }
    let mut pinned = scope.pinned_instruments.iter();
    let (Some(only), None) = (pinned.next(), pinned.next()) else {
        return refused(LiveRefusal::NotOneInstrument);
    };
    if only != grant.instrument {
        return refused(LiveRefusal::OtherInstrument);
    }
    if grant.asset_class != AssetClass::UsEquity {
        return refused(LiveRefusal::NotEquity);
    }
    if grant.order_usd > cap {
        return refused(LiveRefusal::OrderAboveLimit);
    }
    if *grant.approver != scope.workspace_owner {
        return refused(LiveRefusal::NotOwner);
    }
    Ok(LiveAdmission::Admitted)
}
