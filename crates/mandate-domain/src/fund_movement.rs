//! Whether a tool's name moves funds (DEC-839 item 3 as #888 amends it; story E7-12 with E7-16).
//! `mandate-mcp` refuses a connection whose tool list holds such a tool, and `mandate-connections`
//! refuses a credential whose scope or tool list names one; both ask here, so the two cannot drift
//! apart.
//!
//! The rule reads the name alone, never a description (CN-9). The name is split at every character
//! that is not an ASCII letter or digit, between a lowercase ASCII letter or a digit and the
//! uppercase ASCII letter after it, and inside a run of capitals before its last capital when a
//! lowercase letter follows that capital: `ACHDebit` is `ach` and `debit`, `HTTPSend` is `http` and
//! `send`, `v2Transfer` is `v2` and `transfer`. Each part is lower-cased. The name moves funds when
//! any part is one of [`FUND_MOVEMENT_TOKENS`], whole (`wireless`, `refund` and `fundamentals` are
//! not), or when it holds any character outside ASCII, whatever its parts: a homoglyph such as a
//! Cyrillic `і`, a fullwidth letter, or a zero-width character could otherwise hide a token.
//!
//! Only ASCII letters and digits are alphanumeric, so every other character separates parts:
//! `café_send` is `caf` and `send`.

/// The parts that make a name a fund-movement name, sorted by byte value (DEC-839 item 3 as #888
/// amends it).
pub const FUND_MOVEMENT_TOKENS: [&str; 15] = [
    "ach",
    "deposit",
    "deposits",
    "disburse",
    "fund",
    "funding",
    "funds",
    "payout",
    "send",
    "transfer",
    "transfers",
    "wire",
    "withdraw",
    "withdrawal",
    "withdrawals",
];

/// The name's parts in order, each lower-cased; empty for a name with no ASCII letter or digit.
///
/// A stub until E7-12's implementation PR: it has no error to carry, so its body is `todo!()`, the
/// other stub form DEC-137 names.
#[expect(
    clippy::todo,
    reason = "an infallible function has no error to carry, so its stub is todo!(), the other form DEC-137 names"
)]
pub fn name_tokens(name: &str) -> Vec<String> {
    let _ = name;
    todo!()
}

/// Whether any of the name's parts is in [`FUND_MOVEMENT_TOKENS`], or the name holds a character
/// outside ASCII.
///
/// A stub until E7-12's implementation PR, like [`name_tokens`].
#[expect(
    clippy::todo,
    reason = "an infallible function has no error to carry, so its stub is todo!(), the other form DEC-137 names"
)]
pub fn is_fund_movement_name(name: &str) -> bool {
    let _ = name;
    todo!()
}
