//! The captured-payload oracle every channel slice reuses (NT-1, DEC-710 item 7): a test seeds a
//! workspace whose data are these canaries, drives notices through an adapter, and scans every
//! byte it captured.

use crate::NotifyError;

/// One canary per kind of data NT-1 names: instrument, side, quantity, price, order value, P&L,
/// score, thesis, rule, deadline, agent name, mandate content, broker account, and personal data.
/// Each is lowercase, distinct, and holds `z` or `q`, so no hex id, digest, or fixed text holds one.
#[rustfmt::skip]
pub const CANARIES: [&str; 14] = [
    "zqinstrument", "zqside", "zqquantity", "zqprice", "zqordervalue", "zqpnl", "zqscore",
    "zqthesis", "zqrule", "zqdeadline", "zqagentname", "zqmandate", "zqbrokeraccount", "zqpersonal",
];

/// Every canary found anywhere in `captured`, ignoring ASCII case, once each and in the order of
/// [`CANARIES`]; empty for a clean capture.
///
/// # Errors
/// Never: any bytes can be scanned.
pub fn scan(captured: &[u8]) -> Result<Vec<&'static str>, NotifyError> {
    Ok(CANARIES
        .into_iter()
        .filter(|canary| {
            captured
                .windows(canary.len())
                .any(|window| window.eq_ignore_ascii_case(canary.as_bytes()))
        })
        .collect())
}
