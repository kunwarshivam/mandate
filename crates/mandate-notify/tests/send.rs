//! NT-2 and DEC-710 items 4 to 6: a recipient is an opaque id, a provider is keyed by a digest of
//! the send and never by the recipient id, and the rendered message is the text, a newline, and
//! the link (notifications spec §4.3, §5.1, §5.2).

mod common;

use common::{ORIGIN, Recording, answer, hex};
use mandate_notify::{
    AddressHandle, IdempotencyKey, NoticeId, Notification, NotifyError, Origin, PushChannel,
    Recipient, TextKey, rendered,
};

const NOTICE: &str = "6f1c2a9e4b7d03581e2f9a6c4d8b0e17";

fn recipient(id: &str) -> Recipient {
    answer(id, Recipient::parse(id))
}

fn key(notice: &str, recipient_id: &str, channel: PushChannel) -> String {
    let address = AddressHandle {
        recipient: recipient(recipient_id),
        channel,
    };
    let notice = answer("notice", NoticeId::parse(notice));
    let key = answer("key", IdempotencyKey::of(&notice, &address));
    answer("hex", key.hex()).to_owned()
}

#[test]
#[ignore = "pending E8-9"]
fn a_recipient_is_1_to_64_of_lowercase_digits_and_underscore_starting_with_a_letter() {
    let longest = format!("a{}", "z_9".repeat(21));
    assert_eq!(longest.len(), 64);
    for id in ["a", "owner", "owner_2", "u_7", "z9", "q_", &longest] {
        answer(id, Recipient::parse(id));
    }
    assert_eq!(Recipient::parse("1"), Err(NotifyError::NotARecipient));
    assert_eq!(
        Recipient::parse(&format!("{longest}a")),
        Err(NotifyError::NotARecipient)
    );
}

/// NT-2: an address, a name, or anything else that is not an opaque id is refused, so none can be
/// journaled or keyed as a recipient.
#[test]
#[ignore = "pending E8-9"]
fn anything_but_an_opaque_recipient_id_is_refused() {
    answer("the control", Recipient::parse("owner"));
    let too_long = format!("a{}", "b".repeat(64));
    for id in [
        "",
        "1owner",
        "_owner",
        "Owner",
        "owNer",
        "OWNER",
        &too_long,
        "owner-2",
        "owner.2",
        "owner 2",
        " owner",
        "owner\n",
        "owner@example.com",
        "+15555550100",
        "ówner",
        "01j8znb0m000000000000000k1",
    ] {
        assert_eq!(
            Recipient::parse(id),
            Err(NotifyError::NotARecipient),
            "{id:?}"
        );
    }
}

/// The mandate schema's `notifications.channels` enum, read here as text so that no JSON reader of
/// the crate's own is the oracle.
fn schema_channels() -> Vec<String> {
    let schema = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../schemas/mandate.schema.json"
    ))
    .unwrap();
    let (_, notifications) = schema.split_once("\"notifications\": {").unwrap();
    let (_, channels) = notifications.split_once("\"channels\": {").unwrap();
    let (_, listed) = channels.split_once("\"enum\": [").unwrap();
    let (listed, _) = listed.split_once(']').unwrap();
    listed
        .split(',')
        .map(|c| c.trim().trim_matches('"').to_owned())
        .collect()
}

#[test]
#[ignore = "pending E8-9"]
fn the_push_channels_are_the_mandate_schemas_channels() {
    let listed = schema_channels();
    assert_eq!(listed.len(), 6, "{listed:?}");
    let keys: Vec<String> = PushChannel::ALL
        .into_iter()
        .map(|c| answer("key", c.key()).to_owned())
        .collect();
    assert_eq!(keys, listed);
}

/// DEC-710 item 5. Each expected digest is `printf '%s' '<json>' | sha256sum` over the canonical
/// form of `{"channel", "notice", "recipient"}` written by hand under `mandate-canon`'s rules
/// (RFC 8785: members sorted by key, no whitespace, strings in their shortest escape-free form),
/// for example `{"channel":"email","notice":"6f1c2a9e4b7d03581e2f9a6c4d8b0e17","recipient":"owner"}`.
#[test]
#[ignore = "pending E8-9"]
fn a_provider_is_keyed_by_the_sha256_of_the_canonical_send_triple() {
    for (channel, notice, recipient_id, digest) in [
        (
            PushChannel::Email,
            NOTICE,
            "owner",
            "b177305accb4e5ddbb2d527970491a28a0a454f3d780e04545170bea2b1b4605",
        ),
        (
            PushChannel::Phone,
            NOTICE,
            "owner",
            "1ee2dc019816b1c10ec2e15202d15bde4359b8d553e504100571088c238c0bc2",
        ),
        (
            PushChannel::Slack,
            NOTICE,
            "owner",
            "3c45cb67043e13ddc4d0a252fdcca2b620b0bf67b7c2e71c655787e984506b68",
        ),
        (
            PushChannel::Sms,
            NOTICE,
            "owner",
            "0ba1c40129d70f096e01f47ad9300ead212ca25c3b0d21849f010f2109379f54",
        ),
        (
            PushChannel::Telegram,
            NOTICE,
            "owner",
            "0d57ecaec9e91a81d655fe1e73b605e101a9e2e3b1f8761e0dfc4ed57a934607",
        ),
        (
            PushChannel::WebPush,
            NOTICE,
            "owner",
            "f237fc1b3d38c59e49537035d808bdf32dd08cb4a579dbf68d92f56845ae5b7d",
        ),
        (
            PushChannel::WebPush,
            "f0e1d2c3b4a5968778695a4b3c2d1e0f",
            "u_7",
            "a45663bf4a813af8ca2d03e2de74fec760aad79dbb6de7b6657524c238eecb3e",
        ),
        (
            PushChannel::Email,
            NOTICE,
            "owner_2",
            "6c61d0fbac59070ec9dca30fa873d69a0575caee3f1488e5c05d3a8cec8eda39",
        ),
    ] {
        assert_eq!(
            key(notice, recipient_id, channel),
            digest,
            "{channel:?} {notice} {recipient_id}"
        );
    }
}

/// Spec §5.1: a retry reuses its key, so the same send keys the same way every time.
#[test]
#[ignore = "pending E8-9"]
fn a_retry_is_keyed_as_its_first_attempt() {
    let first = key(NOTICE, "owner", PushChannel::Sms);
    assert_eq!(key(NOTICE, "owner", PushChannel::Sms), first);
    assert_eq!(first.len(), 64);
}

/// DEC-710 item 6. The texts are spec §4.2's, written here, not read from the crate.
#[test]
#[ignore = "pending E8-9"]
fn the_rendered_message_is_the_text_a_newline_and_the_link() {
    let texts = [
        "An agent in your workspace needs your approval",
        "Your workspace has a new alert",
        "There was a change to your account or workspace access",
        "Your daily brief is ready",
    ];
    for origin in [ORIGIN, "https://owlhead.corp.internal:8443"] {
        let parsed = answer("origin", Origin::parse(origin));
        let mut random = Recording::seeded(5);
        for (text, expected) in TextKey::ALL.into_iter().zip(texts) {
            let notice = answer("mint", NoticeId::mint(&mut random));
            let id = hex(random.issued.last().unwrap());
            assert_eq!(
                answer(
                    "rendered",
                    rendered(&parsed, &Notification { notice, text })
                ),
                format!("{expected}\n{origin}/n/{id}")
            );
        }
    }
}
