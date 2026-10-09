//! Whether a tool's name moves funds (DEC-839 item 3, as DEC-676 amends it; story E7-12 with
//! E7-16). `mandate-mcp` refuses a connection whose tool list holds such a tool, and
//! `mandate-connections` refuses a credential whose scope or tool list names one; both ask here, so
//! the two cannot drift apart.
//!
//! The rule reads the name alone, never a description (CN-9). The name is split at every character
//! that is not an ASCII letter or digit, between a lowercase ASCII letter and the uppercase ASCII
//! letter after it, and inside a run of capitals before its last capital when a lowercase letter
//! follows that capital (DEC-676's acronym split: `ACHDebit` is `ach` and `debit`, `HTTPSend` is
//! `http` and `send`); each part is lower-cased. The name moves funds when any part is one of
//! [`FUND_MOVEMENT_TOKENS`], whole: `wireless`, `refund` and `fundamentals` are not.
//!
//! DEC-839 does not say which characters are alphanumeric. Here only ASCII letters and digits are,
//! so every other character, `é` or a Cyrillic `і` included, separates parts. That reading refuses
//! at least every name the Unicode one refuses: its boundaries are a superset of the Unicode ones,
//! and every token in the set is ASCII.
//!
//! A digit followed by a capital is not a boundary, since it is neither change: `v2Transfer` is the
//! one part `v2transfer`.

/// The parts that make a name a fund-movement name, sorted by byte value (DEC-839 item 3: ten;
/// DEC-676 adds `deposit`, `deposits`, `fund`, `funds`, and `funding`).
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

/// Whether any of the name's parts is in [`FUND_MOVEMENT_TOKENS`].
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
