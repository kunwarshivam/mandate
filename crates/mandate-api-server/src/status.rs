//! Command status (§5.5, §3.11 item 12, [DEC-2101](../../../docs/project/decisions/DEC-2101.md)
//! item 6): `GET /v1/workspaces/{ws}/commands/{event_id}`. A command's steps are the events whose
//! `causation_id` is it on each deployed agent's stream and each account stream the control stream
//! names, and its phase is the most final of theirs. Only the owning streams' events say what
//! happened (§6.3); a stream that cannot be read is `journal_unavailable`, never a shorter list.

use std::sync::Arc;

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use mandate_authn::Request;
use mandate_canon::{Int, Key, Object, Value, parse};
use mandate_identity::Permission;
use mandate_journal::StoredEvent;

use crate::http::App;
use crate::reads::deployed;
use crate::routes::{Refused, Unreadable, authorized, enveloped, path_refused, session, watermark};

/// A command's phase (§5.5), in the order a later one overrides an earlier one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    Recorded,
    Taken,
    Ended,
    Applied,
    Refused,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Self::Recorded => "recorded",
            Self::Taken => "taken",
            Self::Ended => "ended",
            Self::Applied => "applied",
            Self::Refused => "refused",
        }
    }
}

/// What one owning-stream event says of the command that caused it.
pub fn phase_of(_event_type: &str, _payload: &Value) -> Phase {
    Phase::Recorded
}

/// The command's phase over `steps`.
pub fn phase<'a>(_steps: impl IntoIterator<Item = (&'a str, &'a Value)>) -> Phase {
    Phase::Recorded
}

/// A step's reason.
pub fn reason(_payload: &Value) -> Option<&str> {
    None
}
