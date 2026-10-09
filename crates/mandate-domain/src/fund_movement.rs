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
pub fn name_tokens(name: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut previous: Option<char> = None;
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        if !c.is_ascii_alphanumeric() {
            push_part(&mut parts, &mut part);
            previous = None;
            continue;
        }
        if starts_part(previous, c, chars.peek().copied()) {
            push_part(&mut parts, &mut part);
        }
        part.push(c.to_ascii_lowercase());
        previous = Some(c);
    }
    push_part(&mut parts, &mut part);
    parts
}

/// Moves the part being built onto the parts, unless it is empty: a run of separators, or one at
/// either end of the name, makes no part.
fn push_part(parts: &mut Vec<String>, part: &mut String) {
    if !part.is_empty() {
        parts.push(std::mem::take(part));
    }
}

/// Whether the ASCII letter or digit `c` opens a new part when `previous` is the ASCII letter or
/// digit before it in the same run (`None` at the run's start) and `next` the character after it:
/// a capital after a lowercase letter or a digit, or a capital after a capital when a lowercase
/// letter follows it.
fn starts_part(previous: Option<char>, c: char, next: Option<char>) -> bool {
    c.is_ascii_uppercase()
        && match previous {
            Some(p) if p.is_ascii_lowercase() || p.is_ascii_digit() => true,
            Some(_) => next.is_some_and(|n| n.is_ascii_lowercase()),
            None => false,
        }
}

/// Whether any of the name's parts is in [`FUND_MOVEMENT_TOKENS`], or the name holds a character
/// outside ASCII.
pub fn is_fund_movement_name(name: &str) -> bool {
    !name.is_ascii()
        || name_tokens(name)
            .iter()
            .any(|part| FUND_MOVEMENT_TOKENS.binary_search(&part.as_str()).is_ok())
}
