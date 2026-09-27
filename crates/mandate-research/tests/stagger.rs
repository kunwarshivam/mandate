//! The §8.4 stagger offset and its anchor, plus the proposal cadence.
//!
//! MC-N23's three offsets were recomputed by hand before this file existed:
//! `SHA-256("ws_a\x00th-1")` begins `0x35791baa50c83366…`, and that 256-bit integer modulo 900 is
//! **317**; `ws_b`/`th-1` gives **682** and `ws_a`/`th-2` gives **615**. The reduction's oracle in
//! [`reduction_least_significant_first`] walks the same digest from the other end, so the two share
//! no arithmetic.

mod common;

use common::{at, thesis_id, workspace};
use mandate_canon::Digest;
use mandate_research::{
    AssetClass, StaggerWindow, next_proposal_at, stagger_offset, stagger_release_at,
};

/// The oracle: `sum(byte_k * 256^k) mod window`, walked from the least significant byte with its own
/// table of powers, where the implementation folds from the most significant (DEC-132 item 8).
fn reduction_least_significant_first(digest: &Digest, window: u32) -> u32 {
    if window == 0 {
        return 0;
    }
    let modulus = u64::from(window);
    let mut total: u64 = 0;
    let mut power: u64 = 1;
    for byte in digest.as_bytes().iter().rev() {
        total = (total + (u64::from(*byte) * power)) % modulus;
        power = (power * 256) % modulus;
    }
    u32::try_from(total).unwrap_or(u32::MAX)
}

fn digest_of(workspace_id: &str, thesis: &str) -> Digest {
    Digest::of_parts(&[workspace_id.as_bytes(), &[0x00], thesis.as_bytes()])
}

/// MC-N23, and the oracle agreeing on each of the three pairs.
#[test]
fn mc_n23_the_three_fixture_offsets() {
    let window = StaggerWindow(900);
    let pairs = [
        ("ws_a", "th-1", 317),
        ("ws_b", "th-1", 682),
        ("ws_a", "th-2", 615),
    ];

    for (ws, th, expected) in pairs {
        let offset =
            stagger_offset(&workspace(ws), &thesis_id(th), window).expect("the offset is exact");
        assert_eq!(
            offset, expected,
            "MC-N23: SHA-256({ws} \\x00 {th}) big-endian mod 900 is {expected}"
        );
        assert_eq!(
            offset,
            reduction_least_significant_first(&digest_of(ws, th), 900),
            "the least-significant-first oracle reduces the same digest to the same offset"
        );
    }
}

/// The offset depends on both ids, which is what spreads one thesis's flow across workspaces
/// (DEC-100).
#[test]
fn both_ids_change_the_offset() {
    let window = StaggerWindow(900);
    let a = stagger_offset(&workspace("ws_a"), &thesis_id("th-1"), window)
        .expect("the offset is exact");
    let other_workspace = stagger_offset(&workspace("ws_b"), &thesis_id("th-1"), window)
        .expect("the offset is exact");
    let other_thesis = stagger_offset(&workspace("ws_a"), &thesis_id("th-2"), window)
        .expect("the offset is exact");

    assert_ne!(
        a, other_workspace,
        "two workspaces on one thesis must not act at once, which is the whole point of the offset"
    );
    assert_ne!(
        a, other_thesis,
        "one workspace's two theses are staggered against each other as well"
    );
}

/// §8.4: a window of 0 means no wait.
#[test]
fn a_zero_window_gives_a_zero_offset() {
    let offset = stagger_offset(&workspace("ws_a"), &thesis_id("th-1"), StaggerWindow(0))
        .expect("zero is exact");

    assert_eq!(offset, 0, "§8.4: a window of 0 means no wait");
}

/// A one-second window leaves one possible offset, which pins the modulus rather than the digest.
#[test]
fn a_one_second_window_gives_a_zero_offset() {
    let offset = stagger_offset(&workspace("ws_a"), &thesis_id("th-1"), StaggerWindow(1))
        .expect("one is exact");

    assert_eq!(offset, 0, "every integer modulo 1 is 0");
}

