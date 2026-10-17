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
//! The cell's control-plane agent, `cp-agent` ([control-plane design §3](../../../docs/design/control-plane.md#3-components-and-interfaces);
//! backlog E20-3, [DEC-981]).
//!
//! `cp-agent` owns the deployment's one outbound link to the global control plane. Every inbound
//! instruction it receives is checked here, and only here, before anything on the site sees it:
//!
//! 1. its kind is one of the five inbound instructions of design §3.8 ([`MessageKind`]); any other
//!    is refused `out_of_list`;
//! 2. it is signed, by a key the installed release pins for that kind ([`PinnedKeys`]), and the
//!    signature verifies over its kind, key and canonical body (CP-5 (a)); never the TLS channel;
//! 3. its body is exactly its closed schema ([`Instruction`]), and a license names this site.
//!
//! What passes becomes a [`Verified`], which only [`receive`] can build, and is handed to workspace
//! control services through [`InstructionSink`], a port this crate declares and they implement,
//! the shape of `mandate-executor`'s `ports.rs`. What fails becomes a [`Refusal`], handed over the
//! same way so that they journal it (`ControlPlaneMessageRefused`, journal spec §9.17).
//!
//! `cp-agent` is not a journal writer (design CP-4, CP-5 (b)). This crate depends on no journal,
//! runtime, executor, mandate registry, or vault crate (`xtask/layers.toml`), so it cannot append,
//! hold a writer epoch, place an order, or read a credential. Anti-rollback (CP-9) is judged by
//! workspace services against the highest sequence they have journaled (DEC-981 item 4); a
//! [`Verified`] says nothing about order.
//!
//! [DEC-981]: ../../../docs/project/decisions/DEC-981.md

mod instruction;
mod port;
mod verify;

pub use instruction::{
    CatalogEntry, CatalogKind, DataBundle, Feature, Ident, Instruction, License, ReleaseManifest,
    ReleaseWithdrawn, Sequence, WithdrawalReason,
};
pub use port::{HandOver, InstructionSink, receive};
pub use verify::{
    KeyKind, MAX_MESSAGE_BYTES, MessageKind, PinnedKeys, Refusal, RefusalReason, Site, Verified,
    signed_bytes,
};
