//! NT-1 and NT-4: the payload is exactly `{"notice", "text"}`, and a link is the fixed origin and
//! the notice id and nothing else (notifications spec §4.2, §4.3).

mod common;

use common::{ORIGIN, Recording, answer, hex};
use mandate_canon::to_canonical;
use mandate_notify::{NoticeId, Notification, NotifyError, Origin, TextKey, link, payload};

#[test]
fn the_payload_is_exactly_the_notice_id_and_the_text_key() {
    let keys = [
        "approval_needed",
        "attention_needed",
        "account_changed",
        "brief_ready",
    ];
    let mut random = Recording::seeded(11);
    for (text, key) in TextKey::ALL.into_iter().zip(keys) {
        let notice = answer("mint", NoticeId::mint(&mut random));
        let bytes = to_canonical(&answer("payload", payload(&Notification { notice, text })));
        let id = hex(random.issued.last().unwrap());
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            format!(r#"{{"notice":"{id}","text":"{key}"}}"#)
        );
    }
}

#[test]
fn a_link_is_the_origin_then_n_then_the_notice_id() {
    for origin in [ORIGIN, "https://owlhead.corp.internal:8443"] {
        let mut random = Recording::seeded(3);
        let notice = answer("mint", NoticeId::mint(&mut random));
        let parsed = answer("origin", Origin::parse(origin));
        assert_eq!(
            answer("link", link(&parsed, &notice)),
            format!("{origin}/n/{}", hex(&random.issued[0]))
        );
    }
}

#[test]
fn an_origin_is_https_a_lowercase_host_and_an_optional_port() {
    for origin in [
        ORIGIN,
        "https://localhost",
        "https://a",
        "https://app-1.owlhead.example",
        "https://10.0.0.7:1",
        "https://app.owlhead.example:65535",
    ] {
        answer(origin, Origin::parse(origin));
    }
}

/// DEC-710 item 3: nothing but the origin, so a link can hold no token, query, or tracking.
#[test]
fn an_origin_with_anything_else_is_refused() {
    for origin in [
        "",
        "https://",
        "http://app.owlhead.example",
        "HTTPS://app.owlhead.example",
        "app.owlhead.example",
        "https://App.owlhead.example",
        "https://app.owlhead.example/",
        "https://app.owlhead.example/n",
        "https://app.owlhead.example?t=1",
        "https://app.owlhead.example#x",
        "https://user:pw@app.owlhead.example",
        "https://app..owlhead.example",
        "https://.owlhead.example",
        "https://app.owlhead.example.",
        "https://app_1.owlhead.example",
        "https://[::1]",
        "https://[::1]:8443",
        "https://-app.owlhead.example",
        "https://app-.owlhead.example",
        "https://app.owlhead.example..",
        "https://app.owlhead.example:",
        "https://app.owlhead.example:0",
        "https://app.owlhead.example:08443",
        "https://app.owlhead.example:+443",
        "https://app.owlhead.example:65536",
        "https://app.owlhead.example:8443:1",
        " https://app.owlhead.example",
        "https://app.owlhead.example ",
        "https://app.owlhead.example\n",
        "https://äpp.owlhead.example",
    ] {
        assert_eq!(
            Origin::parse(origin),
            Err(NotifyError::InvalidOrigin),
            "{origin:?}"
        );
    }
}
