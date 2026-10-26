#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The binding gate's reviewed inputs, one reading for every process that assembles a
//! [`mandate_executor::BindingGateInput`] (DEC-2380 item 5): the paper shell's production door and
//! the executor process read the same rule set, fee schedule and mandate limits through the same
//! functions, so the two can never judge one artifact two ways.
//!
//! Each function refuses a member it does not use and a value it cannot read (`AGENTS.md` rule 3);
//! nothing here computes a money or quantity figure (DEC-138 item 3): the fee reservation is
//! `mandate-accounting`'s and the fee schedule is `mandate-executor`'s.
//!
//! The deployment's confirmed mandate, its effective registrations and its approvers are folded
//! from the control stream here too ([`control`], [`approvers`]), so the executor process finds
//! its deployment as the shell does without depending on the shell.

pub mod approvers;
pub mod control;
mod error;
mod mandate;
mod reviewed;
mod rules;
mod view;

pub use error::InputError;
pub use mandate::reservation;
pub use reviewed::{Reviewed, ReviewedInstrument};
pub use rules::{
    Artifact, CALENDAR_MEMBERS, RULE_MEMBERS, artifact_from, environment_text, fee_artifact, fees,
    reference, required_positive_u64, required_text, rule_set,
};
pub use view::{DeployedView, account_fill_refs, fill_refs, protection_prices, stop_limit_offset};