/// The hand-computed digest prefix, so a change in how the parts are joined is caught here and not
/// only through the offset.
#[test]
fn the_digest_is_the_workspace_a_zero_byte_and_the_thesis() {
    let expected = digest_of("ws_a", "th-1");

    assert_eq!(
        &expected.to_hex()[..16],
        "35791baa50c83366",
        "§8.4 joins workspace_id, one 0x00 byte, and thesis_id, in that order"
    );
    assert_eq!(
        stagger_offset(&workspace("ws_a"), &thesis_id("th-1"), StaggerWindow(900))
            .expect("the offset is exact"),
        reduction_least_significant_first(&expected, 900),
        "the crate hashes exactly those three parts"
    );
}

/// §8.4: an equity counts from the later of the admission and the next regular-session open.
#[test]
fn an_equity_release_waits_for_the_later_of_the_two_instants() {
    let admitted = at("2026-09-22T08:00:00.000000000Z");
    let open = at("2026-09-22T13:30:00.000000000Z");

    let release = stagger_release_at(AssetClass::UsEquity, admitted, Some(open), 317)
        .expect("the release is decided");
    assert_eq!(
        release,
        at("2026-09-22T13:35:17.000000000Z"),
        "the admission is before the open, so the offset runs from the open: 13:30:00 + 317 s"
    );
}

/// An admission inside the session is the later instant, so the offset runs from it.
#[test]
fn an_equity_admitted_inside_the_session_counts_from_the_admission() {
    let admitted = at("2026-09-22T14:00:00.000000000Z");
    let open = at("2026-09-22T13:30:00.000000000Z");

    let release = stagger_release_at(AssetClass::UsEquity, admitted, Some(open), 317)
        .expect("the release is decided");
    assert_eq!(
        release,
        at("2026-09-22T14:05:17.000000000Z"),
        "the later of the two is the admission, so a thesis admitted mid-session is not sent backwards"
    );
}

/// §8.4: crypto trades continuously, so the offset runs from the admission and no calendar is needed.
#[test]
fn a_crypto_release_is_counted_from_the_admission() {
    let admitted = at("2026-09-22T03:00:00.000000000Z");

    let release = stagger_release_at(AssetClass::Crypto, admitted, None, 615)
        .expect("the release is decided");
    assert_eq!(
        release,
        at("2026-09-22T03:10:15.000000000Z"),
        "615 s after 03:00:00, with no session to wait for"
    );
}

/// A zero offset releases at the anchor itself.
#[test]
fn a_zero_offset_releases_at_the_anchor() {
    let admitted = at("2026-09-22T14:00:00.000000000Z");

    let release =
        stagger_release_at(AssetClass::Crypto, admitted, None, 0).expect("the release is decided");
    assert_eq!(release, admitted, "no wait means the anchor itself");
}

/// An equity with no next regular open is an error, not a release at the admission: guessing would
/// put an opening order outside the session §8.4 requires.
#[test]
fn an_equity_without_a_next_regular_open_is_an_error() {
    let admitted = at("2026-09-22T08:00:00.000000000Z");

    let e = stagger_release_at(AssetClass::UsEquity, admitted, None, 317)
        .expect_err("an equity anchor needs the calendar");
    assert_eq!(
        e.code(),
        "session_calendar_missing",
        "§8.4 counts an equity's wait from the next regular open, so the open is required"
    );
}

/// §8.4's proposal cadence: the interval is a policy minimum, and the crate reads no clock.
#[test]
fn the_next_proposal_is_one_interval_after_the_last() {
    let last = at("2026-09-22T14:00:00.000000000Z");

    let next = next_proposal_at(Some(last), 3_600).expect("the cadence is decided");
    assert_eq!(
        next,
        Some(at("2026-09-22T15:00:00.000000000Z")),
        "the research_equity base's interval_s is 3600, so one hour after the last proposal"
    );
}

/// Before the first proposal there is no earliest instant to wait for.
#[test]
fn an_agent_that_has_never_proposed_may_propose_now() {
    let next = next_proposal_at(None, 3_600).expect("the cadence is decided");

    assert_eq!(
        next, None,
        "with no last proposal there is nothing to wait for, and the caller's own clock decides"
    );
}
