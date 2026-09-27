//! The journal's names for this crate's enums, one table per enum. Both directions read the same
//! table, so the fold and the step can never spell a name differently, and a name missing from a
//! table fails both ways at once.

use mandate_accounting::Side;

use crate::error::ExecutorError;
use crate::types::{Mode, OrderState, OrderType, Purpose, TimeInForce};

const STATES: [(OrderState, &str); 13] = [
    (OrderState::Intent, "intent"),
    (OrderState::Submitting, "submitting"),
    (OrderState::Accepted, "accepted"),
    (OrderState::PartiallyFilled, "partially_filled"),
    (OrderState::PendingCancel, "pending_cancel"),
    (OrderState::PendingReplace, "pending_replace"),
    (OrderState::Unknown, "unknown"),
    (OrderState::Filled, "filled"),
    (OrderState::Canceled, "canceled"),
    (OrderState::Rejected, "rejected"),
    (OrderState::Expired, "expired"),
    (OrderState::Replaced, "replaced"),
    (OrderState::Abandoned, "abandoned"),
];

const PURPOSES: [(Purpose, &str); 7] = [
    (Purpose::Open, "open"),
    (Purpose::Increase, "increase"),
    (Purpose::RiskExit, "risk_exit"),
    (Purpose::OwnerExit, "owner_exit"),
    (Purpose::DiscretionaryExit, "discretionary_exit"),
    (Purpose::Protective, "protective"),
    (Purpose::Flatten, "flatten"),
];

const MODES: [(Mode, &str); 4] = [
    (Mode::Normal, "normal"),
    (Mode::ExitsOnly, "exits_only"),
    (Mode::Paused, "paused"),
    (Mode::Stopped, "stopped"),
];

const SIDES: [(Side, &str); 2] = [(Side::Buy, "buy"), (Side::Sell, "sell")];

const ORDER_TYPES: [(OrderType, &str); 3] = [
    (OrderType::Limit, "limit"),
    (OrderType::Market, "market"),
    (OrderType::StopLimit, "stop_limit"),
];

const TIFS: [(TimeInForce, &str); 3] = [
    (TimeInForce::Day, "day"),
    (TimeInForce::Gtc, "gtc"),
    (TimeInForce::Ioc, "ioc"),
];

/// The name of a value, from its table. Every value of every enum is in its table, which the
/// table's length pins; an absent one would name as the empty string and fold back as a refusal.
fn name<T: PartialEq + Copy>(table: &[(T, &'static str)], value: T) -> &'static str {
    table
        .iter()
        .find(|(known, _)| *known == value)
        .map_or("", |(_, name)| name)
}

/// The value a name stands for, or a refusal naming the payload field that carried it.
fn value<T: Copy>(
    table: &[(T, &'static str)],
    text: &str,
    field: &str,
) -> Result<T, ExecutorError> {
    table
        .iter()
        .find(|(_, name)| *name == text)
        .map(|(known, _)| *known)
        .ok_or_else(|| ExecutorError::NonCanonicalPayload {
            field: field.to_owned(),
        })
}

pub(crate) fn state_name(state: OrderState) -> &'static str {
    name(&STATES, state)
}

pub(crate) fn state_of(text: &str) -> Result<OrderState, ExecutorError> {
    value(&STATES, text, "state")
}

pub(crate) fn purpose_name(purpose: Purpose) -> &'static str {
    name(&PURPOSES, purpose)
}

pub(crate) fn purpose_of(text: &str) -> Result<Purpose, ExecutorError> {
    value(&PURPOSES, text, "purpose")
}

pub(crate) fn mode_of(text: &str) -> Result<Mode, ExecutorError> {
    value(&MODES, text, "to")
}

pub(crate) fn side_name(side: Side) -> &'static str {
    name(&SIDES, side)
}

pub(crate) fn side_of(text: &str) -> Result<Side, ExecutorError> {
    value(&SIDES, text, "side")
}

pub(crate) fn order_type_name(order_type: OrderType) -> &'static str {
    name(&ORDER_TYPES, order_type)
}

pub(crate) fn order_type_of(text: &str) -> Result<OrderType, ExecutorError> {
    value(&ORDER_TYPES, text, "order_type")
}

pub(crate) fn tif_name(tif: TimeInForce) -> &'static str {
    name(&TIFS, tif)
}

pub(crate) fn tif_of(text: &str) -> Result<TimeInForce, ExecutorError> {
    value(&TIFS, text, "tif")
}
