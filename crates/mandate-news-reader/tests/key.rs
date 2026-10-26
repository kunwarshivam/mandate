#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests: a wrong answer should stop the test"
)]
//! The news data key, read once (DEC-2277 items 3 and 4, DEC-2280 item 6): the vault's answer to
//! the one read decides `no_key`, `vault_unavailable`, or the key's own refusal, and no refusal,
//! error, or `Debug` output carries a byte of the key.

mod common;

use mandate_news_reader::{KeyRefusal, stored_key};
use mandate_sources::fetch::NewsKeyRefusal;
use mandate_vault::SourceError;
use secrecy::{ExposeSecret, SecretSlice};

use common::{PAPER_KEY, block_on};

fn stored(bytes: &[u8]) -> Result<Option<SecretSlice<u8>>, SourceError> {
    Ok(Some(SecretSlice::from(bytes.to_vec())))
}

#[test]
fn the_vault_s_answer_decides_the_key_or_why_there_is_none() {
    assert_eq!(stored_key(Ok(None)).err(), Some(KeyRefusal::NoKey));
    assert_eq!(
        stored_key(Err(SourceError::Missing)).err(),
        Some(KeyRefusal::NoKey)
    );
    for outage in [
        SourceError::Unavailable,
        SourceError::Denied,
        SourceError::Sealed,
        SourceError::Malformed,
        SourceError::Integrity,
        SourceError::RoleMissing,
    ] {
        let named = outage.code();
        assert_eq!(
            stored_key(Err(outage)).err(),
            Some(KeyRefusal::VaultUnavailable),
            "{named} is an outage, not a refusal of the key"
        );
    }
    assert_eq!(
        stored_key(stored(b"AKLIVE0001:secret")).err(),
        Some(KeyRefusal::Refused(NewsKeyRefusal::NotPaper))
    );
    assert_eq!(
        stored_key(stored(b"PKKEY no colon")).err(),
        Some(KeyRefusal::Refused(NewsKeyRefusal::Malformed))
    );
    let key = stored_key(stored(PAPER_KEY)).unwrap();
    assert_eq!(key.key_id().expose_secret(), "PKTESTKEYID0001");
    assert_eq!(key.secret().expose_secret(), "topsecret0001");
}

#[test]
fn each_refusal_is_its_code_alone() {
    let codes = [
        (KeyRefusal::NoKey, "no_key"),
        (KeyRefusal::VaultUnavailable, "vault_unavailable"),
        (KeyRefusal::Refused(NewsKeyRefusal::NotPaper), "not_paper"),
        (
            KeyRefusal::Refused(NewsKeyRefusal::Malformed),
            "news_key_malformed",
        ),
    ];
    for (refusal, code) in codes {
        assert_eq!(refusal.code(), code);
    }
}

#[test]
fn no_byte_of_the_key_reaches_a_refusal_an_error_or_debug_output() {
    let live = b"AKLIVEKEYID9:livesecret9";
    let shown = [
        format!("{:?}", stored_key(stored(PAPER_KEY))),
        format!("{:?}", stored_key(stored(live))),
        format!("{:?}", stored_key(stored(b"PKSPLIT:a:b"))),
        stored_key(stored(live)).err().unwrap().code().to_owned(),
    ];
    for text in shown {
        for part in [
            "PKTESTKEYID0001",
            "topsecret0001",
            "AKLIVEKEYID9",
            "livesecret9",
            "PKSPLIT",
        ] {
            assert!(!text.contains(part), "{part} in {text}");
        }
    }
}

#[test]
fn a_started_reader_never_shows_its_key() {
    let (journal, shelf, transport) = (
        common::Journal::default(),
        common::Shelf::default(),
        common::Recorded::default(),
    );
    common::spy(&journal, &shelf);
    transport.answer(common::page(&[], None));
    let mut reader = common::started(&journal, &shelf, &transport, common::t0());
    block_on(reader.step(common::t0()));
    let shown = format!("{:?} {:?}", reader.counts(), reader.lifecycle());
    for part in ["PKTESTKEYID0001", "topsecret0001"] {
        assert!(!shown.contains(part));
        for (url, _) in transport.sent.borrow().iter() {
            assert!(
                !url.contains(part),
                "the key travels only as headers: {url}"
            );
        }
    }
}
