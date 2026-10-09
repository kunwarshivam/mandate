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
//! The Robinhood equity connector (E7-6, the first live trade brief's C1): connections spec
//! §6.2's `BrokerConnector` over the [`Tools`] seam, as DEC-860 reads it. `Submit` is review then
//! place with the [`ref_id`]; an alert refuses only an opening or an increase (`AGENTS.md` rule
//! 13); a place answer that is not exactly an order record is `Unknown` and never re-sent (LT-6);
//! `Cancel` goes by the broker's `order_id`. Numbers are decimal text read by `mandate-num`.

mod connector;
mod profile;

pub use connector::{RobinhoodConnector, Tools};
pub use profile::robinhood as robinhood_profile;

use mandate_canon::Digest;
use mandate_executor::ClientOrderId;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RobinhoodError {
    /// The stubs of this story's tests PR return it (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    #[error("the order state is none of the contract's ten")]
    UnknownState,
}

/// The `ref_id` for the order with this idempotency key (DEC-860 item 1): a UUID version 8
/// (RFC 9562 §5.8) whose 122 free bits are the first 16 bytes of the SHA-256 of the key's text,
/// written in lower case. A restart derives the same value, so a re-sent order carries it.
///
/// The version and variant bits are added to the kept bits rather than or-ed in: the two share no
/// bit, so the sum is the same value and never wraps, but an `|` there could be mutated to `^`
/// with nothing a test can see, because the bits it sets are the ones the mask has just cleared.
pub fn ref_id(key: &ClientOrderId) -> Result<String, RobinhoodError> {
    let digest = Digest::of(key.as_str().as_bytes());
    let mut bytes: Vec<u8> = digest.as_bytes().iter().take(16).copied().collect();
    if let Some(version) = bytes.get_mut(6) {
        *version = 0x80_u8.wrapping_add(*version & 0x0f);
    }
    if let Some(variant) = bytes.get_mut(8) {
        *variant = 0x80_u8.wrapping_add(*variant & 0x3f);
    }
    let mut text = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            text.push('-');
        }
        for nibble in [byte >> 4, byte & 0x0f] {
            text.extend(char::from_digit(u32::from(nibble), 16));
        }
    }
    Ok(text)
}

/// The status text the executor's §5.7 table reads for a contract `state` (connections spec
/// §6.2, DEC-860 item 3): `new`, `queued`, `confirmed` and `unconfirmed` are `accepted`;
/// `partially_filled` and `filled` are themselves; `cancelled` and `voided` are `canceled`;
/// `rejected` and `failed` are `rejected`. Any other value is [`RobinhoodError::UnknownState`].
pub fn executor_status(state: &str) -> Result<&'static str, RobinhoodError> {
    match state {
        "new" | "queued" | "confirmed" | "unconfirmed" => Ok("accepted"),
        "partially_filled" => Ok("partially_filled"),
        "filled" => Ok("filled"),
        "cancelled" | "voided" => Ok("canceled"),
        "rejected" | "failed" => Ok("rejected"),
        _ => Err(RobinhoodError::UnknownState),
    }
}
