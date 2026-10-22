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
//! The web-push address store (E8-14, DEC-2120). Each workspace's members' push addresses (the
//! endpoint, the keys, and the VAPID `key_id`) live here under their `address_ref`, sealed at
//! rest, never in the journal (workspace API §4.11, §5.7; notifications spec NT-2).
//!
//! **Two processes, two handles** (DEC-2120 items 1 and 3):
//!
//! - The workspace API (`owlhead_api`) opens an [`AddressWriter`], which writes, finds, lists,
//!   and deletes entries.
//! - The dispatcher (`owlhead_notify`) opens an [`AddressReader`], which reads one entry for one
//!   send and only for the member it belongs to. It has no write method, and the filesystem
//!   (`2750` directories, `0640` entries) gives that user no write permission either.
//!
//! Both load the one key, `vault-address-key`, from `$CREDENTIALS_DIRECTORY` and refuse to open
//! on any other arrangement ([`startup`]). No endpoint, key, digest, or hash reaches an error, a
//! `Display`, or a `Debug` output (DEC-2120 item 5).

pub mod error;
pub mod startup;
pub mod store;

pub use error::{AddressError, EntryFault};
pub use store::{
    AddressReader, AddressRef, AddressWriter, Deletion, Held, MemberId, NewEntry, PushKeys, Put,
    SendEntry, UnixNanos, Version, WorkspaceId, Written,
};

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "ADR-0001 ES-09 excepts tests from the safety-critical denies"
)]
mod tests;
