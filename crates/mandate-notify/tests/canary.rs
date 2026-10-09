//! DEC-710 item 7: one canary per kind of data NT-1 names, and a scan that finds a planted one
//! anywhere in a capture, ignoring ASCII case.

mod common;

use std::collections::BTreeSet;

use common::{ORIGIN, answer};
use mandate_notify::canary::{CANARIES, scan};
use mandate_notify::{NoticeKind, TextKey};

/// Every fixed string a notice, its channels, or its records may carry, written here from spec
/// §3.2, §4.1, §4.2, §4.4, and §5.2 where the crate does not yet serve it.
const FIXED: &str = "email|phone|slack|sms|telegram|web_push|action|safety|info|timeout|rate_limited|\
    provider_error|address_rejected|auth_failed|too_large|recipient_not_permitted|address_missing|\
    bounced|complained|unsubscribed|not_pending|retry_window_ended|Open Owlhead to see it.|\
    [[EMAIL-FOOTER]]|Sent by your Mandate workspace to its owner.|fixture-1";

fn fixed_texts() -> Vec<String> {
    let mut texts: Vec<String> = FIXED.split('|').map(str::to_owned).collect();
    texts.push(ORIGIN.to_owned());
    for key in TextKey::ALL {
        texts.push(answer("key", key.key()).to_owned());
        texts.push(answer("text", key.text()).to_owned());
    }
    for kind in NoticeKind::ALL {
        texts.push(answer("kind", kind.key()).to_owned());
    }
    texts
}

fn found(captured: &[u8]) -> Vec<&'static str> {
    answer("scan", scan(captured))
}

#[test]
fn the_canaries_are_fourteen_distinct_lowercase_markers_no_fixed_text_or_hex_holds() {
    let distinct: BTreeSet<&str> = CANARIES.into_iter().collect();
    assert_eq!(distinct.len(), 14);
    let texts = fixed_texts();
    assert_eq!(texts.len(), 27 + 8 + 33);
    for canary in CANARIES {
        assert!(!canary.is_empty());
        assert!(canary.bytes().all(|b| b.is_ascii_lowercase()), "{canary}");
        assert!(canary.contains('z') || canary.contains('q'), "{canary}");
        assert!(!canary.bytes().all(|b| b.is_ascii_hexdigit()), "{canary}");
        for other in CANARIES.into_iter().filter(|o| *o != canary) {
            assert!(!other.contains(canary), "{other} holds {canary}");
        }
        for text in &texts {
            assert!(
                !text.to_ascii_lowercase().contains(canary),
                "{text} holds {canary}"
            );
        }
    }
}

#[test]
fn the_scan_finds_a_canary_at_the_start_in_the_middle_and_at_the_end() {
    for canary in CANARIES {
        for captured in [
            canary.to_owned(),
            format!("{canary} Your workspace has a new alert"),
            format!("Your workspace {canary}has a new alert"),
            format!("Your workspace has a new alert\n{ORIGIN}/n/{canary}"),
        ] {
            assert_eq!(found(captured.as_bytes()), [canary], "{captured:?}");
        }
    }
}

#[test]
fn the_scan_ignores_ascii_case() {
    for captured in ["ZQPRICE", "zQpRiCe", "Your daily brief is ready: ZqPrice."] {
        assert_eq!(found(captured.as_bytes()), ["zqprice"], "{captured}");
    }
}

#[test]
fn the_scan_names_each_canary_present_once_in_canary_order() {
    let mut planted: Vec<&str> = CANARIES.into_iter().rev().collect();
    planted.extend(CANARIES);
    assert_eq!(found(planted.join("|").as_bytes()), CANARIES);
    assert_eq!(
        found(b"zqrule, zqside and zqrule again"),
        ["zqside", "zqrule"]
    );
}

/// A clean capture names nothing, a near miss is not a canary, and a canary between bytes that are
/// not UTF-8 is still found.
#[test]
fn a_clean_capture_names_nothing_and_a_canary_in_any_bytes_is_found() {
    for clean in [
        "",
        "{\"notice\":\"6f1c2a9e4b7d03581e2f9a6c4d8b0e17\",\"text\":\"approval_needed\"}",
        "An agent in your workspace needs your approval\nhttps://app.owlhead.example/n/6f1c2a9e4b7d03581e2f9a6c4d8b0e17",
        "b177305accb4e5ddbb2d527970491a28a0a454f3d780e04545170bea2b1b4605",
        "zq zqpric zq price qzprice zqpn-l",
    ] {
        assert_eq!(found(clean.as_bytes()), [] as [&str; 0], "{clean}");
    }
    let mut dirty = vec![0xff, 0xfe];
    dirty.extend(b"zqPNL");
    dirty.push(0xc3);
    assert_eq!(found(&dirty), ["zqpnl"]);
}
