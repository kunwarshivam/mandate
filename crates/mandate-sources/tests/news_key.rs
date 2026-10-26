#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! The news reader's stored key (DEC-2277 items 2 and 4): one line `<key id>:<secret>`, a paper
//! key's id, and nothing of it in any refusal or debug output.

use mandate_sources::fetch::{NewsAuth, NewsKeyRefusal};
use proptest::prelude::*;
use secrecy::ExposeSecret;

#[test]
fn a_paper_key_line_reads_as_its_two_parts() {
    for stored in [
        &b"PKTESTKEYID:s3cretValue"[..],
        b"PKTESTKEYID:s3cretValue\n",
    ] {
        let auth = NewsAuth::from_stored(stored).unwrap();
        assert_eq!(auth.key_id().expose_secret(), "PKTESTKEYID");
        assert_eq!(auth.secret().expose_secret(), "s3cretValue");
        assert_eq!(format!("{auth:?}"), "NewsAuth(redacted)");
    }
}

#[test]
fn a_live_or_foreign_key_id_is_refused_not_paper() {
    for stored in [
        &b"AKLIVEKEYID:s3cret"[..],
        b"pkLOWER:s3cret",
        b"XPKID:s3cret",
        b"P:s3cret",
    ] {
        assert_eq!(
            NewsAuth::from_stored(stored).map(|_| ()),
            Err(NewsKeyRefusal::NotPaper),
            "{}",
            String::from_utf8_lossy(stored)
        );
    }
    assert_eq!(NewsKeyRefusal::NotPaper.code(), "not_paper");
}

#[test]
fn anything_but_one_clean_line_is_refused_malformed() {
    for stored in [
        &b""[..],
        b"\n",
        b"PKID",
        b"PKID:",
        b":s3cret",
        b"PKID:s3c:ret",
        b"PK ID:s3cret",
        b"PKID:s3cret ",
        b"PKID:s3cret\n\n",
        b"PKID:s3cret\r\n",
        b"PKID\t:s3cret",
        b"PKID:s3\x00cret",
        b"PKID:\xff\xfe",
    ] {
        assert_eq!(
            NewsAuth::from_stored(stored).map(|_| ()),
            Err(NewsKeyRefusal::Malformed),
            "{stored:?}"
        );
    }
    assert_eq!(NewsKeyRefusal::Malformed.code(), "news_key_malformed");
}

proptest! {
    /// Any clean paper key reads back exactly, against an oracle of its own: the line is split here
    /// by position, not by the reader's rule. A live key's refusal is the bare variant, which holds
    /// no part of the key.
    #[test]
    fn a_key_reads_back_or_is_refused_without_a_trace(
        id_tail in "[A-Za-z0-9]{1,24}",
        secret in "[A-Za-z0-9/+=]{1,48}",
        live in any::<bool>(),
    ) {
        let id = format!("{}{id_tail}", if live { "AK" } else { "PK" });
        let line = format!("{id}:{secret}");
        let read = NewsAuth::from_stored(line.as_bytes());
        if live {
            let refusal = read.map(|_| ()).unwrap_err();
            prop_assert_eq!(refusal, NewsKeyRefusal::NotPaper);
            prop_assert_eq!(format!("{refusal:?}"), "NotPaper");
        } else {
            let auth = read.unwrap();
            let colon = id.len();
            prop_assert_eq!(auth.key_id().expose_secret(), &line[..colon]);
            prop_assert_eq!(auth.secret().expose_secret(), &line[colon + 1..]);
        }
    }
}
