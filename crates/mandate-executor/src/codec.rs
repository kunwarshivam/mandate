//! The journal's names for this crate's enums, in one place so the fold and the step can never
//! spell one differently.

use mandate_accounting::Side;

use crate::error::ExecutorError;
use crate::types::{Mode, OrderState, OrderType, Purpose, TimeInForce};

fn unknown(field: &str) -> ExecutorError {
    ExecutorError::NonCanonicalPayload {
        field: field.to_owned(),
    }
}

pub(crate) fn state_name(state: OrderState) -> &'static str {
    match state {
        OrderState::Intent => "intent",
        OrderState::Submitting => "submitting",
        OrderState::Accepted => "accepted",
        OrderState::PartiallyFilled => "partially_filled",
        OrderState::PendingCancel => "pending_cancel",
        OrderState::PendingReplace => "pending_replace",
        OrderState::Unknown => "unknown",
        OrderState::Filled => "filled",
        OrderState::Canceled => "canceled",
        OrderState::Rejected => "rejected",
        OrderState::Expired => "expired",
        OrderState::Replaced => "replaced",
        OrderState::Abandoned => "abandoned",
    }
}

pub(crate) fn state_of(name: &str) -> Result<OrderState, ExecutorError> {
    Ok(match name {
        "intent" => OrderState::Intent,
        "submitting" => OrderState::Submitting,
        "accepted" => OrderState::Accepted,
        "partially_filled" => OrderState::PartiallyFilled,
        "pending_cancel" => OrderState::PendingCancel,
        "pending_replace" => OrderState::PendingReplace,
        "unknown" => OrderState::Unknown,
        "filled" => OrderState::Filled,
        "canceled" => OrderState::Canceled,
        "rejected" => OrderState::Rejected,
        "expired" => OrderState::Expired,
        "replaced" => OrderState::Replaced,
        "abandoned" => OrderState::Abandoned,
        _ => return Err(unknown("state")),
    })
}

pub(crate) fn purpose_name(purpose: Purpose) -> &'static str {
    match purpose {
        Purpose::Open => "open",
        Purpose::Increase => "increase",
        Purpose::RiskExit => "risk_exit",
        Purpose::OwnerExit => "owner_exit",
        Purpose::DiscretionaryExit => "discretionary_exit",
        Purpose::Protective => "protective",
        Purpose::Flatten => "flatten",
    }
}

pub(crate) fn purpose_of(name: &str) -> Result<Purpose, ExecutorError> {
    Ok(match name {
        "open" => Purpose::Open,
        "increase" => Purpose::Increase,
        "risk_exit" => Purpose::RiskExit,
        "owner_exit" => Purpose::OwnerExit,
        "discretionary_exit" => Purpose::DiscretionaryExit,
        "protective" => Purpose::Protective,
        "flatten" => Purpose::Flatten,
        _ => return Err(unknown("purpose")),
    })
}

pub(crate) fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Normal => "normal",
        Mode::ExitsOnly => "exits_only",
        Mode::Paused => "paused",
        Mode::Stopped => "stopped",
    }
}

pub(crate) fn mode_of(name: &str) -> Result<Mode, ExecutorError> {
    Ok(match name {
        "normal" => Mode::Normal,
        "exits_only" => Mode::ExitsOnly,
        "paused" => Mode::Paused,
        "stopped" => Mode::Stopped,
        _ => return Err(unknown("to")),
    })
}

pub(crate) fn side_name(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

pub(crate) fn side_of(name: &str) -> Result<Side, ExecutorError> {
    match name {
        "buy" => Ok(Side::Buy),
        "sell" => Ok(Side::Sell),
        _ => Err(unknown("side")),
    }
}

pub(crate) fn order_type_name(order_type: OrderType) -> &'static str {
    match order_type {
        OrderType::Limit => "limit",
        OrderType::Market => "market",
        OrderType::StopLimit => "stop_limit",
    }
}

pub(crate) fn order_type_of(name: &str) -> Result<OrderType, ExecutorError> {
    match name {
        "limit" => Ok(OrderType::Limit),
        "market" => Ok(OrderType::Market),
        "stop_limit" => Ok(OrderType::StopLimit),
        _ => Err(unknown("order_type")),
    }
}

pub(crate) fn tif_name(tif: TimeInForce) -> &'static str {
    match tif {
        TimeInForce::Day => "day",
        TimeInForce::Gtc => "gtc",
        TimeInForce::Ioc => "ioc",
    }
}

pub(crate) fn tif_of(name: &str) -> Result<TimeInForce, ExecutorError> {
    match name {
        "day" => Ok(TimeInForce::Day),
        "gtc" => Ok(TimeInForce::Gtc),
        "ioc" => Ok(TimeInForce::Ioc),
        _ => Err(unknown("tif")),
    }
}
