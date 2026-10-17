//! The port between `cp-agent` and workspace control services (control-plane design §3, CP-4).
//! This crate declares it and they implement it, so the dependency runs from workspace services to
//! `cp-agent` and never back: `cp-agent` gains no path to the journal.

use crate::verify::{PinnedKeys, Refusal, RefusalReason, Site, Verified, check};

/// What workspace control services do with what `cp-agent` hands them. They are the control
/// stream's single writer: each call journals before any effect (CP-5 (b)).
pub trait InstructionSink {
    /// Judges `verified` against the highest sequence of its kind journaled (CP-9), journals its
    /// §9.17 record on each covered workspace's control stream, and only then applies it.
    fn hand_over(&mut self, verified: Verified) -> HandOver;

    /// Journals `refusal` as `ControlPlaneMessageRefused`.
    fn refused(&mut self, refusal: Refusal) -> HandOver;
}

/// The answer of workspace services to one hand-over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandOver {
    /// Journaled; for an instruction, its effect follows.
    Journaled,
    /// Its digest is that of the instruction in effect; nothing is journaled (DEC-981 item 5).
    AlreadyApplied,
    /// Refused by workspace services, and the refusal journaled: `Replayed` for CP-9.
    Refused(RefusalReason),
    /// Nothing was journaled and nothing took effect; `cp-agent` keeps the message and tries again.
    Unavailable,
}

/// Checks `bytes`, one inbound message as received, and hands the result to `sink`: a
/// [`Verified`] to [`InstructionSink::hand_over`], anything else to [`InstructionSink::refused`].
pub fn receive(
    bytes: &[u8],
    keys: &PinnedKeys,
    site: &Site,
    sink: &mut impl InstructionSink,
) -> HandOver {
    match check(bytes, keys, site) {
        Ok(verified) => sink.hand_over(verified),
        Err(refusal) => sink.refused(refusal),
    }
}
